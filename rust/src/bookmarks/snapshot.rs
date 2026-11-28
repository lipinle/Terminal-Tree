//! Explicit native bookmark snapshots. No automatic writes or replacement.
use super::{Bookmarks, MAX_BOOKMARKS, MAX_PATH_BYTES};
use rustix::fs::{self as rx, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};
use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

const MAGIC: &[u8; 8] = b"TTBOOK01";
const DIGEST_BYTES: usize = 32;
const MAX_BYTES: usize = MAGIC.len() + 4 + MAX_BOOKMARKS * (4 + MAX_PATH_BYTES) + DIGEST_BYTES;

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

/// Native path bytes are stored losslessly; digest detects accidental damage,
/// not authenticity. Decoding never accesses a recorded location.
pub fn encode(marks: &Bookmarks) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&(marks.paths().len() as u32).to_le_bytes());
    for path in marks.paths() {
        let raw = path.as_os_str().as_bytes();
        bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        bytes.extend_from_slice(raw);
    }
    let digest = Sha256::digest(&bytes);
    bytes.extend_from_slice(&digest);
    bytes
}

fn take<'a>(bytes: &mut &'a [u8], size: usize) -> io::Result<&'a [u8]> {
    if bytes.len() < size {
        return Err(invalid("truncated bookmark snapshot"));
    }
    let (head, tail) = bytes.split_at(size);
    *bytes = tail;
    Ok(head)
}

fn count(bytes: &mut &[u8]) -> io::Result<usize> {
    let raw: [u8; 4] = take(bytes, 4)?
        .try_into()
        .map_err(|_| invalid("invalid snapshot length"))?;
    Ok(u32::from_le_bytes(raw) as usize)
}

pub fn decode(bytes: &[u8]) -> io::Result<Bookmarks> {
    if !(MAGIC.len() + 4 + DIGEST_BYTES..=MAX_BYTES).contains(&bytes.len()) {
        return Err(invalid("invalid bookmark snapshot size"));
    }
    let (mut payload, digest) = bytes.split_at(bytes.len() - DIGEST_BYTES);
    if &Sha256::digest(payload)[..] != digest || take(&mut payload, MAGIC.len())? != MAGIC {
        return Err(invalid("invalid bookmark snapshot version or digest"));
    }
    let entries = count(&mut payload)?;
    if entries > MAX_BOOKMARKS {
        return Err(invalid("bookmark count exceeds limit"));
    }
    let mut marks = Bookmarks::default();
    for _ in 0..entries {
        let size = count(&mut payload)?;
        if !(1..=MAX_PATH_BYTES).contains(&size) {
            return Err(invalid("invalid bookmark path length"));
        }
        let raw = take(&mut payload, size)?;
        marks
            .add(PathBuf::from(OsString::from_vec(raw.to_vec())))
            .map_err(|_| invalid("invalid or duplicate bookmark path"))?;
    }
    if !payload.is_empty() {
        return Err(invalid("unexpected trailing bookmark data"));
    }
    Ok(marks)
}

fn private_dir(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(invalid("absolute snapshot directory required"));
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                dir = rx::openat(&dir, name, flags, Mode::empty())?.into();
            }
            _ => return Err(invalid("invalid snapshot directory component")),
        }
    }
    let metadata = dir.metadata()?;
    if metadata.uid() != rustix::process::getuid().as_raw() || metadata.mode() & 0o077 != 0 {
        return Err(invalid("private owned snapshot directory required"));
    }
    Ok(dir)
}

fn parts(path: &Path) -> io::Result<(&Path, &OsStr)> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("snapshot directory required"))?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("snapshot filename required"))?;
    if !path.is_absolute() || name.as_bytes().contains(&0) {
        return Err(invalid("invalid snapshot path"));
    }
    Ok((parent, name))
}

fn check_dir(dir: &File, path: &Path) -> io::Result<()> {
    let original = dir.metadata()?;
    let current = private_dir(path)?.metadata()?;
    if (original.dev(), original.ino()) != (current.dev(), current.ino()) {
        return Err(invalid("snapshot directory replaced"));
    }
    Ok(())
}

/// Load a bounded, private regular file. Return a complete new model or error.
pub fn load(path: &Path) -> io::Result<Bookmarks> {
    let (parent, name) = parts(path)?;
    let dir = private_dir(parent)?;
    let file: File = rx::openat(
        &dir,
        name,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let before = file.metadata()?;
    if !before.is_file()
        || before.nlink() != 1
        || before.uid() != rustix::process::getuid().as_raw()
        || before.mode() & 0o077 != 0
        || before.len() > MAX_BYTES as u64
    {
        return Err(invalid("invalid bookmark snapshot file"));
    }
    let mut bytes = Vec::new();
    (&file)
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    let after = file.metadata()?;
    if bytes.len() as u64 != before.len()
        || after.len() != before.len()
        || after.mtime() != before.mtime()
        || after.mtime_nsec() != before.mtime_nsec()
        || after.ctime() != before.ctime()
        || after.ctime_nsec() != before.ctime_nsec()
        || after.nlink() != 1
    {
        return Err(invalid("bookmark snapshot changed during read"));
    }
    check_dir(&dir, parent)?;
    decode(&bytes)
}

/// Explicit save to a new filename. Never replace an existing destination.
/// A failure may leave a private temporary file; do not blindly unlink it.
pub fn save_new(path: &Path, marks: &Bookmarks) -> io::Result<()> {
    let (parent, name) = parts(path)?;
    let dir = private_dir(parent)?;
    // Refuse an observed existing name before creating a temporary file.
    // The atomic NOREPLACE rename below remains the authoritative gate.
    match rx::statat(&dir, name, rx::AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "bookmark snapshot already exists",
            ));
        }
        Err(error) if error == rustix::io::Errno::NOENT => (),
        Err(error) => return Err(error.into()),
    }
    let mut random = [0u8; 16];
    File::open("/dev/urandom")?.read_exact(&mut random)?;
    let suffix = random
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let temporary = format!(".tt-bookmarks-{suffix}.tmp");
    let mut file: File = rx::openat(
        &dir,
        &temporary,
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::from_bits_truncate(0o600),
    )?
    .into();
    file.write_all(&encode(marks))?;
    file.sync_all()?;
    check_dir(&dir, parent)?;
    rx::renameat_with(&dir, &temporary, &dir, name, RenameFlags::NOREPLACE)?;
    dir.sync_all()
}
