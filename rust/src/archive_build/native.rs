//! Build an archive from explicitly named regular files, without opening an output path.
use super::{build, extend_budget, invalid, Entry, BLOCK, MAX_BYTES, MAX_ENTRIES};
use crate::archive::{portable_layout_preview, Kind};
use crate::readonly_file::ReadSession;
use std::io::{self, Read};
use std::path::PathBuf;

/// Read bounded regular-file inputs with observed identity rechecks.
/// Return a complete portable archive, not a consistent multi-file snapshot.
/// Metadata is normalized; this is not a backup or a write authorization.
pub fn build_files(files: &[(String, PathBuf)]) -> io::Result<Vec<u8>> {
    if files.is_empty() || files.len() > MAX_ENTRIES {
        return Err(invalid("select 1..=4096 archive input files"));
    }
    let mut total = 2 * BLOCK;
    let mut payloads = Vec::with_capacity(files.len());
    for (name, path) in files {
        if name.is_empty() || name.len() > 100 || name.as_bytes().contains(&0) {
            return Err(invalid("invalid archive member name"));
        }
        let session = ReadSession::open(path)?;
        let len = session.len();
        if len > MAX_BYTES as u64 {
            return Err(invalid("USTAR byte budget exceeded"));
        }
        total = extend_budget(total, len as usize)?;
        let mut data = Vec::with_capacity(len as usize);
        session.file().take(len + 1).read_to_end(&mut data)?;
        if data.len() as u64 != len {
            return Err(invalid("archive input length changed"));
        }
        session.verify()?;
        payloads.push(data);
    }
    let entries: Vec<_> = files
        .iter()
        .zip(&payloads)
        .map(|((name, _), data)| Entry {
            name,
            kind: Kind::File,
            data,
        })
        .collect();
    let archive = build(&entries)?;
    portable_layout_preview(archive.as_slice())?;
    Ok(archive)
}
