//! Read a complete bounded PPM file, without following symbolic links.
use super::{invalid, ppm_thumbnail, Thumbnail, MAX_HEADER, MAX_PIXELS};
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{File, Metadata};
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

const MAX_BYTES: usize = MAX_HEADER + MAX_PIXELS * 3;

fn parent(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid("absolute image path required"));
    }
    let parent = path
        .parent()
        .ok_or_else(|| invalid("image parent required"))?;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for part in parent.components() {
        match part {
            Component::RootDir => (),
            Component::Normal(name) => dir = rx::openat(&dir, name, flags, Mode::empty())?.into(),
            _ => return Err(invalid("invalid image path component")),
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
        .ok_or_else(|| invalid("image filename required"))?;
    let stat = rx::statat(dir, name, rx::AtFlags::SYMLINK_NOFOLLOW)?;
    if rx::FileType::from_raw_mode(stat.st_mode) != rx::FileType::RegularFile {
        return Err(invalid("image preview requires a regular non-link file"));
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
        return Err(invalid("image changed before opening"));
    }
    Ok(file)
}

/// Bounded read-only access and observed metadata rechecks, not a consistent
/// snapshot or a guarantee of bounded blocking-I/O time.
pub fn thumbnail_file(path: &Path, columns: usize, rows: usize) -> io::Result<Thumbnail> {
    let dir = parent(path)?;
    let file = open(&dir, path)?;
    let before = file.metadata()?;
    if before.len() > MAX_BYTES as u64 {
        return Err(invalid("image preview byte budget exceeded"));
    }
    let mut bytes = Vec::new();
    (&file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES || bytes.len() as u64 != before.len() {
        return Err(invalid("image size changed or exceeds preview limits"));
    }
    let thumbnail = ppm_thumbnail(&bytes, columns, rows)?;
    let current_dir = parent(path)?;
    let a = dir.metadata()?;
    let b = current_dir.metadata()?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino())
        || !unchanged(&before, &file.metadata()?)
        || !unchanged(&before, &open(&current_dir, path)?.metadata()?)
    {
        return Err(invalid("image changed during preview"));
    }
    Ok(thumbnail)
}
