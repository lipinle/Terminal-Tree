//! Descriptor-based read-only access to strict USTAR files on Linux.
use super::{invalid, list, Member, MAX_ARCHIVE_BYTES};
use crate::readonly_file::ReadSession;
use std::io::{self, BufReader};
use std::path::Path;

/// Read a bounded regular file using no-follow directory descriptors.
/// Metadata rechecks detect observed changes; not a consistent snapshot.
/// No writes, extraction, or blocking-I/O time guarantee.
pub fn list_file(path: &Path) -> io::Result<Vec<Member>> {
    let session = ReadSession::open(path)?;
    if session.len() > MAX_ARCHIVE_BYTES as u64 {
        return Err(invalid("USTAR byte budget exceeded"));
    }
    let members = list(BufReader::new(session.file()))?;
    session.verify()?;
    Ok(members)
}
