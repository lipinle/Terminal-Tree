//! Native, bounded read-only previews. No path grants write authorization.
use crate::display_name;
use crate::readonly_file::ReadSession;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::io::{self, Read};
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

pub const MAX_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Auto,
    Hex,
}

fn hex_lines(bytes: &[u8]) -> Vec<String> {
    bytes
        .chunks(16)
        .enumerate()
        .map(|(index, chunk)| {
            let mut line = format!("{:08x}  ", index * 16);
            for column in 0..16 {
                if let Some(byte) = chunk.get(column) {
                    // Formatting into a String cannot fail.
                    let _ = write!(line, "{byte:02x} ");
                } else {
                    line.push_str("   ");
                }
            }
            line.push_str(" |");
            for &byte in chunk {
                line.push(if byte.is_ascii_graphic() || byte == b' ' {
                    char::from(byte)
                } else {
                    '.'
                });
            }
            line.push('|');
            line
        })
        .collect()
}

/// Read at most 64 KiB and one truncation sentinel from a regular non-link file.
/// Reject symlinked ancestors and recheck observed identity and metadata.
/// Not an atomic snapshot or a guarantee of bounded blocking-I/O time.
/// The returned text is escaped for terminal display, never used as a path.
pub fn read_lines(path: &Path, mode: Mode) -> io::Result<Vec<String>> {
    // Preserve relative-path input without canonicalizing away symlink components.
    // Keep the declared Rust 1.75 minimum; std::path::absolute is newer.
    if path.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty preview path",
        ));
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let session = ReadSession::open(&absolute)?;
    let mut bytes = Vec::new();
    session
        .file()
        .take((MAX_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    session.verify()?;
    let truncated = bytes.len() > MAX_BYTES;
    bytes.truncate(MAX_BYTES);
    let mut lines = vec![display_name(path.as_os_str())];
    if mode == Mode::Hex || bytes.contains(&0) {
        lines.push("Hex preview".into());
        lines.extend(hex_lines(&bytes));
    } else {
        for line in bytes.split(|&byte| byte == b'\n') {
            lines.push(display_name(OsStr::from_bytes(line)));
        }
    }
    if truncated {
        lines.push("[Preview limited to 64 KiB]".into());
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "tt-preview-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn binary_hex_is_bounded_and_terminal_safe() {
        let f = Fixture::new();
        let path = f.0.join("binary");
        let mut bytes = vec![0; MAX_BYTES + 1];
        bytes[1] = 0x1b;
        bytes[2] = b'A';
        bytes[16] = 0xff;
        fs::write(&path, &bytes).unwrap();
        let lines = read_lines(&path, Mode::Auto).unwrap();
        assert_eq!(lines[1], "Hex preview");
        assert!(lines[2].starts_with("00000000  00 1b 41 "));
        assert!(lines[3].starts_with("00000010  ff "));
        assert_eq!(lines.len(), 2 + MAX_BYTES / 16 + 1);
        assert_eq!(lines.last().unwrap(), "[Preview limited to 64 KiB]");
        assert!(!lines.iter().any(|line| line.contains('\x1b')));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[test]
    fn text_escapes_controls_and_raw_bytes() {
        let f = Fixture::new();
        let path = f.0.join("text");
        fs::write(&path, b"hello\x1b\xff\nworld").unwrap();
        let lines = read_lines(&path, Mode::Auto).unwrap();
        assert_eq!(lines[1], "hello\\u{1b}\\xff");
        assert_eq!(lines[2], "world");
    }

    #[test]
    fn explicit_hex_for_text_and_exact_limit() {
        let f = Fixture::new();
        let path = f.0.join("text");
        fs::write(&path, vec![b'A'; MAX_BYTES]).unwrap();
        let lines = read_lines(&path, Mode::Hex).unwrap();
        assert_eq!(lines[1], "Hex preview");
        assert!(lines[2].starts_with("00000000  41 41 "));
        assert_eq!(lines.len(), 2 + MAX_BYTES / 16);
    }

    #[test]
    fn links_and_directories_are_refused() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let path = f.0.join("file");
        fs::write(&path, b"keep").unwrap();
        let link = f.0.join("link");
        symlink(&path, &link).unwrap();
        assert!(read_lines(&link, Mode::Hex).is_err());
        assert!(read_lines(&f.0, Mode::Auto).is_err());
        assert_eq!(fs::read(path).unwrap(), b"keep");
    }

    #[test]
    fn rejects_symlinked_ancestor_and_parent_traversal() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        let real = f.0.join("real");
        fs::create_dir(&real).unwrap();
        let file = real.join("file");
        fs::write(&file, b"keep").unwrap();
        symlink(&real, f.0.join("alias")).unwrap();
        for mode in [Mode::Auto, Mode::Hex] {
            assert!(read_lines(&f.0.join("alias/file"), mode).is_err());
            assert!(read_lines(&real.join("../real/file"), mode).is_err());
            assert!(read_lines(&file, mode).is_ok());
        }
        assert_eq!(fs::read(&file).unwrap(), b"keep");
    }
}
