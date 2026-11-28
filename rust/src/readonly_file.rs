//! Shared native read-only file acquisition and observed identity checks.
//! Not a write authorization, mount policy, atomic snapshot or I/O deadline.
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io;
use std::os::unix::{ffi::OsStrExt, fs::MetadataExt};
use std::path::{Component, Path};

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn parent(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid("absolute file path required"));
    }
    // Path::file_name and parent normalize trailing slash/dot components.
    // Preserve the kernel's directory-only meaning rather than reading a
    // regular file when the caller requested file/ or file/.
    let raw = path.as_os_str().as_bytes();
    if raw.ends_with(b"/") || raw.ends_with(b"/.") {
        return Err(invalid("file path must end in a filename"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("file parent required"))?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for part in parent.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(name) => dir = rx::openat(&dir, name, flags, Mode::empty())?.into(),
            _ => return Err(invalid("invalid file path component")),
        }
    }
    Ok(dir)
}

fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    b.is_file()
        && a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn open(dir: &File, path: &Path) -> io::Result<File> {
    let name = path
        .file_name()
        .ok_or_else(|| invalid("filename required"))?;
    // Refuse observed nonregular files before opening, including devices.
    let stat = rx::statat(dir, name, rx::AtFlags::SYMLINK_NOFOLLOW)?;
    if rx::FileType::from_raw_mode(stat.st_mode) != rx::FileType::RegularFile {
        return Err(invalid("read requires a regular non-link file"));
    }
    let file: File = rx::openat(
        dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.dev() != stat.st_dev || metadata.ino() != stat.st_ino {
        return Err(invalid("file changed before opening"));
    }
    Ok(file)
}

/// Hold the original parent and file descriptors across a complete read.
/// Call verify before returning any derived result to the user.
pub(crate) struct ReadSession<'a> {
    path: &'a Path,
    dir: File,
    file: File,
    before: Metadata,
}

impl<'a> ReadSession<'a> {
    pub(crate) fn open(path: &'a Path) -> io::Result<Self> {
        let dir = parent(path)?;
        let file = open(&dir, path)?;
        let before = file.metadata()?;
        Ok(Self {
            path,
            dir,
            file,
            before,
        })
    }

    pub(crate) fn file(&self) -> &File {
        &self.file
    }

    pub(crate) fn len(&self) -> u64 {
        self.before.len()
    }

    pub(crate) fn verify(self) -> io::Result<()> {
        let current_dir = parent(self.path)?;
        let a = self.dir.metadata()?;
        let b = current_dir.metadata()?;
        if (a.dev(), a.ino()) != (b.dev(), b.ino())
            || !unchanged(&self.before, &self.file.metadata()?)
            || !unchanged(&self.before, &open(&current_dir, self.path)?.metadata()?)
        {
            return Err(invalid("file changed during read"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Read;
    use std::os::unix::fs::symlink;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "tt-read-session-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn reads_regular_file_and_verifies_without_mutation() {
        let f = Fixture::new();
        let path = f.0.join("file");
        fs::write(&path, b"keep").unwrap();
        let session = ReadSession::open(&path).unwrap();
        assert_eq!(session.len(), 4);
        let mut bytes = Vec::new();
        session.file().read_to_end(&mut bytes).unwrap();
        session.verify().unwrap();
        assert_eq!(bytes, b"keep");
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }

    #[test]
    fn refuses_nonregular_relative_and_symlinked_paths() {
        let f = Fixture::new();
        let path = f.0.join("file");
        fs::write(&path, b"keep").unwrap();
        symlink(&path, f.0.join("alias")).unwrap();
        symlink(&f.0, f.0.join("parent-alias")).unwrap();
        for bad in [
            f.0.clone(),
            f.0.join("alias"),
            f.0.join("parent-alias/file"),
            f.0.join("../file"),
            PathBuf::from("relative"),
        ] {
            assert!(ReadSession::open(&bad).is_err());
        }
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }

    #[test]
    fn rejects_directory_only_suffix_on_regular_file() {
        let f = Fixture::new();
        let path = f.0.join("file");
        fs::write(&path, b"keep").unwrap();
        for bad in [f.0.join("file/"), f.0.join("file/."), f.0.join("file//.")] {
            assert!(ReadSession::open(&bad).is_err());
        }
        assert!(ReadSession::open(&f.0.join("./file")).is_ok());
        assert_eq!(fs::read(&path).unwrap(), b"keep");
    }

    #[test]
    fn detects_observed_content_and_identity_changes() {
        let f = Fixture::new();
        let path = f.0.join("file");
        fs::write(&path, b"keep").unwrap();
        let session = ReadSession::open(&path).unwrap();
        fs::write(&path, b"changed length").unwrap();
        assert!(session.verify().is_err());
        let session = ReadSession::open(&path).unwrap();
        fs::rename(&path, f.0.join("original")).unwrap();
        fs::write(&path, b"changed length").unwrap();
        assert!(session.verify().is_err());
        assert_eq!(fs::read(f.0.join("original")).unwrap(), b"changed length");
        assert_eq!(fs::read(&path).unwrap(), b"changed length");
    }

    #[test]
    fn detects_replaced_parent_directory() {
        let f = Fixture::new();
        let dir = f.0.join("parent");
        fs::create_dir(&dir).unwrap();
        let path = dir.join("file");
        fs::write(&path, b"keep").unwrap();
        let session = ReadSession::open(&path).unwrap();
        fs::rename(&dir, f.0.join("old-parent")).unwrap();
        fs::create_dir(&dir).unwrap();
        fs::write(&path, b"keep").unwrap();
        assert!(session.verify().is_err());
        assert_eq!(fs::read(f.0.join("old-parent/file")).unwrap(), b"keep");
    }
}
