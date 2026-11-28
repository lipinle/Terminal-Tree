//! Descriptor-based read-only access to strict USTAR files on Linux.
use super::{invalid, list, Member, MAX_ARCHIVE_BYTES};
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io::{self, BufReader};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

fn parent(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid("absolute archive path required"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("archive parent required"))?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for part in parent.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(name) => dir = rx::openat(&dir, name, flags, Mode::empty())?.into(),
            _ => return Err(invalid("invalid archive path component")),
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
        .ok_or_else(|| invalid("archive filename required"))?;
    // Refuse observed nonregular files before opening (including devices).
    let stat = rx::statat(dir, name, rx::AtFlags::SYMLINK_NOFOLLOW)?;
    if rx::FileType::from_raw_mode(stat.st_mode) != rx::FileType::RegularFile {
        return Err(invalid("archive requires a regular non-link file"));
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
        return Err(invalid("archive changed before opening"));
    }
    Ok(file)
}

/// Read a bounded regular file using no-follow directory descriptors.
/// Metadata rechecks detect observed changes; not a consistent snapshot.
/// No writes, extraction, or blocking-I/O time guarantee.
pub fn list_file(path: &Path) -> io::Result<Vec<Member>> {
    let dir = parent(path)?;
    let file = open(&dir, path)?;
    let before = file.metadata()?;
    if before.len() > MAX_ARCHIVE_BYTES as u64 {
        return Err(invalid("USTAR byte budget exceeded"));
    }
    let members = list(BufReader::new(&file))?;
    let current_dir = parent(path)?;
    let a = dir.metadata()?;
    let b = current_dir.metadata()?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino())
        || !unchanged(&before, &file.metadata()?)
        || !unchanged(&before, &open(&current_dir, path)?.metadata()?)
    {
        return Err(invalid("archive changed during listing"));
    }
    Ok(members)
}
