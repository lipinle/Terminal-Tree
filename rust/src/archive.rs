//! Strict, bounded USTAR listing only. No extraction or filesystem writes.
use std::collections::BTreeMap;
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
