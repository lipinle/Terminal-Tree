//! Descriptor-based, bounded USTAR and gzip-compressed USTAR access.
use super::{
    invalid, layout_preview, list, member_payload, portable_layout_preview, LayoutPreview, Member,
    MAX_ARCHIVE_BYTES,
};
use crate::readonly_file::ReadSession;
use std::io::{self, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

const MAX_COMPRESSED_BYTES: usize = MAX_ARCHIVE_BYTES + 1024 * 1024;

// Feed and drain concurrently so neither pipe can deadlock on large data.
// Fixed program and arguments, no filenames, no shell, no inherited GZIP
// options. A nonzero exit, truncated input or output limit returns no bytes.
fn gzip_filter(input: &[u8], decompress: bool, limit: usize) -> io::Result<Vec<u8>> {
    let mut child = Command::new("/usr/bin/gzip")
        .args(if decompress {
            ["-d", "-c"]
        } else {
            ["-n", "-c"]
        })
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    std::thread::scope(|scope| {
        let writer = scope.spawn(move || stdin.write_all(input));
        let mut output = Vec::new();
        let read = stdout.take(limit as u64 + 1).read_to_end(&mut output);
        if read.is_err() || output.len() > limit {
            let _ = child.kill();
        }
        let status = child.wait();
        let written = writer
            .join()
            .map_err(|_| io::Error::other("gzip input worker failed"))?;
        read?;
        if output.len() > limit {
            return Err(invalid("gzip output budget exceeded"));
        }
        if !status?.success() {
            return Err(invalid("gzip stream refused"));
        }
        written?;
        Ok(output)
    })
}

/// Validate complete portable USTAR before gzip encoding.
pub fn compress_archive(bytes: &[u8]) -> io::Result<Vec<u8>> {
    portable_layout_preview(bytes)?;
    gzip_filter(bytes, false, MAX_COMPRESSED_BYTES)
}

/// Read bounded regular files, recognizing gzip by its magic bytes.
/// Decompression is capped independently of compressed input size.
/// The caller must still validate the resulting USTAR before using it.
pub fn read_archive_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let session = ReadSession::open(path)?;
    let len = session.len();
    if len > MAX_COMPRESSED_BYTES as u64 {
        return Err(invalid("archive input budget exceeded"));
    }
    let mut bytes = Vec::with_capacity(len as usize);
    session.file().take(len + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 != len {
        return Err(invalid("archive input length changed"));
    }
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        gzip_filter(&bytes, true, MAX_ARCHIVE_BYTES)?
    } else {
        if bytes.len() > MAX_ARCHIVE_BYTES {
            return Err(invalid("USTAR byte budget exceeded"));
        }
        bytes
    };
    session.verify()?;
    Ok(bytes)
}

pub fn list_file(path: &Path) -> io::Result<Vec<Member>> {
    list(read_archive_bytes(path)?.as_slice())
}

pub fn layout_file(path: &Path) -> io::Result<LayoutPreview> {
    layout_preview(read_archive_bytes(path)?.as_slice())
}

pub fn portable_layout_file(path: &Path) -> io::Result<LayoutPreview> {
    portable_layout_preview(read_archive_bytes(path)?.as_slice())
}

/// Return a regular member only after full portable archive validation.
pub fn read_member(path: &Path, name: &str) -> io::Result<Vec<u8>> {
    let bytes = read_archive_bytes(path)?;
    Ok(member_payload(&bytes, name)?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gzip_filter_refuses_output_beyond_limit_and_corrupt_streams() {
        let input = vec![b'x'; 4096];
        let compressed = gzip_filter(&input, false, 4096).unwrap();
        assert_eq!(gzip_filter(&compressed, true, 4096).unwrap(), input);
        assert!(gzip_filter(&compressed, true, 4095).is_err());
        assert!(gzip_filter(&compressed[..compressed.len() - 1], true, 4096).is_err());
    }
}
