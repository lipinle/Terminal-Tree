//! Read a complete bounded PPM file, without following symbolic links.
use super::{invalid, ppm_thumbnail, Thumbnail, MAX_HEADER, MAX_PIXELS};
use crate::readonly_file::ReadSession;
use std::io::{self, Read};
use std::path::Path;

const MAX_BYTES: usize = MAX_HEADER + MAX_PIXELS * 3;

/// Bounded read-only access and observed metadata rechecks, not a consistent
/// snapshot or a guarantee of bounded blocking-I/O time.
pub fn thumbnail_file(path: &Path, columns: usize, rows: usize) -> io::Result<Thumbnail> {
    let session = ReadSession::open(path)?;
    if session.len() > MAX_BYTES as u64 {
        return Err(invalid("image preview byte budget exceeded"));
    }
    let mut bytes = Vec::new();
    session
        .file()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_BYTES || bytes.len() as u64 != session.len() {
        return Err(invalid("image size changed or exceeds preview limits"));
    }
    let thumbnail = ppm_thumbnail(&bytes, columns, rows)?;
    session.verify()?;
    Ok(thumbnail)
}
