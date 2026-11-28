//! Descriptor-based read-only access to strict USTAR files on Linux.
use super::{
    invalid, layout_preview, list, portable_layout_preview, LayoutPreview, Member, MAX_ARCHIVE_BYTES,
};
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

/// Return the complete validated archive structure including implicit parents.
/// No output is returned until the stream and observed file identity pass.
/// This does not validate or authorize extraction to any destination.
pub fn layout_file(path: &Path) -> io::Result<LayoutPreview> {
    let session = ReadSession::open(path)?;
    if session.len() > MAX_ARCHIVE_BYTES as u64 {
        return Err(invalid("USTAR byte budget exceeded"));
    }
    let layout = layout_preview(BufReader::new(session.file()))?;
    session.verify()?;
    Ok(layout)
}

/// Apply the conservative portable-name profile before returning the layout.
/// Still a read-only observation, not an extraction authorization.
pub fn portable_layout_file(path: &Path) -> io::Result<LayoutPreview> {
    let session = ReadSession::open(path)?;
    if session.len() > MAX_ARCHIVE_BYTES as u64 {
        return Err(invalid("USTAR byte budget exceeded"));
    }
    let layout = portable_layout_preview(BufReader::new(session.file()))?;
    session.verify()?;
    Ok(layout)
}
