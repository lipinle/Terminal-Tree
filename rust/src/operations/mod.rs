//! Experimental native operation core for an explicitly scoped workspace.
//! The same transfer function is exercised in the crash-test guest.
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
mod policy;
mod trash;
use manifest::{
    copy_tree, fd_path, manifest, manifest_with, snapshot, Identity, Manifest, Snapshot,
};
pub use trash::{PurgeOutcome, PurgeReview, RestoreOutcome, RestoreReview, Trash};

fn fail(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(ok: bool, message: &str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(fail(message))
    }
}
fn identity(m: &Metadata) -> Identity {
    (
        m.dev(),
        m.ino(),
        m.mode(),
        m.size(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
fn attributes(f: &File) -> io::Result<BTreeMap<OsString, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for key in f.list_xattr()? {
        let value = f
            .get_xattr(&key)?
            .ok_or_else(|| fail("xattr disappeared"))?;
        result.insert(key, value);
    }
    Ok(result)
}
fn digest(
    mut f: &File,
    buf: &mut [u8],
    checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
) -> io::Result<[u8; 32]> {
    let before = identity(&f.metadata()?);
    let mut h = Sha256::new();
    loop {
        let n = f.read(buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
        checkpoint("hash-chunk")?;
    }
    require(
        identity(&f.metadata()?) == before,
        "file changed during hash",
    )?;
    Ok(h.finalize().into())
}
fn open_dir(path: &Path) -> io::Result<File> {
    Ok(rx::open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into())
}
fn dir_id(f: &File) -> io::Result<(u64, u64)> {
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
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    Ok(format!(
        "{n:032x}-{:08x}-{:016x}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn one_name(name: &OsStr) -> io::Result<()> {
    let mut c = Path::new(name).components();
    require(
        matches!(c.next(), Some(Component::Normal(_)))
            && c.next().is_none()
            && !name.as_bytes().contains(&b'/')
            && !name.as_bytes().contains(&0),
        "single filename required",
    )
}
fn no_conflict(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        )),
    }
}
fn same_parent(path: &Path, f: &File) -> io::Result<()> {
    require(
        dir_id(&open_dir(path)?)? == dir_id(f)?,
        "parent directory changed",
    )
}
// Decode Linux mountinfo octal escapes, including non-UTF-8 paths.
fn reject_mounts(path: &Path) -> io::Result<()> {
    use std::os::unix::ffi::OsStringExt;
    for line in fs::read("/proc/self/mountinfo")?.split(|b| *b == b'\n') {
        let Some(raw) = line.split(|b| *b == b' ').nth(4) else {
            continue;
        };
        let mut decoded = Vec::new();
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'\\'
                && i + 3 < raw.len()
                && raw[i + 1..i + 4].iter().all(|b| (b'0'..=b'7').contains(b))
            {
                decoded
                    .push((raw[i + 1] - b'0') * 64 + (raw[i + 2] - b'0') * 8 + (raw[i + 3] - b'0'));
                i += 4;
            } else {
                decoded.push(raw[i]);
                i += 1;
            }
        }
        let mount = PathBuf::from(OsString::from_vec(decoded));
        require(
            !mount.starts_with(path),
            "mount point or nested mount refused",
        )?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Review {
    path: PathBuf,
    identity: Identity,
    parent: (u64, u64),
    snapshot: Snapshot,
    manifest: Manifest,
}
impl Review {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
#[derive(Debug, Clone)]
pub struct Destination {
    path: PathBuf,
    parent: (u64, u64),
}
impl Destination {
    pub fn path(&self) -> &Path {
        &self.path
    }
}
#[derive(Debug)]
pub struct Undo {
    review: Review,
    original: PathBuf,
}
impl Undo {
    pub fn source(&self) -> &Path {
        &self.review.path
    }
    pub fn destination(&self) -> &Path {
        &self.original
    }
}
#[derive(Debug, Clone, Copy)]
pub enum Transfer {
    Copy,
    Move,
}

/// Scope writes to an explicitly selected, canonical workspace. This is an
/// experimental interface, not yet a substitute for the Python system guards.
pub struct Engine {
    root: PathBuf,
    root_identity: (u64, u64),
    journal: File,
    journal_path: PathBuf,
    personal: Option<policy::PersonalPolicy>,
}
impl Engine {
    pub fn new(root: &Path, journal: &Path) -> io::Result<Self> {
        let root = root.canonicalize()?;
        require(root != Path::new("/"), "root workspace refused")?;
        for sys in [
            "/bin", "/boot", "/dev", "/etc", "/lib", "/lib64", "/proc", "/root", "/run", "/sbin",
            "/sys", "/usr", "/var",
        ] {
            require(!root.starts_with(sys), "system workspace refused")?;
        }
        require(
            fs::symlink_metadata(journal)?.is_dir(),
            "journal symlink or non-directory refused",
        )?;
        let j = journal.canonicalize()?;
        require(
            !j.starts_with(&root) && !root.starts_with(&j),
            "journal must be separate from workspace",
        )?;
        let journal = open_dir(&j)?;
        let meta = journal.metadata()?;
        require(
            meta.uid() == rustix::process::getuid().as_raw()
                && meta.permissions().mode() & 0o077 == 0,
            "private owned journal required",
        )?;
        Ok(Self {
            root_identity: dir_id(&open_dir(&root)?)?,
            root,
            journal,
            journal_path: j,
            personal: None,
        })
    }
    /// Explicit personal-file preview; the home and listed external mounts
    /// are the only permitted roots. Home Trash uses a separate narrow adapter.
    pub fn new_personal(
        home: &Path,
        journal: &Path,
        extra: &[PathBuf],
        protected: &[PathBuf],
    ) -> io::Result<Self> {
        let j = policy::normal_absolute(journal)?;
        let file = policy::open_absolute_dir(&j)?;
        let meta = file.metadata()?;
        require(
            meta.uid() == rustix::process::getuid().as_raw() && meta.mode() & 0o077 == 0,
            "private owned journal required",
        )?;
        let mut reserved = protected.to_vec();
        reserved.push(j.clone());
        reserved.push(std::env::current_exe()?);
        let personal = policy::PersonalPolicy::new(home, extra, &reserved)?;
        let root = policy::normal_absolute(home)?;
        let root_identity = dir_id(&policy::open_absolute_dir(&root)?)?;
        Ok(Self {
            root,
            root_identity,
            journal: file,
            journal_path: j,
            personal: Some(personal),
        })
    }
    pub fn is_personal(&self) -> bool {
        self.personal.is_some()
    }
    pub fn personal_data_home(&self) -> Option<&Path> {
        self.personal.as_ref().map(|p| p.data_home())
    }
    pub fn validate_personal_path(&self, path: &Path) -> io::Result<()> {
        require(self.is_personal(), "personal mode required")?;
        self.path(path).map(|_| ())
    }
    fn check_journal(&self) -> io::Result<()> {
        let actual = policy::open_absolute_dir(&self.journal_path)?;
        let m = actual.metadata()?;
        require(
            dir_id(&actual)? == dir_id(&self.journal)?
                && m.uid() == rustix::process::getuid().as_raw()
                && m.mode() & 0o077 == 0,
            "journal replaced or no longer private",
        )
    }
    fn parent_for(&self, path: &Path) -> io::Result<File> {
        if let Some(p) = &self.personal {
            p.parent(path)
        } else {
            policy::open_absolute_dir(path.parent().ok_or_else(|| fail("missing parent"))?)
        }
    }
    fn check_parent(&self, path: &Path, pinned: &File) -> io::Result<()> {
        require(
            dir_id(&self.parent_for(path)?)? == dir_id(pinned)?,
            "parent changed or redirected",
        )
    }
    fn path(&self, path: &Path) -> io::Result<PathBuf> {
        if let Some(p) = &self.personal {
            return p.path(path);
        }

        require(
            dir_id(&open_dir(&self.root)?)? == self.root_identity,
            "workspace replaced",
        )?;
        let name = path.file_name().ok_or_else(|| fail("missing filename"))?;
        one_name(name)?;
        let parent = path
            .parent()
            .ok_or_else(|| fail("missing parent"))?
            .canonicalize()?;
        require(parent.starts_with(&self.root), "outside workspace")?;
        Ok(parent.join(name))
    }
    pub fn review(&self, path: &Path) -> io::Result<Review> {
        self.review_with(path, &mut |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn review_with(
        &self,
        path: &Path,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<Review> {
        let path = self.path(path)?;
        reject_mounts(&path)?;
        let parent = self.parent_for(&path)?;
        let m = fs::symlink_metadata(fd_path(&parent).join(path.file_name().unwrap()))?;
        let (manifest, snapshot) = manifest_with(
            &fd_path(&parent).join(path.file_name().unwrap()),
            false,
            checkpoint,
        )?;
        let reviewed = Review {
            path,
            identity: identity(&m),
            parent: dir_id(&parent)?,
            snapshot,
            manifest,
        };
        self.check_identity(&reviewed)?;
        Ok(reviewed)
    }
    fn check_identity(&self, review: &Review) -> io::Result<File> {
        let p = self.path(&review.path)?;
        require(p == review.path, "source path changed")?;
        reject_mounts(&p)?;
        let parent = self.parent_for(&p)?;
        require(dir_id(&parent)? == review.parent, "source parent changed")?;
        let entry = fd_path(&parent).join(p.file_name().unwrap());
        require(
            identity(&fs::symlink_metadata(&entry)?) == review.identity,
            "source changed",
        )?;
        require(snapshot(&entry)? == review.snapshot, "source tree changed")?;
        Ok(parent)
    }
    fn check(&self, review: &Review) -> io::Result<File> {
        let parent = self.check_identity(review)?;
        let (content, ids) = manifest(
            &fd_path(&parent).join(review.path.file_name().unwrap()),
            false,
        )?;
        require(
            content == review.manifest && ids == review.snapshot,
            "reviewed content changed",
        )?;
        Ok(parent)
    }
    fn log(
        &self,
        id: &str,
        operation: &str,
        src: Option<&Path>,
        dst: Option<&Path>,
        stage: Option<&Path>,
    ) -> io::Result<()> {
        self.check_journal()?;
        let hex = |p: &Path| {
            p.as_os_str()
                .as_bytes()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        };
        let record = serde_json::json!({
            "id": id, "operation": operation,
            "source": src.and_then(Path::to_str), "destination": dst.and_then(Path::to_str),
            "stage": stage.and_then(Path::to_str),
            "source_bytes_hex": src.map(hex), "destination_bytes_hex": dst.map(hex),
            "stage_bytes_hex": stage.map(hex), "started": id,
        });
        let mut file: File = rx::openat(
            &self.journal,
            format!("{id}.pending.json"),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::from_bits_truncate(0o600),
        )?
        .into();
        serde_json::to_writer(&mut file, &record).map_err(io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        self.journal.sync_all()
    }
    fn complete(&self, id: &str) -> io::Result<()> {
        self.check_journal()?;
        rx::renameat_with(
            &self.journal,
            format!("{id}.pending.json"),
            &self.journal,
            format!("{id}.done.json"),
            RenameFlags::NOREPLACE,
        )?;
        self.journal.sync_all()
    }
    pub fn destination(&self, path: &Path) -> io::Result<Destination> {
        let path = self.path(path)?;
        let parent = self.parent_for(&path)?;
        no_conflict(&fd_path(&parent).join(path.file_name().unwrap()))?;
        Ok(Destination {
            path,
            parent: dir_id(&parent)?,
        })
    }
    /// Read-only batch preflight. A successful preflight does not reserve
    /// names; each transaction still rechecks the reviewed identities.
    pub fn validate_transfer(
        &self,
        review: &Review,
        destination: &Destination,
        kind: Transfer,
    ) -> io::Result<()> {
        self.check_journal()?;
        let src = self.check(review)?;
        let dst = self.path(&destination.path)?;
        require(
            dst == destination.path && !dst.starts_with(&review.path),
            "destination inside source or changed",
        )?;
        let parent = self.parent_for(&dst)?;
        require(
            dir_id(&parent)? == destination.parent,
            "reviewed destination replaced",
        )?;
        no_conflict(&fd_path(&parent).join(dst.file_name().unwrap()))?;
        if matches!(kind, Transfer::Move) {
            require(
                fs::symlink_metadata(fd_path(&src).join(review.path.file_name().unwrap()))?.dev()
                    == parent.metadata()?.dev(),
                "cross-device move refused",
            )?;
        }
        Ok(())
    }
    pub fn validate_batch(
        &self,
        items: &[(Review, Destination)],
        kind: Transfer,
    ) -> io::Result<()> {
        require(!items.is_empty(), "empty batch")?;
        // Index every path once; checking ancestors is proportional to
        // path depth rather than comparing all pairs in a large batch.
        let mut paths = BTreeSet::new();
        for (src, dst) in items {
            require(
                paths.insert(src.path()) && paths.insert(dst.path()),
                "duplicate batch path refused",
            )?;
        }
        for path in &paths {
            require(
                !path
                    .ancestors()
                    .skip(1)
                    .any(|parent| paths.contains(parent)),
                "overlapping batch paths refused",
            )?;
        }
        // Still independently validate every source manifest and destination.
        for (src, dst) in items {
            self.validate_transfer(src, dst, kind)?;
        }
        Ok(())
    }
    pub fn transfer_reviewed(
        &self,
        review: &Review,
        destination: &Destination,
        kind: Transfer,
    ) -> io::Result<Option<Undo>> {
        self.transfer_reviewed_with(review, destination, kind, &mut |_| Ok(()))
    }
    pub fn transfer(
        &self,
        review: &Review,
        destination: &Path,
        kind: Transfer,
    ) -> io::Result<Option<Undo>> {
        self.transfer_with(review, destination, kind, &mut |_| Ok(()))
    }
    #[doc(hidden)]
    pub fn transfer_with(
        &self,
        review: &Review,
        destination: &Path,
        kind: Transfer,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<Option<Undo>> {
        let reviewed = self.destination(destination)?;
        self.transfer_reviewed_with(review, &reviewed, kind, checkpoint)
    }
    #[doc(hidden)]
    pub fn transfer_reviewed_with(
        &self,
        review: &Review,
        destination: &Destination,
        kind: Transfer,
        checkpoint: &mut dyn FnMut(&str) -> io::Result<()>,
    ) -> io::Result<Option<Undo>> {
        let dst = self.path(&destination.path)?;
        require(dst == destination.path, "destination path changed")?;
        let src_parent = self.check_identity(review)?;
        let dest_parent = self.parent_for(&dst)?;
        require(
            dir_id(&dest_parent)? == destination.parent,
            "reviewed destination replaced",
        )?;
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
            require(
                fs::symlink_metadata(&src)?.dev() == dest_parent.metadata()?.dev(),
                "cross-device move refused",
            )?;
        }
        checkpoint("transfer-prepare")?;
        self.log(
            &id,
            if copy { "copy" } else { "move" },
            Some(&review.path),
            Some(&dst),
            if copy { Some(&stage_path) } else { None },
        )?;
        if copy {
            let stage = create_dir_at(&dest_parent, OsStr::new(&stage_name))?;
            let payload = fd_path(&stage).join("payload");
            let staged = copy_tree(
                &src,
                &payload,
                &review.manifest,
                &review.snapshot,
                checkpoint,
            )?;
            self.check_identity(review)?;
            self.check_parent(&dst, &dest_parent)?;
            reject_mounts(&stage_path)?;
            checkpoint("copy-before-publish")?;
            self.check_journal()?;
            reject_mounts(&stage_path)?;
            require(
                snapshot(&payload)? == staged,
                "verified payload changed before publish",
            )?;
            self.check_identity(review)?;
            self.check_parent(&review.path, &src_parent)?;
            self.check_parent(&dst, &dest_parent)?;
            stage.sync_all()?;
            dest_parent.sync_all()?;
            checkpoint("transfer-commit")?;
            rx::renameat_with(
                &stage,
                "payload",
                &dest_parent,
                dest_name,
                RenameFlags::NOREPLACE,
            )?;
            checkpoint("copy-before-sync")?;
            dest_parent.sync_all()?;
            stage.sync_all()?;
            checkpoint("copy-after-sync")?;
            require(
                dir_id(&open_dir(&fd_path(&dest_parent).join(&stage_name))?)? == dir_id(&stage)?,
                "stage replaced",
            )?;
            rx::unlinkat(&dest_parent, &stage_name, AtFlags::REMOVEDIR)?;
            dest_parent.sync_all()?;
            self.complete(&id)?;
            checkpoint("copy-done")?;
            Ok(None)
        } else {
            checkpoint("move-verify-source")?;
            let (before, ids) = manifest_with(&src, true, checkpoint)?;
            require(
                before == review.manifest && ids == review.snapshot,
                "source changed while syncing",
            )?;
            self.check_identity(review)?;
            self.check_parent(&review.path, &src_parent)?;
            self.check_parent(&dst, &dest_parent)?;
            src_parent.sync_all()?;
            let same_dir = dir_id(&src_parent)? == dir_id(&dest_parent)?;
            if !same_dir {
                dest_parent.sync_all()?;
            }
            self.check_journal()?;
            checkpoint("transfer-commit")?;
            rx::renameat_with(
                &src_parent,
                src_name,
                &dest_parent,
                dest_name,
                RenameFlags::NOREPLACE,
            )?;
            checkpoint("move-before-sync")?;
            dest_parent.sync_all()?;
            if !same_dir {
                src_parent.sync_all()?;
            }
            checkpoint("move-after-sync")?;
            self.complete(&id)?;
            checkpoint("move-done")?;
            // A failed post-move review must not cause a misleading retry.
            // If another writer has already changed the result, do not grant Undo.
            checkpoint("move-verify-result")?;
            let after = self.review(&dst).and_then(|after| {
                require(
                    after.manifest == before,
                    "moved result changed before undo review",
                )?;
                require(
                    after.snapshot.len() == review.snapshot.len()
                        && review.snapshot.iter().all(|(p, id)| {
                            if p == Path::new(".") {
                                after.snapshot.get(p).is_some_and(|a| {
                                    (a.0, a.1, a.2, a.3, a.4, a.5)
                                        == (id.0, id.1, id.2, id.3, id.4, id.5)
                                })
                            } else {
                                after.snapshot.get(p) == Some(id)
                            }
                        }),
                    "moved tree identity changed",
                )?;
                require(
                    after.identity.0 == review.identity.0 && after.identity.1 == review.identity.1,
                    "moved inode replaced",
                )?;
                Ok(after)
            });
            Ok(after.ok().map(|after| Undo {
                review: after,
                original: review.path.clone(),
            }))
        }
    }
    pub fn undo(&self, undo: &Undo) -> io::Result<()> {
        let destination = self.destination(&undo.original)?;
        self.undo_reviewed(undo, &destination)
    }
    pub fn undo_reviewed(&self, undo: &Undo, destination: &Destination) -> io::Result<()> {
        require(
            destination.path == undo.original,
            "undo destination differs",
        )?;
        self.transfer_reviewed(&undo.review, destination, Transfer::Move)
            .map(|_| ())
    }
    pub fn create(&self, destination: &Path, directory: bool) -> io::Result<()> {
        let reviewed = self.destination(destination)?;
        self.create_reviewed(&reviewed, directory)
    }
    pub fn create_reviewed(&self, destination: &Destination, directory: bool) -> io::Result<()> {
        let dst = self.path(&destination.path)?;
        require(dst == destination.path, "destination changed")?;
        let parent = self.parent_for(&dst)?;
        require(
            dir_id(&parent)? == destination.parent,
            "reviewed destination replaced",
        )?;
        let name = dst.file_name().unwrap();
        no_conflict(&fd_path(&parent).join(name))?;
        let id = unique()?;
        self.log(
            &id,
            if directory { "mkdir" } else { "create-file" },
            None,
            Some(&dst),
            None,
        )?;
        self.check_parent(&dst, &parent)?;
        self.check_journal()?;
        if directory {
            create_dir_at(&parent, name)?;
        } else {
            let f: File = rx::openat(
                &parent,
                name,
                OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
                Mode::from_bits_truncate(0o666),
            )?
            .into();
            f.sync_all()?;
            parent.sync_all()?;
        }
        self.complete(&id)
    }
    /// Read-only enumeration; malformed records are reported as errors.
    /// Bounded, read-only diagnostic view. A bad record must not conceal
    /// other pending records. Never interpret record paths as action inputs.
    pub fn recovery_report(
        &self,
    ) -> io::Result<(Vec<(OsString, io::Result<serde_json::Value>)>, bool)> {
        self.check_journal()?;
        let mut out = Vec::new();
        let mut truncated = false;
        for entry in fs::read_dir(fd_path(&self.journal))? {
            let name = entry?.file_name();
            if !name.as_bytes().ends_with(b".pending.json") {
                continue;
            }
            if out.len() >= 128 {
                truncated = true;
                break;
            }
            let result = (|| -> io::Result<serde_json::Value> {
                let f: File = rx::openat(
                    &self.journal,
                    &name,
                    OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                    Mode::empty(),
                )?
                .into();
                let m = f.metadata()?;
                require(
                    m.is_file()
                        && m.uid() == rustix::process::getuid().as_raw()
                        && m.mode() & 0o077 == 0
                        && m.nlink() == 1
                        && m.len() <= 65536,
                    "unsafe or oversized recovery record",
                )?;
                let mut bytes = Vec::new();
                (&f).take(65537).read_to_end(&mut bytes)?;
                require(
                    bytes.len() <= 65536 && identity(&f.metadata()?) == identity(&m),
                    "record changed during read",
                )?;
                let named = fs::symlink_metadata(fd_path(&self.journal).join(&name))?;
                require(
                    identity(&named) == identity(&m),
                    "record replaced during read",
                )?;
                let value: serde_json::Value =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                require(value.is_object(), "invalid journal object")?;
                Ok(value)
            })();
            out.push((name, result));
        }
        self.check_journal()?;
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok((out, truncated))
    }
    pub fn pending(&self) -> io::Result<Vec<serde_json::Value>> {
        self.check_journal()?;
        let mut out = Vec::new();
        for e in fs::read_dir(fd_path(&self.journal))? {
            let name = e?.file_name();
            if !name.as_bytes().ends_with(b".pending.json") {
                continue;
            }
            let f: File = rx::openat(
                &self.journal,
                &name,
                OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
                Mode::empty(),
            )?
            .into();
            let m = f.metadata()?;
            require(
                m.is_file()
                    && m.len() <= 1024 * 1024
                    && m.uid() == rustix::process::getuid().as_raw(),
                "invalid journal",
            )?;
            let value: serde_json::Value =
                serde_json::from_reader(f.take(1024 * 1024 + 1)).map_err(io::Error::other)?;
            require(value.is_object(), "invalid journal object")?;
            out.push(value);
        }
        Ok(out)
    }
}
