//! Experimental native operation core. Not wired to the live navigator yet.
//! The same transfer function is exercised in the crash-test guest.
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use xattr::FileExt;

mod manifest;
use manifest::{fd_path, manifest, copy_tree, Snapshot, Identity};

fn fail(message: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidData, message) }
fn require(ok: bool, message: &str) -> io::Result<()> { if ok { Ok(()) } else { Err(fail(message)) } }
fn identity(m: &Metadata) -> Identity {
    (m.dev(), m.ino(), m.mode(), m.size(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
}
fn attributes(f: &File) -> io::Result<BTreeMap<OsString, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for key in f.list_xattr()? {
        let value = f.get_xattr(&key)?.ok_or_else(|| fail("xattr disappeared"))?;
        result.insert(key, value);
    }
    Ok(result)
}
fn digest(mut f: &File) -> io::Result<[u8; 32]> {
    let before = identity(&f.metadata()?);
    let mut h = Sha256::new();
    let mut buf = vec![0; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        h.update(&buf[..n]);
    }
    require(identity(&f.metadata()?) == before, "file changed during hash")?;
    Ok(h.finalize().into())
}
fn open_dir(path: &Path) -> io::Result<File> {
    Ok(rx::open(path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into())
}
fn dir_id(f: &File) -> io::Result<(u64,u64)> {
    let m = f.metadata()?;
    Ok((m.dev(), m.ino()))
}
fn create_dir_at(parent: &File, name: &OsStr) -> io::Result<File> {
    rx::mkdirat(parent, name, Mode::from_bits_truncate(0o700))?;
    let dir = open_dir(&fd_path(parent).join(name))?;
    dir.sync_all()?;
    parent.sync_all()?;
    Ok(dir)
}
fn unique() -> io::Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let n = SystemTime::now().duration_since(UNIX_EPOCH).map_err(io::Error::other)?.as_nanos();
    Ok(format!("{n:032x}-{:08x}-{:016x}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)))
}
fn one_name(name: &OsStr) -> io::Result<()> {
    let mut c = Path::new(name).components();
    require(matches!(c.next(), Some(Component::Normal(_))) && c.next().is_none()
        && !name.as_bytes().contains(&b'/') && !name.as_bytes().contains(&0), "single filename required")
}
fn no_conflict(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(io::Error::new(io::ErrorKind::AlreadyExists, "destination exists")),
    }
}
fn same_parent(path: &Path, f: &File) -> io::Result<()> {
    require(dir_id(&open_dir(path)?)? == dir_id(f)?, "parent directory changed")
}
// Decode Linux mountinfo octal escapes, including non-UTF-8 paths.
fn reject_mounts(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    for line in fs::read("/proc/self/mountinfo")?.split(|b| *b == b'\n') {
        let Some(raw) = line.split(|b| *b == b' ').nth(4) else { continue };
        let mut decoded = Vec::new(); let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'\\' && i + 3 < raw.len() && raw[i+1..i+4].iter().all(|b| (b'0'..=b'7').contains(b)) {
                decoded.push((raw[i+1]-b'0')*64+(raw[i+2]-b'0')*8+(raw[i+3]-b'0'));
                i += 4;
            } else { decoded.push(raw[i]); i += 1; }
        }
        let mount = PathBuf::from(OsString::from_vec(decoded));
        require(!mount.starts_with(path), "mount point or nested mount refused")?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Review {
    path: PathBuf,
    identity: Identity,
    parent: (u64,u64),
    snapshot: Snapshot,
}
#[derive(Debug)]
pub struct Undo {
    review: Review,
    original: PathBuf,
}
#[derive(Debug, Clone, Copy)]
pub enum Transfer { Copy, Move }

/// Scope writes to an explicitly selected, canonical workspace. This is an
/// experimental interface, not yet a substitute for the Python system guards.
pub struct Engine {
    root: PathBuf,
    root_identity: (u64,u64),
    journal: File,
}
impl Engine {
    pub fn new(root: &Path, journal: &Path) -> io::Result<Self> {
        let root = root.canonicalize()?;
        require(root != Path::new("/"), "root workspace refused")?;
        for sys in ["/bin","/boot","/dev","/etc","/lib","/lib64","/proc","/root","/run","/sbin","/sys","/usr","/var"] {
            require(!root.starts_with(sys), "system workspace refused")?;
        }
        let j = journal.canonicalize()?;
        require(!j.starts_with(&root) && !root.starts_with(&j), "journal must be separate from workspace")?;
        let journal = open_dir(&j)?;
        let meta = journal.metadata()?;
        require(meta.uid() == rustix::process::getuid().as_raw() && meta.permissions().mode() & 0o077 == 0,
            "private owned journal required")?;
        Ok(Self { root_identity: dir_id(&open_dir(&root)?)?, root, journal })
    }
    fn path(&self, path: &Path) -> io::Result<PathBuf> {
        require(dir_id(&open_dir(&self.root)?)? == self.root_identity, "workspace replaced")?;
        let name = path.file_name().ok_or_else(|| fail("missing filename"))?;
        one_name(name)?;
        let parent = path.parent().ok_or_else(|| fail("missing parent"))?.canonicalize()?;
        require(parent.starts_with(&self.root), "outside workspace")?;
        Ok(parent.join(name))
    }
    pub fn review(&self, path: &Path) -> io::Result<Review> {
        let path = self.path(path)?;
        reject_mounts(&path)?;
        let parent = open_dir(path.parent().unwrap())?;
        let m = fs::symlink_metadata(fd_path(&parent).join(path.file_name().unwrap()))?;
        let (_, snapshot) = manifest(&fd_path(&parent).join(path.file_name().unwrap()), false)?;
        let reviewed = Review { path, identity: identity(&m), parent: dir_id(&parent)?, snapshot };
        self.check(&reviewed)?;
        Ok(reviewed)
    }
    fn check(&self, review: &Review) -> io::Result<File> {
        let p = self.path(&review.path)?;
        require(p == review.path, "source path changed")?;
        reject_mounts(&p)?;
        let parent = open_dir(p.parent().unwrap())?;
        require(dir_id(&parent)? == review.parent, "source parent changed")?;
        let entry = fd_path(&parent).join(p.file_name().unwrap());
        require(identity(&fs::symlink_metadata(&entry)?) == review.identity, "source changed")?;
        require(manifest(&entry, false)?.1 == review.snapshot, "source tree changed")?;
        Ok(parent)
    }
    fn log(&self, id: &str, operation: &str, src: Option<&Path>, dst: &Path, stage: Option<&Path>) -> io::Result<()> {
        let hex = |p: &Path| p.as_os_str().as_bytes().iter().map(|b| format!("{b:02x}")).collect::<String>();
        let record = serde_json::json!({
            "id": id, "operation": operation,
            "source": src.and_then(Path::to_str), "destination": dst.to_str(),
            "stage": stage.and_then(Path::to_str),
            "source_bytes_hex": src.map(hex), "destination_bytes_hex": hex(dst),
            "stage_bytes_hex": stage.map(hex), "started": id,
        });
        let mut file: File = rx::openat(&self.journal, format!("{id}.pending.json"),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600))?.into();
        serde_json::to_writer(&mut file, &record).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        self.journal.sync_all()
    }
    fn complete(&self, id: &str) -> io::Result<()> {
        rx::renameat_with(&self.journal, format!("{id}.pending.json"),
            &self.journal, format!("{id}.done.json"), RenameFlags::NOREPLACE)?;
        self.journal.sync_all()
    }
    pub fn transfer(&self, review: &Review, destination: &Path, kind: Transfer) -> io::Result<Option<Undo>> {
        self.transfer_with(review, destination, kind, &mut |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn transfer_with(&self, review: &Review, destination: &Path, kind: Transfer,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>) -> io::Result<Option<Undo>> {
        let dst = self.path(destination)?;
        let src_parent = self.check(review)?;
        let dest_parent = open_dir(dst.parent().unwrap())?;
        let dest_name = dst.file_name().unwrap();
        let dest_entry = fd_path(&dest_parent).join(dest_name);
        no_conflict(&dest_entry)?;
        require(!dst.starts_with(&review.path), "destination inside source")?;
        let src_name = review.path.file_name().unwrap();
        let src = fd_path(&src_parent).join(src_name);
        let id = unique()?;
        let stage_name = format!(".terminal-tree-copy-{id}");
        let stage_path = dst.parent().unwrap().join(&stage_name);
        let copy = matches!(kind, Transfer::Copy);
        if !copy {
            require(fs::symlink_metadata(&src)?.dev() == dest_parent.metadata()?.dev(), "cross-device move refused")?;
        }
        self.log(&id, if copy {"copy"} else {"move"}, Some(&review.path), &dst,
            if copy {Some(&stage_path)} else {None})?;
        if copy {
            let stage = create_dir_at(&dest_parent, OsStr::new(&stage_name))?;
            let payload = fd_path(&stage).join("payload");
            copy_tree(&src, &payload, checkpoint)?;
            self.check(review)?;
            same_parent(dst.parent().unwrap(), &dest_parent)?;
            reject_mounts(&stage_path)?;
            checkpoint("copy-before-publish")?;
            stage.sync_all()?; dest_parent.sync_all()?;
            rx::renameat_with(&stage, "payload", &dest_parent, dest_name, RenameFlags::NOREPLACE)?;
            checkpoint("copy-before-sync")?;
            dest_parent.sync_all()?; stage.sync_all()?;
            checkpoint("copy-after-sync")?;
            require(dir_id(&open_dir(&fd_path(&dest_parent).join(&stage_name))?)? == dir_id(&stage)?, "stage replaced")?;
            rx::unlinkat(&dest_parent, &stage_name, AtFlags::REMOVEDIR)?;
            dest_parent.sync_all()?;
            self.complete(&id)?;
            checkpoint("copy-done")?;
            Ok(None)
        } else {
            manifest(&src, true)?;
            self.check(review)?;
            same_parent(review.path.parent().unwrap(), &src_parent)?;
            same_parent(dst.parent().unwrap(), &dest_parent)?;
            src_parent.sync_all()?; dest_parent.sync_all()?;
            rx::renameat_with(&src_parent, src_name, &dest_parent, dest_name, RenameFlags::NOREPLACE)?;
            checkpoint("move-before-sync")?;
            dest_parent.sync_all()?; src_parent.sync_all()?;
            checkpoint("move-after-sync")?;
            self.complete(&id)?;
            checkpoint("move-done")?;
            // A failed post-move review must not cause a misleading retry.
            Ok(self.review(&dst).ok().map(|after| Undo { review: after, original: review.path.clone() }))
        }
    }
    pub fn undo(&self, undo: &Undo) -> io::Result<()> {
        self.transfer(&undo.review, &undo.original, Transfer::Move).map(|_| ())
    }
    pub fn create(&self, destination: &Path, directory: bool) -> io::Result<()> {
        let dst = self.path(destination)?;
        let parent = open_dir(dst.parent().unwrap())?;
        let name = dst.file_name().unwrap();
        no_conflict(&fd_path(&parent).join(name))?;
        let id = unique()?;
        self.log(&id, if directory {"mkdir"} else {"create-file"}, None, &dst, None)?;
        same_parent(dst.parent().unwrap(), &parent)?;
        if directory { create_dir_at(&parent, name)?; }
        else {
            let f: File = rx::openat(&parent, name, OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL
                | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_bits_truncate(0o666))?.into();
            f.sync_all()?; parent.sync_all()?;
        }
        self.complete(&id)
    }
    /// Read-only enumeration; malformed records are reported as errors.
    pub fn pending(&self) -> io::Result<Vec<serde_json::Value>> {
        let mut out = Vec::new();
        for e in fs::read_dir(fd_path(&self.journal))? {
            let name = e?.file_name();
            if !name.as_bytes().ends_with(b".pending.json") { continue; }
            let f: File = rx::openat(&self.journal, &name, OFlags::RDONLY | OFlags::NOFOLLOW
                | OFlags::NONBLOCK | OFlags::CLOEXEC, Mode::empty())?.into();
            let m = f.metadata()?;
            require(m.is_file() && m.len() <= 1024*1024 && m.uid() == rustix::process::getuid().as_raw(), "invalid journal")?;
            let value: serde_json::Value = serde_json::from_reader(f.take(1024*1024+1)).map_err(io::Error::other)?;
            require(value.is_object(), "invalid journal object")?;
            out.push(value);
        }
        Ok(out)
    }
}
