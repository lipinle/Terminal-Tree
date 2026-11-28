//! Read-only directory preflight. Never authorizes or executes a rename.
use super::{replace_literal, suggest_available_name, Rename, MAX_DIRECTORY_NAMES, MAX_ITEMS};
use rustix::fs::{self as rx, AtFlags, Mode, OFlags};
use std::ffi::{OsStr, OsString};
use std::fs::{self, File, Metadata};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path};

fn open_directory(path: &Path) -> io::Result<File> {
    if !path.is_absolute() {
        return Err(io::Error::other("absolute rename directory required"));
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut dir: File = rx::open("/", flags, Mode::empty())?.into();
    for component in path.components() {
        match component {
            Component::RootDir => (),
            Component::Normal(name) => {
                dir = rx::openat(&dir, name, flags, Mode::empty())?.into();
            }
            _ => return Err(io::Error::other("invalid rename directory component")),
        }
    }
    Ok(dir)
}

fn unchanged(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn directory_names(dir: &File) -> io::Result<Vec<OsString>> {
    let mut existing = Vec::new();
    for entry in fs::read_dir(format!("/proc/self/fd/{}", dir.as_raw_fd()))? {
        if existing.len() == MAX_DIRECTORY_NAMES {
            return Err(io::Error::other("rename directory scan limit exceeded"));
        }
        existing.push(entry?.file_name());
    }
    Ok(existing)
}

/// Suggest a sibling name using a bounded observation of the directory.
/// A native alias or observed directory change invalidates the suggestion.
/// No reservation is made; the write engine must independently review it.
pub fn suggest(directory: &Path, requested: &OsStr) -> io::Result<OsString> {
    let dir = open_directory(directory)?;
    let before = dir.metadata()?;
    let existing = directory_names(&dir)?;
    let candidate = suggest_available_name(&existing, requested)
        .map_err(|error| io::Error::other(format!("name suggestion refused: {error:?}")))?;
    match rx::statat(&dir, &candidate, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(_) => return Err(io::Error::other("suggested name occupied")),
        Err(error) if error == rustix::io::Errno::NOENT => (),
        Err(error) => return Err(error.into()),
    }
    let reopened = open_directory(directory)?;
    if !unchanged(&before, &dir.metadata()?) || !unchanged(&before, &reopened.metadata()?) {
        return Err(io::Error::other("directory changed during name suggestion"));
    }
    Ok(candidate)
}

/// Observe a bounded directory, including hidden names, then generate an
/// explicit selected-name replacement plan. Check each destination against
/// the native filesystem as well as the exact-name snapshot. A case-folding
/// filesystem may still alias two currently absent destinations. Results are
/// diagnostic only, not a consistent snapshot or permission to perform writes.
pub fn preview(
    directory: &Path,
    selected: &[OsString],
    needle: &OsStr,
    replacement: &OsStr,
) -> io::Result<Vec<Rename>> {
    if selected.is_empty() || selected.len() > MAX_ITEMS {
        return Err(io::Error::other("select 1..=1000 filenames"));
    }
    let dir = open_directory(directory)?;
    let before = dir.metadata()?;
    let existing = directory_names(&dir)?;
    let plan = replace_literal(&existing, selected, needle, replacement)
        .map_err(|error| io::Error::other(format!("rename plan refused: {error:?}")))?;
    for item in &plan {
        // No link following. Refuse an observed occupied destination even
        // if its native spelling differs from the directory snapshot.
        rx::statat(&dir, &item.source, AtFlags::SYMLINK_NOFOLLOW)?;
        match rx::statat(&dir, &item.destination, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(_) => return Err(io::Error::other("rename destination occupied")),
            Err(error) if error == rustix::io::Errno::NOENT => (),
            Err(error) => return Err(error.into()),
        }
    }
    let reopened = open_directory(directory)?;
    if !unchanged(&before, &dir.metadata()?) || !unchanged(&before, &reopened.metadata()?) {
        return Err(io::Error::other("rename directory changed during preview"));
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;

    #[test]
    fn suggestion_respects_raw_names_and_dangling_links_without_writes() {
        let root = std::env::temp_dir().join(format!("tt-name-suggestion-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        struct Cleanup(std::path::PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(root.clone());
        let raw = OsString::from_vec(b".name-\xff".to_vec());
        let first = OsString::from_vec(b".name-\xff (1)".to_vec());
        fs::write(root.join(&raw), b"keep").unwrap();
        symlink(root.join("missing"), root.join(&first)).unwrap();
        let candidate = suggest(&root, &raw).unwrap();
        assert_eq!(candidate, OsString::from_vec(b".name-\xff (2)".to_vec()));
        assert!(fs::symlink_metadata(root.join(candidate)).is_err());
        assert_eq!(fs::read(root.join(&raw)).unwrap(), b"keep");
        assert!(fs::symlink_metadata(root.join(first))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
        assert!(suggest(&root, OsStr::new("../escape")).is_err());
    }
}
