//! Shared native manifest and staged copy implementation.
use super::{attributes, digest, fail, identity, require};
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, Timestamps, Timespec, CWD};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use xattr::FileExt;

type Attrs = BTreeMap<OsString, Vec<u8>>;
pub(crate) type Identity = (u64, u64, u32, u64, i64, i64, i64, i64);
#[derive(Debug, PartialEq, Eq)]
enum Body { Directory, Link(PathBuf), File(u64, [u8; 32]) }
#[derive(Debug, PartialEq, Eq)]
struct Item {
    mode: u32, uid: u32, gid: u32, seconds: i64, nanos: i64,
    attrs: Attrs, body: Body,
}
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Manifest {
    items: BTreeMap<PathBuf, Item>,
    groups: Vec<Vec<PathBuf>>,
}
pub(crate) type Snapshot = BTreeMap<PathBuf, Identity>;
type Links = BTreeMap<(u64, u64), Vec<PathBuf>>;

pub(crate) fn fd_path(f: &File) -> PathBuf {
    PathBuf::from(format!("/proc/self/fd/{}", f.as_raw_fd()))
}
fn open(path: &Path, directory: bool) -> io::Result<File> {
    let mut flags = OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
    if directory { flags |= OFlags::DIRECTORY; }
    Ok(rx::open(path, flags, Mode::empty())?.into())
}
fn path_attrs(p: &Path) -> io::Result<Attrs> {
    let mut result = BTreeMap::new();
    for key in xattr::list(p)? {
        let value = xattr::get(p, &key)?.ok_or_else(|| fail("xattr disappeared"))?;
        result.insert(key, value);
    }
    Ok(result)
}
fn names(dir: &File) -> io::Result<Vec<OsString>> {
    let mut result = fs::read_dir(fd_path(dir))?
        .map(|entry| entry.map(|e| e.file_name())).collect::<io::Result<Vec<_>>>()?;
    result.sort();
    Ok(result)
}
fn checked_open(path: &Path, m: &Metadata, dir: bool) -> io::Result<File> {
    let f = open(path, dir)?;
    require(identity(&f.metadata()?) == identity(m), "entry changed before open")?;
    Ok(f)
}
fn check(path: &Path, original: &Metadata) -> io::Result<()> {
    require(identity(&fs::symlink_metadata(path)?) == identity(original), "tree entry changed")
}
fn collect(path: &Path, relative: &Path, device: u64, flush: bool,
           items: &mut BTreeMap<PathBuf, Item>, snapshots: &mut Snapshot,
           links: &mut Links) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    require(m.dev() == device, "cross-device entry refused")?;
    let (body, attrs) = if m.is_dir() {
        let dir = checked_open(path, &m, true)?;
        let attrs = attributes(&dir)?;
        for name in names(&dir)? {
            collect(&fd_path(&dir).join(&name), &relative.join(&name), device,
                    flush, items, snapshots, links)?;
        }
        require(identity(&dir.metadata()?) == identity(&m), "directory changed during traversal")?;
        if flush { dir.sync_all()?; }
        (Body::Directory, attrs)
    } else if m.is_file() {
        let file = checked_open(path, &m, false)?;
        let attrs = attributes(&file)?;
        let hash = digest(&file)?;
        require(identity(&file.metadata()?) == identity(&m), "file changed during traversal")?;
        if flush { file.sync_all()?; }
        links.entry((m.dev(), m.ino())).or_default().push(relative.to_path_buf());
        (Body::File(m.size(), hash), attrs)
    } else if m.file_type().is_symlink() {
        (Body::Link(fs::read_link(path)?), path_attrs(path)?)
    } else { return Err(fail("special file refused")); };
    check(path, &m)?;
    snapshots.insert(relative.to_path_buf(), identity(&m));
    items.insert(relative.to_path_buf(), Item { mode: m.mode(), uid: m.uid(), gid: m.gid(),
        seconds: m.mtime(), nanos: m.mtime_nsec(), attrs, body });
    Ok(())
}
pub(crate) fn manifest(path: &Path, flush: bool) -> io::Result<(Manifest, Snapshot)> {
    let device = fs::symlink_metadata(path)?.dev();
    let mut items = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut links = Links::new();
    collect(path, Path::new("."), device, flush, &mut items, &mut snapshots, &mut links)?;
    let mut groups: Vec<_> = links.into_values().filter(|g| g.len() > 1).collect();
    for g in &mut groups { g.sort(); }
    groups.sort();
    Ok((Manifest { items, groups }, snapshots))
}
fn metadata(f: &File, original: &Metadata, attrs: &Attrs) -> io::Result<()> {
    // Refuse rather than silently change ownership. Fixture files share owner.
    let actual = f.metadata()?;
    require(actual.uid() == original.uid() && actual.gid() == original.gid(), "ownership differs")?;
    for (name, value) in attrs { f.set_xattr(name, value)?; }
    f.set_permissions(fs::Permissions::from_mode(original.mode() & 0o7777))?;
    f.set_times(FileTimes::new().set_modified(original.modified()?))?;
    f.sync_all()
}
fn copy_entry(src: &Path, dst: &Path, device: u64, checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
              hardlinks: &mut BTreeMap<(u64, u64), PathBuf>) -> io::Result<()> {
    let m = fs::symlink_metadata(src)?;
    require(m.dev() == device, "cross-device source refused")?;
    if m.is_dir() {
        let input = checked_open(src, &m, true)?;
        let attrs = attributes(&input)?;
        rx::mkdir(dst, Mode::from_bits_truncate(0o700))?;
        let output = open(dst, true)?;
        for name in names(&input)? {
            copy_entry(&fd_path(&input).join(&name), &fd_path(&output).join(&name),
                       device, checkpoint, hardlinks)?;
        }
        metadata(&output, &m, &attrs)?;
    } else if m.is_file() {
        let key = (m.dev(), m.ino());
        if let Some(previous) = hardlinks.get(&key) {
            fs::hard_link(previous, dst)?;
        } else {
            let mut input = checked_open(src, &m, false)?;
            let attrs = attributes(&input)?;
            let mut output: File = rx::open(dst, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL
                | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_bits_truncate(0o600))?.into();
            let mut buf = vec![0; 1024 * 1024];
            loop {
                let n = input.read(&mut buf)?;
                if n == 0 { break; }
                output.write_all(&buf[..n])?;
                checkpoint("copy-partial")?;
            }
            metadata(&output, &m, &attrs)?;
            // Store a stable private staging path, not a transient child fd.
            hardlinks.insert(key, dst.canonicalize()?);
        }
    } else if m.file_type().is_symlink() {
        symlink(fs::read_link(src)?, dst)?;
        for (key, value) in path_attrs(src)? { xattr::set(dst, key, &value)?; }
        let times = Timestamps {
            last_access: Timespec { tv_sec: m.atime(), tv_nsec: m.atime_nsec() as _ },
            last_modification: Timespec { tv_sec: m.mtime(), tv_nsec: m.mtime_nsec() as _ },
        };
        rx::utimensat(CWD, dst, &times, AtFlags::SYMLINK_NOFOLLOW)?;
    } else { return Err(fail("special file refused")); }
    check(src, &m)
}
pub(crate) fn copy_tree(src: &Path, payload: &Path, checkpoint: &mut dyn FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    let original = fs::symlink_metadata(src)?;
    let (expected, snapshot) = manifest(src, false)?;
    copy_entry(src, payload, original.dev(), checkpoint, &mut BTreeMap::new())?;
    require(manifest(payload, true)?.0 == expected, "staged tree differs from original")?;
    require(manifest(src, false)? == (expected, snapshot), "source tree changed")
}
