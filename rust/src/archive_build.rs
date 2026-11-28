//! Bounded in-memory POSIX USTAR construction for explicit file/directory data.
//! No filesystem traversal, destination creation, metadata restoration or compression.
use crate::archive::{list, Kind};
use std::io;

const BLOCK: usize = 512;
const MAX_BYTES: usize = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 4096;

#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod native;

pub struct Entry<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub data: &'a [u8],
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}

fn extend_budget(total: usize, len: usize) -> io::Result<usize> {
    let padded = len
        .checked_add(BLOCK - 1)
        .ok_or_else(|| invalid("USTAR byte budget exceeded"))?
        / BLOCK
        * BLOCK;
    total
        .checked_add(BLOCK)
        .and_then(|n| n.checked_add(padded))
        .filter(|&n| n <= MAX_BYTES)
        .ok_or_else(|| invalid("USTAR byte budget exceeded"))
}

/// Construct a complete archive using a deterministic, minimal metadata profile.
/// Names must fit the 100-byte USTAR name field (no prefix splitting yet).
/// Duplicate names, file ancestors and unsupported names are rejected by the
/// existing strict reader before the bytes are returned. Input data is borrowed.
pub fn build(entries: &[Entry<'_>]) -> io::Result<Vec<u8>> {
    if entries.len() > MAX_ENTRIES {
        return Err(invalid("USTAR member limit exceeded"));
    }
    let mut total = 2 * BLOCK;
    for entry in entries {
        if entry.name.is_empty()
            || entry.name.len() > 100
            || entry.name.as_bytes().contains(&0)
            || (entry.kind == Kind::Directory && !entry.data.is_empty())
        {
            return Err(invalid("invalid USTAR build entry"));
        }
        total = extend_budget(total, entry.data.len())?;
    }
    let mut output = Vec::with_capacity(total);
    for entry in entries {
        let mut header = [0u8; BLOCK];
        header[..entry.name.len()].copy_from_slice(entry.name.as_bytes());
        header[100..108].copy_from_slice(match entry.kind {
            Kind::File => b"0000644\0",
            Kind::Directory => b"0000755\0",
        });
        header[108..116].copy_from_slice(b"0000000\0");
        header[116..124].copy_from_slice(b"0000000\0");
        header[124..136].copy_from_slice(format!("{:011o}\0", entry.data.len()).as_bytes());
        header[136..148].copy_from_slice(b"00000000000\0");
        header[148..156].fill(b' ');
        header[156] = match entry.kind {
            Kind::File => b'0',
            Kind::Directory => b'5',
        };
        header[257..265].copy_from_slice(b"ustar\x0000");
        let checksum: u32 = header.iter().map(|&b| u32::from(b)).sum();
        header[148..156].copy_from_slice(format!("{checksum:06o}\0 ").as_bytes());
        output.extend_from_slice(&header);
        output.extend_from_slice(entry.data);
        output.resize(output.len() + (BLOCK - entry.data.len() % BLOCK) % BLOCK, 0);
    }
    output.resize(total, 0);
    list(output.as_slice())?;
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file<'a>(name: &'a str, data: &'a [u8]) -> Entry<'a> {
        Entry {
            name,
            kind: Kind::File,
            data,
        }
    }

    #[test]
    fn deterministic_round_trip_preserves_payload_and_padding() {
        let entries = [
            Entry {
                name: "folder/",
                kind: Kind::Directory,
                data: b"",
            },
            file("folder/file", b"hello"),
            file("empty", b""),
        ];
        let bytes = build(&entries).unwrap();
        assert_eq!(bytes, build(&entries).unwrap());
        let members = list(bytes.as_slice()).unwrap();
        assert_eq!(members.len(), 3);
        assert_eq!(members[0].kind, Kind::Directory);
        assert_eq!(members[1].name, "folder/file");
        assert_eq!(members[1].size, 5);
        assert_eq!(&bytes[1024..1029], b"hello");
        assert!(bytes[1029..1536].iter().all(|&b| b == 0));
        assert_eq!(build(&[]).unwrap(), vec![0; 1024]);
    }

    #[test]
    fn refuses_invalid_names_collisions_and_limits() {
        for name in [
            "",
            "../escape",
            "/absolute",
            "a//b",
            "bad\0name",
            "a/.",
            "a/",
        ] {
            assert!(build(&[file(name, b"")]).is_err());
        }
        assert!(build(&[file(&"x".repeat(101), b"")]).is_err());
        assert!(build(&[file("a", b""), file("a", b"")]).is_err());
        assert!(build(&[file("a", b""), file("a/b", b"")]).is_err());
        assert!(extend_budget(2 * BLOCK, MAX_BYTES).is_err());
        assert_eq!(
            extend_budget(2 * BLOCK, MAX_BYTES - 3 * BLOCK).unwrap(),
            MAX_BYTES
        );
    }
}
