//! Bounded, read-only filename search. Results never authorize file operations.
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod content;

use crate::{scan_bounded, Entry, Order};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_ENTRIES: usize = 16_384;
const MAX_DEPTH: usize = 16;
const MAX_QUERY_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug)]
pub struct SearchLimits {
    pub entries: usize,
    pub depth: usize,
    pub matches: usize,
}

fn check_cancel(cancel: &AtomicBool) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "search cancelled",
        ));
    }
    Ok(())
}

fn plain_directory(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("search requires a plain directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(io::Error::other("search refuses reparse points"));
        }
    }
    Ok(metadata)
}

// Recheck path components before and after enumeration. These pathname checks
// are best effort for read-only discovery, not race-free authorization.
fn check_directory(root: &Path, path: &Path) -> io::Result<()> {
    let root_metadata = plain_directory(root)?;
    if root.canonicalize()? != root {
        return Err(io::Error::other("search root changed"));
    }
    let relative = path
        .strip_prefix(root)
        .map_err(|_| io::Error::other("search path outside root"))?;
    let mut current = root.to_path_buf();
    for part in relative.components() {
        if !matches!(part, std::path::Component::Normal(_)) {
            return Err(io::Error::other("invalid search path component"));
        }
        current.push(part);
        let metadata = plain_directory(&current)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.dev() != root_metadata.dev() {
                return Err(io::Error::other("search refuses device changes"));
            }
        }
        #[cfg(not(unix))]
        let _ = (&root_metadata, &metadata);
    }
    Ok(())
}

// Compare the observed root identity on Unix. This does not pin a directory
// descriptor or guarantee a consistent filesystem snapshot.
fn check_root_identity(root: &Path, original: &fs::Metadata) -> io::Result<()> {
    let current = plain_directory(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if (current.dev(), current.ino()) != (original.dev(), original.ino()) {
            return Err(io::Error::other("search root identity changed"));
        }
    }
    #[cfg(not(unix))]
    let _ = (current, original);
    if root.canonicalize()? != root {
        return Err(io::Error::other("search root changed"));
    }
    Ok(())
}

/// Search filenames recursively, without descending into observed directory links.
/// Count hidden/nonmatching entries against the scan budget. Return an error,
/// not incomplete results, on cancellation, scan failure or limit exhaustion.
/// Cancellation is cooperative; blocking filesystem calls have no time deadline.
pub fn filenames(
    root: &Path,
    query: &str,
    hidden: bool,
    limits: SearchLimits,
    cancel: &AtomicBool,
) -> io::Result<Vec<Entry>> {
    if !(1..=MAX_ENTRIES).contains(&limits.entries)
        || !(1..=MAX_DEPTH).contains(&limits.depth)
        || !(1..=limits.entries).contains(&limits.matches)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid search limits",
        ));
    }
    if query.len() > MAX_QUERY_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "search query exceeds byte limit",
        ));
    }
    check_cancel(cancel)?;
    let root = root.canonicalize()?;
    let original = plain_directory(&root)?;
    let query = query.to_lowercase();
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.clone(), 0)];
    let mut remaining = limits.entries;
    let mut matches = Vec::new();
    while let Some((directory, depth)) = stack.pop() {
        check_cancel(cancel)?;
        check_root_identity(&root, &original)?;
        check_directory(&root, &directory)?;
        let entries = scan_bounded(&directory, true, Order::Name, "", remaining)?;
        check_directory(&root, &directory)?;
        check_root_identity(&root, &original)?;
        remaining = remaining.checked_sub(entries.len())
            .ok_or_else(|| io::Error::other("search entry budget exceeded"))?;
        for entry in entries {
            check_cancel(cancel)?;
            let name = entry
                .path
                .file_name()
                .ok_or_else(|| io::Error::other("missing search entry name"))?;
            if !hidden && name.as_encoded_bytes().starts_with(b".") {
                continue;
            }
            if entry.directory && !entry.symlink && depth + 1 < limits.depth {
                stack.push((entry.path.clone(), depth + 1));
            }
            if name.to_string_lossy().to_lowercase().contains(&query) {
                if matches.len() == limits.matches {
                    return Err(io::Error::other("search match limit exceeded"));
                }
                matches.push(entry);
            }
        }
    }
    check_cancel(cancel)?;
    check_root_identity(&root, &original)?;
    Ok(matches)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn replaced_plain_root_is_detected() {
        let parent = std::env::temp_dir().join(format!("tt-search-root-{}", std::process::id()));
        fs::create_dir(&parent).unwrap();
        let root = parent.join("root");
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let original = plain_directory(&root).unwrap();
        assert!(check_root_identity(&root, &original).is_ok());
        fs::rename(&root, parent.join("old")).unwrap();
        fs::create_dir(&root).unwrap();
        let result = check_root_identity(&root, &original);
        fs::remove_dir_all(&parent).unwrap();
        assert!(result.is_err());
    }
}
