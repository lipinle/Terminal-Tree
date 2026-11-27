//! Shared native manifest and staged copy implementation.
use super::{attributes, digest, fail, identity, require};
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, Timestamps, Timespec, CWD};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, Read, Write, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use xattr::FileExt;

type Attrs = BTreeMap<OsString, Vec<u8>>;
pub(crate) type Identity = (u64, u64, u32, u64, i64, i64, i64, i64);
#[derive(Debug, Clone, PartialEq, Eq)]
enum Body { Directory, Link(PathBuf), File(u64, [u8; 32]) }
#[derive(Debug, Clone, PartialEq, Eq)]
struct Item {
    mode: u32, uid: u32, gid: u32, seconds: i64, nanos: i64,
    attrs: Attrs, body: Body,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Manifest {
    items: BTreeMap<PathBuf, Item>,
    groups: Vec<Vec<PathBuf>>,
}
pub(crate) type Snapshot = BTreeMap<PathBuf, Identity>;
type Links = BTreeMap<(u64, u64), Vec<PathBuf>>;
type HashCache = BTreeMap<(u64,u64), (Identity,[u8;32])>;

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
           links: &mut Links, buffer: &mut [u8], hashes: &mut HashCache,checkpoint:&mut dyn FnMut(&str)->io::Result<()>) -> io::Result<()> {
    checkpoint("manifest-entry")?;
    let m = fs::symlink_metadata(path)?;
    require(m.dev() == device, "cross-device entry refused")?;
    let (body, attrs) = if m.is_dir() {
        let dir = checked_open(path, &m, true)?;
        let attrs = attributes(&dir)?;
        for name in names(&dir)? {
            collect(&fd_path(&dir).join(&name), &relative.join(&name), device,
                    flush, items, snapshots, links, buffer, hashes,checkpoint)?;
        }
        require(identity(&dir.metadata()?) == identity(&m), "directory changed during traversal")?;
        if flush { dir.sync_all()?; }
        (Body::Directory, attrs)
    } else if m.is_file() {
        let file = checked_open(path, &m, false)?;
        let attrs = attributes(&file)?;
        // Cache only within this traversal, for multiple names of one inode.
        // A separate source/destination manifest always starts with an empty cache.
        let key = (m.dev(),m.ino());
        let hash = if let Some((id, hash)) = hashes.get(&key) {
            require(*id == identity(&m), "hard-linked inode changed during traversal")?;
            *hash
        } else {
            let hash = digest(&file, buffer,checkpoint)?;
            if flush { file.sync_all()?; }
            if m.nlink() > 1 { hashes.insert(key,(identity(&m),hash)); }
            hash
        };
        require(identity(&file.metadata()?) == identity(&m), "file changed during traversal")?;
        // Single-link files cannot contribute to a multi-name group.
        // Still hash and verify every file and its identity independently.
        if m.nlink()>1 {links.entry((m.dev(), m.ino())).or_default().push(relative.to_path_buf());}
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
    manifest_with(path,flush,&mut |_|Ok(()))
}
pub(crate) fn manifest_with(path:&Path,flush:bool,checkpoint:&mut dyn FnMut(&str)->io::Result<()>)->io::Result<(Manifest,Snapshot)>{
    let mut buffer=vec![0;1024*1024];
    manifest_buffered(path,flush,checkpoint,&mut buffer)
}
fn manifest_buffered(path:&Path,flush:bool,checkpoint:&mut dyn FnMut(&str)->io::Result<()>,buffer:&mut [u8])->io::Result<(Manifest,Snapshot)>{
    let device = fs::symlink_metadata(path)?.dev();
    let mut items = BTreeMap::new();
    let mut snapshots = BTreeMap::new();
    let mut links = Links::new();
    collect(path, Path::new("."), device, flush, &mut items, &mut snapshots, &mut links, buffer, &mut HashCache::new(),checkpoint)?;
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
    Ok(())
}
// Only unsupported-operation errors permit a slower implementation. Disk I/O,
// ENOSPC and permission errors remain fatal; verification never becomes optional.
fn unsupported(e: rustix::io::Errno) -> bool {
    matches!(e, rustix::io::Errno::OPNOTSUPP | rustix::io::Errno::XDEV
        | rustix::io::Errno::NOSYS | rustix::io::Errno::INVAL | rustix::io::Errno::NOTTY)
}
fn copy_bytes(input: &mut File, output: &mut File, size: u64, buffer: &mut [u8],
              checkpoint: &mut dyn FnMut(&str) -> io::Result<()>) -> io::Result<()> {
    // FICLONE uses the already opened private destination, never a pathname.
    // A successful clone is still read independently by the staged manifest.
    match rx::ioctl_ficlone(&*output, &*input) {
        Ok(()) => {
            require(output.metadata()?.len() == size, "reflink length mismatch")?;
            checkpoint("copy-reflink")?;
            checkpoint(&format!("copy-bytes:{size}"))?;
            if size != 0 { checkpoint("copy-partial")?; }
            return Ok(());
        }
        Err(e) if unsupported(e) => {
            // Reset a private unpublished payload before trying another method.
            output.set_len(0)?;
        }
        Err(e) => return Err(e.into()),
    }
    let mut copied = 0u64;
    while copied < size {
        let count = (size - copied).min(buffer.len() as u64) as usize;
        match rx::copy_file_range(&*input, None, &*output, None, count) {
            Ok(0) => break,
            Ok(n) => {
                copied += n as u64;
                checkpoint(&format!("copy-bytes:{n}"))?;
                checkpoint("copy-partial")?;
            }
            Err(e) if e == rustix::io::Errno::INTR => continue,
            Err(e) if unsupported(e) => break,
            Err(e) => return Err(e.into()),
        }
    }
    // Continue at the exact confirmed offset, including a partially completed
    // kernel copy. Never silently accept short reads or a changed source.
    input.seek(SeekFrom::Start(copied))?;
    output.seek(SeekFrom::Start(copied))?;
    while copied < size {
        let count = (size - copied).min(buffer.len() as u64) as usize;
        let n = input.read(&mut buffer[..count])?;
        require(n > 0, "source shortened during copy")?;
        output.write_all(&buffer[..n])?;
        copied += n as u64;
        checkpoint(&format!("copy-bytes:{n}"))?;
        checkpoint("copy-partial")?;
    }
    require(output.metadata()?.len() == size, "copied length mismatch")
}
fn copy_entry(src: &Path, dst: &Path, device: u64, checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
              hardlinks: &mut BTreeMap<(u64, u64), PathBuf>, buffer: &mut [u8]) -> io::Result<()> {
    checkpoint("copy-entry")?;
    let m = fs::symlink_metadata(src)?;
    require(m.dev() == device, "cross-device source refused")?;
    if m.is_dir() {
        let input = checked_open(src, &m, true)?;
        let attrs = attributes(&input)?;
        rx::mkdir(dst, Mode::from_bits_truncate(0o700))?;
        let output = open(dst, true)?;
        for name in names(&input)? {
            copy_entry(&fd_path(&input).join(&name), &fd_path(&output).join(&name),
                       device, checkpoint, hardlinks, buffer)?;
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
            copy_bytes(&mut input, &mut output, m.size(), buffer, checkpoint)?;
            metadata(&output, &m, &attrs)?;
            // Store a stable private staging path, not a transient child fd.
            if m.nlink() > 1 { hardlinks.insert(key, dst.canonicalize()?); }
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
// Metadata-only rechecks supplement, rather than replace, content manifests.
fn collect_snapshot(path: &Path, rel: &Path, device: u64, out: &mut Snapshot) -> io::Result<()> {
    let m = fs::symlink_metadata(path)?;
    require(m.dev() == device, "cross-device entry refused")?;
    if m.is_dir() {
        let dir = checked_open(path, &m, true)?;
        for name in names(&dir)? {
            collect_snapshot(&fd_path(&dir).join(&name), &rel.join(&name), device, out)?;
        }
        require(identity(&dir.metadata()?) == identity(&m), "directory changed during traversal")?;
    } else if !m.is_file() && !m.file_type().is_symlink() { return Err(fail("special file refused")); }
    check(path, &m)?;
    out.insert(rel.to_path_buf(), identity(&m));
    Ok(())
}
pub(crate) fn snapshot(path: &Path) -> io::Result<Snapshot> {
    let mut result = BTreeMap::new();
    collect_snapshot(path, Path::new("."), fs::symlink_metadata(path)?.dev(), &mut result)?;
    Ok(result)
}
pub(crate) fn copy_tree(src: &Path, payload: &Path, expected: &Manifest, reviewed: &Snapshot,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>) -> io::Result<Snapshot> {
    let original = fs::symlink_metadata(src)?;
    require(&snapshot(src)? == reviewed, "source changed before copy")?;
    let mut buffer = vec![0; 1024 * 1024];
    copy_entry(src, payload, original.dev(), checkpoint, &mut BTreeMap::new(), &mut buffer)?;
    // Independently read and sync every staged file and directory before publish.
    checkpoint("copy-verify-stage")?;
    let (actual, staged) = manifest_buffered(payload, true,checkpoint,&mut buffer)?;
    require(&actual == expected, "staged tree differs from reviewed source")?;
    checkpoint("copy-verify-source")?;
    let (source_after, ids_after) = manifest_buffered(src, false,checkpoint,&mut buffer)?;
    require(&source_after == expected && &ids_after == reviewed, "source tree changed")?;
    Ok(staged)
}
