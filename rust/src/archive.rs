//! Strict, bounded USTAR listing only. No extraction or filesystem writes.
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Read};

#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod native;

const BLOCK: usize = 512;
const MAX_ARCHIVE_BYTES: usize = 64 * 1024 * 1024;
const MAX_MEMBERS: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    File,
    Directory,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Member {
    pub name: String,
    pub kind: Kind,
    pub size: u64,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn field(bytes: &[u8]) -> io::Result<&str> {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    if bytes[end..].iter().any(|&b| b != 0) {
        return Err(invalid("invalid USTAR string padding"));
    }
    std::str::from_utf8(&bytes[..end]).map_err(io::Error::other)
}

fn octal(bytes: &[u8]) -> io::Result<u64> {
    let mut start = 0;
    let mut end = bytes.len();
    while start < end && bytes[start] == b' ' {
        start += 1;
    }
    while end > start && matches!(bytes[end - 1], 0 | b' ') {
        end -= 1;
    }
    if start == end {
        return Err(invalid("missing USTAR number"));
    }
    let mut value = 0u64;
    for &byte in &bytes[start..end] {
        if !(b'0'..=b'7').contains(&byte) {
            return Err(invalid("unsupported USTAR numeric encoding"));
        }
        value = value
            .checked_mul(8)
            .and_then(|v| v.checked_add(u64::from(byte - b'0')))
            .ok_or_else(|| invalid("USTAR number overflow"))?;
    }
    Ok(value)
}

fn name(header: &[u8; BLOCK], kind: Kind) -> io::Result<String> {
    let basename = field(&header[..100])?;
    if basename.is_empty() {
        return Err(invalid("empty USTAR member name"));
    }
    let prefix = field(&header[345..500])?;
    let mut path = if prefix.is_empty() {
        basename.to_owned()
    } else {
        format!("{prefix}/{basename}")
    };
    if kind == Kind::Directory && path.ends_with('/') {
        path.pop();
    }
    // Deliberately conservative portable ASCII subset. These strings are
    // listing labels, not validated destinations for a future extractor.
    if path.is_empty()
        || !path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-' | b' '))
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == ".." || p.ends_with('.') || p.ends_with(' '))
    {
        return Err(invalid("unsupported or unsafe USTAR member name"));
    }
    Ok(path)
}

fn parse(header: &[u8; BLOCK]) -> io::Result<Member> {
    if &header[257..265] != b"ustar\x0000" {
        return Err(invalid("only POSIX USTAR is supported"));
    }
    let expected = octal(&header[148..156])?;
    let checksum: u64 = header
        .iter()
        .enumerate()
        .map(|(i, &b)| u64::from(if (148..156).contains(&i) { b' ' } else { b }))
        .sum();
    if checksum != expected {
        return Err(invalid("USTAR header checksum mismatch"));
    }
    let kind = match header[156] {
        0 | b'0' => Kind::File,
        b'5' => Kind::Directory,
        _ => return Err(invalid("unsupported USTAR entry type")),
    };
    if !field(&header[157..257])?.is_empty() {
        return Err(invalid("USTAR links are unsupported"));
    }
    let size = octal(&header[124..136])?;
    if kind == Kind::Directory && size != 0 {
        return Err(invalid("USTAR directory has data"));
    }
    Ok(Member {
        name: name(header, kind)?,
        kind,
        size,
    })
}

fn block(reader: &mut impl Read, remaining: &mut usize) -> io::Result<Option<[u8; BLOCK]>> {
    let mut data = [0; BLOCK];
    // One-byte EOF probe, including at the exact budget boundary.
    let got = loop {
        match reader.read(&mut data[..1]) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => break result?,
        }
    };
    if got == 0 {
        return Ok(None);
    }
    if *remaining < BLOCK {
        return Err(invalid("USTAR byte budget exceeded"));
    }
    reader.read_exact(&mut data[1..])?;
    *remaining -= BLOCK;
    Ok(Some(data))
}

/// Inspect an uncompressed, strict POSIX USTAR stream, up to 64 MiB and
/// 4096 entries. Require two zero end blocks and zero-only trailing blocks.
/// Header checksums do not authenticate file data. Reject links, extensions,
/// duplicate names and file/directory conflicts. Errors return no partial list.
/// This API never opens a path and provides no extraction authorization.
pub fn list(mut reader: impl Read) -> io::Result<Vec<Member>> {
    let mut remaining = MAX_ARCHIVE_BYTES;
    let mut members = Vec::new();
    let mut names = BTreeMap::new();
    loop {
        let header = block(&mut reader, &mut remaining)?
            .ok_or_else(|| invalid("missing USTAR end blocks"))?;
        if header.iter().all(|&b| b == 0) {
            let second = block(&mut reader, &mut remaining)?
                .ok_or_else(|| invalid("missing second USTAR end block"))?;
            if second.iter().any(|&b| b != 0) {
                return Err(invalid("invalid USTAR end blocks"));
            }
            while let Some(tail) = block(&mut reader, &mut remaining)? {
                if tail.iter().any(|&b| b != 0) {
                    return Err(invalid("unexpected data after USTAR end"));
                }
            }
            break;
        }
        if members.len() == MAX_MEMBERS {
            return Err(invalid("USTAR member limit exceeded"));
        }
        let member = parse(&header)?;
        if names.insert(member.name.clone(), member.kind).is_some() {
            return Err(invalid("duplicate USTAR member"));
        }
        let mut size = member.size;
        if size > remaining as u64 {
            return Err(invalid("USTAR byte budget exceeded"));
        }
        while size > 0 {
            let data = block(&mut reader, &mut remaining)?
                .ok_or_else(|| invalid("truncated USTAR payload"))?;
            let used = size.min(BLOCK as u64) as usize;
            if data[used..].iter().any(|&b| b != 0) {
                return Err(invalid("nonzero USTAR payload padding"));
            }
            size -= used as u64;
        }
        members.push(member);
    }
    for member in &members {
        let mut path = member.name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            if names.get(parent) == Some(&Kind::File) {
                return Err(invalid("USTAR member has a file ancestor"));
            }
            path = parent;
        }
    }
    Ok(members)
}

/// A preview of the directory/file structure of a validated archive stream.
/// Implicit ancestor directories are included exactly once. The order is
/// parent-before-child for directories; files retain archive order.
/// This is not an extraction plan authorized for any real filesystem.
#[derive(Debug, PartialEq, Eq)]
pub struct LayoutPreview {
    pub directories: Vec<String>,
    pub files: Vec<Member>,
    pub payload_bytes: u64,
}

pub fn layout_preview(reader: impl Read) -> io::Result<LayoutPreview> {
    let members = list(reader)?;
    let mut directories = BTreeSet::new();
    let mut files = Vec::new();
    let mut payload_bytes = 0u64;
    for member in members {
        let mut path = member.name.as_str();
        while let Some((parent, _)) = path.rsplit_once('/') {
            directories.insert(parent.to_owned());
            if directories.len() > MAX_MEMBERS {
                return Err(invalid("USTAR layout directory limit exceeded"));
            }
            path = parent;
        }
        match member.kind {
            Kind::Directory => {
                directories.insert(member.name);
                if directories.len() > MAX_MEMBERS {
                    return Err(invalid("USTAR layout directory limit exceeded"));
                }
            }
            Kind::File => {
                payload_bytes = payload_bytes
                    .checked_add(member.size)
                    .ok_or_else(|| invalid("USTAR payload size overflow"))?;
                files.push(member);
            }
        }
    }
    Ok(LayoutPreview {
        directories: directories.into_iter().collect(),
        files,
        payload_bytes,
    })
}

/// Check a conservative portable naming profile, including implicit parents.
/// This rejects ASCII case aliases and common Windows reserved basenames.
/// No destination filesystem is consulted; this is not extraction authority.
pub fn portable_layout_preview(reader: impl Read) -> io::Result<LayoutPreview> {
    let layout = layout_preview(reader)?;
    let mut folded = BTreeSet::new();
    for name in layout
        .directories
        .iter()
        .map(String::as_str)
        .chain(layout.files.iter().map(|member| member.name.as_str()))
    {
        if name.split('/').any(reserved_component) {
            return Err(invalid("USTAR layout contains a reserved basename"));
        }
        if !folded.insert(name.to_ascii_lowercase()) {
            return Err(invalid("USTAR layout contains a case-folded collision"));
        }
    }
    Ok(layout)
}

/// Borrow one regular member's data from a fully validated portable archive.
/// No partial result is returned for malformed trailing data or name aliases.
/// This does not create files or authorize extraction to any destination.
pub fn member_payload<'a>(bytes: &'a [u8], requested: &str) -> io::Result<&'a [u8]> {
    let layout = portable_layout_preview(bytes)?;
    if !layout.files.iter().any(|member| member.name == requested) {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "regular archive member not found",
        ));
    }
    let mut offset = 0;
    while let Some(header) = bytes.get(offset..offset + BLOCK) {
        if header.iter().all(|&b| b == 0) {
            break;
        }
        let header: &[u8; BLOCK] = header
            .try_into()
            .map_err(|_| invalid("invalid USTAR header length"))?;
        let member = parse(header)?;
        let size = usize::try_from(member.size).map_err(io::Error::other)?;
        let start = offset + BLOCK;
        let end = start
            .checked_add(size)
            .ok_or_else(|| invalid("USTAR payload offset overflow"))?;
        let payload = bytes
            .get(start..end)
            .ok_or_else(|| invalid("truncated USTAR payload"))?;
        if member.kind == Kind::File && member.name == requested {
            return Ok(payload);
        }
        offset = end
            .checked_add((BLOCK - size % BLOCK) % BLOCK)
            .ok_or_else(|| invalid("USTAR padding offset overflow"))?;
    }
    Err(invalid("validated archive member missing"))
}

fn reserved_component(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (stem.len() == 4
            && (stem.starts_with("COM") || stem.starts_with("LPT"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'))
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    fn append(bytes: &mut Vec<u8>, name: &str, data: &[u8]) {
        let mut header = [0u8; BLOCK];
        header[..name.len()].copy_from_slice(name.as_bytes());
        header[124..136].copy_from_slice(format!("{:011o}\0", data.len()).as_bytes());
        header[148..156].fill(b' ');
        header[156] = b'0';
        header[257..265].copy_from_slice(b"ustar\x0000");
        let sum: u32 = header.iter().map(|&byte| u32::from(byte)).sum();
        header[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        bytes.extend_from_slice(&header);
        bytes.extend_from_slice(data);
        bytes.resize(bytes.len() + (BLOCK - data.len() % BLOCK) % BLOCK, 0);
    }

    #[test]
    fn layout_includes_implicit_parents_and_payload_total() {
        let mut bytes = Vec::new();
        append(&mut bytes, "a/b/one", b"123");
        append(&mut bytes, "a/two", b"45");
        bytes.resize(bytes.len() + 2 * BLOCK, 0);
        let preview = layout_preview(bytes.as_slice()).unwrap();
        assert_eq!(preview.directories, ["a", "a/b"]);
        assert_eq!(preview.files.len(), 2);
        assert_eq!(preview.files[0].name, "a/b/one");
        assert_eq!(preview.payload_bytes, 5);
    }

    #[test]
    fn portable_layout_refuses_case_aliases_and_reserved_names() {
        for names in [
            ["A/one", "a/two"],
            ["file", "FILE"],
            ["folder/CON.txt", "safe"],
            ["LPT1", "safe"],
            ["aux .txt", "safe"],
        ] {
            let mut bytes = Vec::new();
            for name in names {
                append(&mut bytes, name, b"x");
            }
            bytes.resize(bytes.len() + 2 * BLOCK, 0);
            assert!(layout_preview(bytes.as_slice()).is_ok());
            assert!(portable_layout_preview(bytes.as_slice()).is_err());
        }
        let mut bytes = Vec::new();
        append(&mut bytes, "folder/COM10.txt", b"x");
        append(&mut bytes, "folder/other", b"y");
        bytes.resize(bytes.len() + 2 * BLOCK, 0);
        assert!(portable_layout_preview(bytes.as_slice()).is_ok());
    }

    #[test]
    fn member_payload_validates_entire_archive_before_returning_bytes() {
        let mut bytes = Vec::new();
        append(&mut bytes, "first", b"hello");
        append(&mut bytes, "folder/second", &[7; 513]);
        append(&mut bytes, "empty", b"");
        bytes.resize(bytes.len() + 2 * BLOCK, 0);
        assert_eq!(member_payload(&bytes, "first").unwrap(), b"hello");
        assert_eq!(
            member_payload(&bytes, "folder/second").unwrap(),
            &[7; 513]
        );
        assert!(member_payload(&bytes, "empty").unwrap().is_empty());
        assert!(member_payload(&bytes, "folder").is_err());
        assert!(member_payload(&bytes, "missing").is_err());
        assert!(member_payload(&bytes[..bytes.len() - 1], "first").is_err());
        let end = bytes.len() - 1;
        bytes[end] = 1;
        assert!(member_payload(&bytes, "first").is_err());
    }

    #[test]
    fn malformed_stream_never_returns_layout() {
        let mut bytes = Vec::new();
        append(&mut bytes, "../escape", b"data");
        bytes.resize(bytes.len() + 2 * BLOCK, 0);
        assert!(layout_preview(bytes.as_slice()).is_err());
    }
}
