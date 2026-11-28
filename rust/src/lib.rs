#![forbid(unsafe_code)]
//! Native read-only filesystem model. No file mutation API is exposed.
#[cfg(all(feature = "native-ops", not(target_os = "linux")))]
compile_error!("native-ops requires the Linux safety backend; use the default read-only build on other platforms");
pub mod archive;
pub mod bookmarks;
pub mod image_preview;
pub mod keymap;
pub mod layout;
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod operations;
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod preview;
#[cfg(all(feature = "native-ops", target_os = "linux"))]
mod readonly_file;
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod recovery_advice;
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod rename_plan;
pub mod search;
pub mod theme;
pub mod tree;

use std::ffi::OsStr;
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Name,
    Size,
    Date,
    Type,
}
impl Order {
    pub fn next(self) -> Self {
        match self {
            Self::Name => Self::Size,
            Self::Size => Self::Date,
            Self::Date => Self::Type,
            Self::Type => Self::Name,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub directory: bool,
    pub symlink: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    sort_name: String,
    extension: String,
}

// The display text is never used as a filesystem path. Preserve raw path bytes.
// Escape controls, format characters, invalid UTF-8 and literal backslashes.
#[cfg(unix)]
pub fn display_name(name: &OsStr) -> String {
    let mut result = String::new();
    let mut bytes = name.as_bytes();
    while !bytes.is_empty() {
        match std::str::from_utf8(bytes) {
            Ok(s) => {
                append_safe(&mut result, s);
                break;
            }
            Err(err) => {
                let valid = err.valid_up_to();
                if valid != 0 {
                    if let Ok(s) = std::str::from_utf8(&bytes[..valid]) {
                        append_safe(&mut result, s);
                    }
                }
                let invalid = err.error_len().unwrap_or(bytes.len() - valid);
                for byte in &bytes[valid..valid + invalid] {
                    result.push_str(&format!("\\x{byte:02x}"));
                }
                bytes = &bytes[valid + invalid..];
            }
        }
    }
    result
}

#[cfg(windows)]
pub fn display_name(name: &OsStr) -> String {
    use std::os::windows::ffi::OsStrExt;
    let mut result = String::new();
    for unit in char::decode_utf16(name.encode_wide()) {
        match unit {
            Ok(c) => append_safe(&mut result, c.encode_utf8(&mut [0; 4])),
            Err(e) => result.push_str(&format!("\\u{{{:04x}}}", e.unpaired_surrogate())),
        }
    }
    result
}
fn append_safe(out: &mut String, s: &str) {
    for c in s.chars() {
        if c == '\\' {
            out.push_str("\\\\");
        } else if c.is_control()
            || matches!(c,
            '\u{00ad}' | '\u{061c}' | '\u{180e}' |
            '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' |
            '\u{2060}'..='\u{206f}' | '\u{feff}' | '\u{fff9}'..='\u{fffb}' |
            '\u{110bd}' | '\u{110cd}' | '\u{13430}'..='\u{13455}' |
            '\u{1bca0}'..='\u{1bca3}' | '\u{1d173}'..='\u{1d17a}' |
            '\u{e0001}' | '\u{e0020}'..='\u{e007f}')
        {
            out.push_str(&format!("\\u{{{:x}}}", c as u32));
        } else {
            out.push(c);
        }
    }
}

pub fn scan(path: &Path, hidden: bool, order: Order, query: &str) -> io::Result<Vec<Entry>> {
    scan_with_limit(path, hidden, order, query, None)
}

/// Read a directory with a bound on inspected entries, including filtered entries.
/// Reject an over-budget directory rather than returning a partial listing.
pub fn scan_bounded(
    path: &Path,
    hidden: bool,
    order: Order,
    query: &str,
    max_entries: usize,
) -> io::Result<Vec<Entry>> {
    scan_with_limit(path, hidden, order, query, Some(max_entries))
}

fn scan_with_limit(
    path: &Path,
    hidden: bool,
    order: Order,
    query: &str,
    max_entries: Option<usize>,
) -> io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    let query = query.to_lowercase();
    for (index, item) in fs::read_dir(path)?.enumerate() {
        let item = item?;
        if max_entries.is_some_and(|limit| index >= limit) {
            return Err(io::Error::other("directory scan entry limit exceeded"));
        }
        let raw = item.file_name();
        if !hidden && raw.as_encoded_bytes().starts_with(b".") {
            continue;
        }
        let name = display_name(&raw);
        let sort_name = raw.to_string_lossy().to_lowercase();
        if !sort_name.contains(&query) {
            continue;
        }
        let p = item.path();
        // For name/type sorting, avoid stat for regular directory entries.
        let ty = item.file_type()?;
        let symlink = ty.is_symlink();
        let directory = ty.is_dir() || (symlink && p.is_dir());
        let metadata = if matches!(order, Order::Size | Order::Date) {
            Some(fs::symlink_metadata(&p)?)
        } else {
            None
        };
        let size = metadata.as_ref().map_or(0, fs::Metadata::len);
        let modified = metadata.as_ref().and_then(|m| m.modified().ok());
        let extension = p
            .extension()
            .map_or_else(String::new, |s| s.to_string_lossy().to_lowercase());
        entries.push(Entry {
            path: p,
            name,
            directory,
            symlink,
            size,
            modified,
            sort_name,
            extension,
        });
    }
    entries.sort_unstable_by(|a, b| {
        b.directory
            .cmp(&a.directory)
            .then_with(|| match order {
                Order::Name => a.sort_name.cmp(&b.sort_name),
                Order::Size => b.size.cmp(&a.size),
                Order::Date => b.modified.cmp(&a.modified),
                Order::Type => a.extension.cmp(&b.extension),
            })
            .then_with(|| a.sort_name.cmp(&b.sort_name))
            .then_with(|| {
                a.path
                    .as_os_str()
                    .as_encoded_bytes()
                    .cmp(b.path.as_os_str().as_encoded_bytes())
            })
    });
    Ok(entries)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindFilter {
    All,
    Directories,
    Files,
    Links,
}
impl KindFilter {
    fn accepts(self, e: &Entry) -> bool {
        match self {
            Self::All => true,
            Self::Directories => e.directory && !e.symlink,
            Self::Files => !e.directory && !e.symlink,
            Self::Links => e.symlink,
        }
    }
}
#[derive(Debug)]
pub struct Model {
    pub directory: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub top: usize,
    pub hidden: bool,
    pub order: Order,
    pub query: String,
    pub kind_filter: KindFilter,
    history: Vec<PathBuf>,
    history_index: usize,
}

impl Model {
    pub fn new(directory: PathBuf) -> io::Result<Self> {
        let entries = scan(&directory, false, Order::Name, "")?;
        Ok(Self {
            directory: directory.clone(),
            entries,
            selected: 0,
            top: 0,
            hidden: false,
            order: Order::Name,
            query: String::new(),
            kind_filter: KindFilter::All,
            history: vec![directory],
            history_index: 0,
        })
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let selected_path = self.entries.get(self.selected).map(|e| e.path.clone());
        let mut entries = scan(&self.directory, self.hidden, self.order, &self.query)?;
        entries.retain(|e| self.kind_filter.accepts(e));
        self.entries = entries;
        self.selected = selected_path
            .and_then(|p| self.entries.iter().position(|e| e.path == p))
            .unwrap_or_else(|| self.selected.min(self.entries.len().saturating_sub(1)));
        self.top = self.top.min(self.selected);
        Ok(())
    }

    fn load_directory(&mut self, directory: PathBuf) -> io::Result<()> {
        let mut entries = scan(&directory, self.hidden, self.order, "")?;
        entries.retain(|e| self.kind_filter.accepts(e));
        self.directory = directory;
        self.entries = entries;
        self.query.clear();
        self.selected = 0;
        self.top = 0;
        Ok(())
    }

    pub fn go(&mut self, path: PathBuf) -> io::Result<()> {
        self.load_directory(path.clone())?;
        if self.history[self.history_index] != path {
            self.history.truncate(self.history_index + 1);
            self.history.push(path);
            self.history_index += 1;
        }
        Ok(())
    }

    pub fn history(&mut self, forward: bool) -> io::Result<()> {
        let next = if forward {
            self.history_index
                .checked_add(1)
                .filter(|i| *i < self.history.len())
        } else {
            self.history_index.checked_sub(1)
        };
        if let Some(index) = next {
            self.load_directory(self.history[index].clone())?;
            self.history_index = index;
        }
        Ok(())
    }

    pub fn enter(&mut self) -> io::Result<()> {
        if let Some(entry) = self.entries.get(self.selected) {
            if entry.directory {
                return self.go(entry.path.clone());
            }
        }
        Ok(())
    }

    pub fn parent(&mut self) -> io::Result<()> {
        if let Some(path) = self.directory.parent() {
            let previous = self.directory.clone();
            self.go(path.to_path_buf())?;
            self.selected = self
                .entries
                .iter()
                .position(|e| e.path == previous)
                .unwrap_or(0);
        }
        Ok(())
    }

    // No rescanning, rebuilding, cloning or sorting on cursor movement.
    pub fn shift(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.entries.len().saturating_sub(1));
    }

    pub fn viewport(&mut self, height: usize) -> (usize, usize) {
        let height = height.max(1);
        self.top = self.top.min(self.entries.len().saturating_sub(height));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected.saturating_sub(self.top) >= height {
            self.top = self.selected + 1 - height;
        }
        (
            self.top,
            self.top.saturating_add(height).min(self.entries.len()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    #[cfg(unix)]
    use std::os::unix::ffi::OsStringExt;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "tt-rust-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[test]
    fn bounded_scan_counts_hidden_and_filtered_entries() {
        let f = Fixture::new();
        fs::write(f.0.join("visible"), b"x").unwrap();
        fs::write(f.0.join(".hidden"), b"x").unwrap();
        assert!(scan_bounded(&f.0, false, Order::Name, "visible", 1).is_err());
        let rows = scan_bounded(&f.0, false, Order::Name, "visible", 2).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "visible");
        assert_eq!(scan(&f.0, true, Order::Name, "").unwrap().len(), 2);
    }

    #[test]
    fn bounded_scan_zero_budget_only_accepts_empty_directory() {
        let f = Fixture::new();
        assert!(scan_bounded(&f.0, true, Order::Name, "", 0)
            .unwrap()
            .is_empty());
        fs::write(f.0.join("entry"), b"x").unwrap();
        assert!(scan_bounded(&f.0, true, Order::Name, "", 0).is_err());
    }

    #[test]
    fn directories_first_hidden_filter_and_query() {
        let f = Fixture::new();
        fs::create_dir(f.0.join("Z-folder")).unwrap();
        fs::write(f.0.join("Apple"), b"a").unwrap();
        fs::write(f.0.join(".hidden"), b"h").unwrap();
        let e = scan(&f.0, false, Order::Name, "").unwrap();
        assert_eq!(
            e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
            ["Z-folder", "Apple"]
        );
        assert_eq!(scan(&f.0, true, Order::Name, "").unwrap().len(), 3);
        assert_eq!(scan(&f.0, false, Order::Name, "APPLE").unwrap().len(), 1);
    }

    #[cfg(unix)]
    #[test]
    fn preserves_invalid_path_bytes_and_escapes_control_text() {
        let f = Fixture::new();
        let raw = OsString::from_vec(vec![b'a', 0xff, 0x1b, b'\n']);
        let path = f.0.join(raw);
        fs::write(&path, b"x").unwrap();
        let e = scan(&f.0, false, Order::Name, "").unwrap();
        assert_eq!(e[0].path, path);
        assert!(e[0].name.contains("\\xff"));
        assert!(!e[0].name.contains('\x1b'));
        assert!(!e[0].name.contains('\n'));
        assert_ne!(
            display_name(OsStr::new("\\xFF")),
            display_name(OsStr::new("\u{00ff}"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn directory_symlink_and_dangling_link() {
        let f = Fixture::new();
        fs::create_dir(f.0.join("target")).unwrap();
        symlink("target", f.0.join("link")).unwrap();
        symlink("missing", f.0.join("dangling")).unwrap();
        let e = scan(&f.0, false, Order::Name, "").unwrap();
        assert!(e
            .iter()
            .any(|e| e.name == "link" && e.directory && e.symlink));
        assert!(e
            .iter()
            .any(|e| e.name == "dangling" && !e.directory && e.symlink));
    }

    #[cfg(windows)]
    #[test]
    fn windows_display_escapes_unpaired_surrogates() {
        use std::os::windows::ffi::OsStringExt;
        let name = OsString::from_wide(&[b'a' as u16, 0xd800, 0x1b]);
        let text = display_name(&name);
        assert_eq!(text, "a\\u{d800}\\u{1b}");
    }
    #[test]
    fn sort_size_descending_and_type() {
        let f = Fixture::new();
        fs::write(f.0.join("a.z"), b"x").unwrap();
        fs::write(f.0.join("b.a"), b"longer").unwrap();
        assert_eq!(scan(&f.0, false, Order::Size, "").unwrap()[0].name, "b.a");
        assert_eq!(scan(&f.0, false, Order::Type, "").unwrap()[0].name, "b.a");
    }

    #[test]
    fn cursor_movement_never_rescans_and_viewport_is_bounded() {
        let f = Fixture::new();
        for i in 0..100 {
            fs::write(f.0.join(format!("{i:03}")), b"x").unwrap();
        }
        let mut m = Model::new(f.0.clone()).unwrap();
        fs::write(f.0.join("new"), b"x").unwrap();
        for _ in 0..50 {
            m.shift(1);
        }
        assert_eq!(m.entries.len(), 100);
        assert_eq!(m.selected, 50);
        assert_eq!(m.viewport(10), (41, 51));
        m.reload().unwrap();
        assert_eq!(m.entries.len(), 101);
    }

    #[test]
    fn history_and_parent_keep_valid_selection() {
        let f = Fixture::new();
        let child = f.0.join("child");
        fs::create_dir(&child).unwrap();
        let mut m = Model::new(f.0.clone()).unwrap();
        m.enter().unwrap();
        assert_eq!(m.directory, child);
        m.history(false).unwrap();
        assert_eq!(m.directory, f.0);
        m.history(true).unwrap();
        assert_eq!(m.directory, child);
        m.parent().unwrap();
        assert_eq!(m.directory, f.0);
        assert_eq!(m.entries[m.selected].path, child);
    }

    #[test]
    fn failed_navigation_keeps_previous_state() {
        let f = Fixture::new();
        let mut m = Model::new(f.0.clone()).unwrap();
        assert!(m.go(f.0.join("missing")).is_err());
        assert_eq!(m.directory, f.0);
        m.shift(999);
        assert_eq!(m.viewport(10), (0, 0));
    }
}
