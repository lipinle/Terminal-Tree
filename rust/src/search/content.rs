//! Native Linux text search with explicit resource budgets and omissions.
use super::{check_cancel, check_directory, filenames, SearchLimits, MAX_QUERY_BYTES};
use crate::readonly_file::ReadSession;
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

const MAX_FILE_BYTES: usize = 1_048_576;
const MAX_TOTAL_BYTES: usize = 64 * 1_048_576;

#[derive(Clone, Copy, Debug)]
pub struct ContentLimits {
    pub traversal: SearchLimits,
    pub file_bytes: usize,
    pub total_bytes: usize,
}

#[derive(Debug, Default)]
pub struct ContentReport {
    pub matches: Vec<PathBuf>,
    pub files_read: usize,
    pub bytes_read: usize,
    pub skipped_large: usize,
    pub skipped_binary: usize,
    pub skipped_nonregular: usize,
}

fn unchanged(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    b.is_file()
        && a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

/// Search complete, bounded UTF-8 files for an exact case-sensitive substring.
/// NUL-bearing or invalid UTF-8 files and oversized files are counted, not
/// silently treated as nonmatches. Errors/cancellation return no partial report.
/// Directory identity checks are best effort; this is not write authorization
/// or a consistent filesystem snapshot. Blocking reads have no time deadline.
pub fn text(
    root: &Path,
    query: &str,
    hidden: bool,
    limits: ContentLimits,
    cancel: &AtomicBool,
) -> io::Result<ContentReport> {
    if query.is_empty()
        || query.len() > MAX_QUERY_BYTES
        || !(1..=MAX_FILE_BYTES).contains(&limits.file_bytes)
        || !(1..=MAX_TOTAL_BYTES).contains(&limits.total_bytes)
        || !(1..=limits.traversal.entries).contains(&limits.traversal.matches)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid content search limits or query",
        ));
    }
    check_cancel(cancel)?;
    let root = root.canonicalize()?;
    let device = fs::symlink_metadata(&root)?.dev();
    let candidates = filenames(
        &root,
        "",
        hidden,
        SearchLimits {
            matches: limits.traversal.entries,
            ..limits.traversal
        },
        cancel,
    )?;
    let mut report = ContentReport::default();
    for entry in candidates {
        check_cancel(cancel)?;
        if entry.symlink {
            report.skipped_nonregular += 1;
            continue;
        }
        if entry.directory {
            continue;
        }
        let parent = entry
            .path
            .parent()
            .ok_or_else(|| io::Error::other("missing content search parent"))?;
        check_directory(&root, parent)?;
        let before = fs::symlink_metadata(&entry.path)?;
        if !before.is_file() {
            report.skipped_nonregular += 1;
            continue;
        }
        if before.dev() != device {
            return Err(io::Error::other("content search refuses device changes"));
        }
        if before.len() > limits.file_bytes as u64 {
            report.skipped_large += 1;
            continue;
        }
        let remaining = limits.total_bytes - report.bytes_read;
        // Reserve a sentinel for growth/EOF detection. Fail closed if the
        // remaining aggregate budget cannot accommodate the observed file.
        if before.len() >= remaining as u64 {
            return Err(io::Error::other("content search byte budget exhausted"));
        }
        let session = ReadSession::open(&entry.path)?;
        let opened = session.file().metadata()?;
        if !unchanged(&before, &opened) {
            return Err(io::Error::other("file changed before content search"));
        }
        let read_cap = (limits.file_bytes + 1).min(remaining);
        let mut bytes = Vec::new();
        session
            .file()
            .take(read_cap as u64)
            .read_to_end(&mut bytes)?;
        report.bytes_read += bytes.len();
        check_cancel(cancel)?;
        check_directory(&root, parent)?;
        session.verify()?;
        if !unchanged(&opened, &fs::symlink_metadata(&entry.path)?)
            || bytes.len() as u64 != opened.len()
        {
            return Err(io::Error::other("file changed during content search"));
        }
        report.files_read += 1;
        if bytes.contains(&0) {
            report.skipped_binary += 1;
            continue;
        }
        let Ok(content) = std::str::from_utf8(&bytes) else {
            report.skipped_binary += 1;
            continue;
        };
        if content.contains(query) {
            if report.matches.len() == limits.traversal.matches {
                return Err(io::Error::other("content search match limit exceeded"));
            }
            report.matches.push(entry.path);
        }
    }
    check_cancel(cancel)?;
    Ok(report)
}
