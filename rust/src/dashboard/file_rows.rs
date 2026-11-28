//! File row presentation, breadcrumbs and sortable column labels.
use super::view::View;
use std::path::PathBuf;
use terminal_tree::{display_name, Entry};

pub(super) fn breadcrumb(path: &std::path::Path) -> String {
    path.components()
        .map(|p| display_name(p.as_os_str()))
        .collect::<Vec<_>>()
        .join(" > ")
}

pub(super) fn breadcrumb_hit(path: &std::path::Path, column: u16, width: u16) -> Option<PathBuf> {
    use unicode_width::UnicodeWidthStr;
    let mut prefix = PathBuf::new();
    let mut x = 0usize;
    for part in path.components() {
        prefix.push(part.as_os_str());
        let label = display_name(part.as_os_str());
        let end = x + label.width();
        if end > usize::from(width) {
            break;
        }
        if (x..end).contains(&usize::from(column)) {
            return Some(prefix);
        }
        x = end + 3;
    }
    None
}
pub(super) fn list_sort_hit(column: u16, width: u16, details: bool) -> terminal_tree::Order {
    use terminal_tree::Order;
    if details && width >= 82 {
        if column >= width - 17 {
            Order::Date
        } else if column >= width - 30 {
            Order::Type
        } else if column >= width - 41 {
            Order::Size
        } else {
            Order::Name
        }
    } else if details && width >= 48 {
        if column >= width - 11 {
            Order::Type
        } else if column >= width - 22 {
            Order::Size
        } else {
            Order::Name
        }
    } else if width >= 30 && column >= width - 12 {
        Order::Size
    } else {
        Order::Name
    }
}
pub(super) fn list_heading(
    order: terminal_tree::Order,
    focused: bool,
    width: u16,
    details: bool,
) -> String {
    use terminal_tree::Order;
    let name = if focused {
        "Files * / Name"
    } else {
        "Files / Name"
    };
    table_columns(
        &format!("{name}{}", if order == Order::Name { " ↑" } else { "" }),
        if order == Order::Size {
            "Size ↑"
        } else {
            "Size"
        },
        if order == Order::Type {
            "Type ↑"
        } else {
            "Type"
        },
        if order == Order::Date {
            "Modified ↓ UTC"
        } else {
            "Modified (UTC)"
        },
        width,
        details,
    )
}
pub(super) fn human_size(size: u64) -> String {
    if size < 1024 {
        return format!("{size} B");
    }
    let mut n = size as f64;
    for unit in ["KiB", "MiB", "GiB", "TiB", "PiB", "EiB"] {
        n /= 1024.0;
        if n < 1024.0 || unit == "EiB" {
            return format!("{n:.1} {unit}");
        }
    }
    unreachable!()
}
pub(super) fn entry_type(entry: &Entry) -> String {
    if entry.symlink {
        "Link".into()
    } else if entry.directory {
        "Folder".into()
    } else {
        entry
            .path
            .extension()
            .map(display_name)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "File".into())
    }
}
pub(super) fn pad_cell(text: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    let value = crate::clip(text, width);
    let pad = width.saturating_sub(value.width());
    format!("{value}{}", " ".repeat(pad))
}
pub(super) fn list_columns(
    name: &str,
    size: &str,
    kind: &str,
    width: u16,
    details: bool,
) -> String {
    let w = usize::from(width);
    if details && w >= 48 {
        format!(
            "{} {:>10} {}",
            pad_cell(name, w - 23),
            size,
            pad_cell(kind, 11)
        )
    } else if w >= 30 {
        format!("{} {:>12}", pad_cell(name, w - 13), size)
    } else {
        crate::clip(name, w)
    }
}
pub(super) fn table_columns(
    name: &str,
    size: &str,
    kind: &str,
    date: &str,
    width: u16,
    details: bool,
) -> String {
    let w = usize::from(width);
    if details && w >= 82 {
        format!(
            "{} {} {} {}",
            pad_cell(name, w - 42),
            pad_cell(size, 10),
            pad_cell(kind, 12),
            pad_cell(date, 17)
        )
    } else {
        list_columns(name, size, kind, width, details)
    }
}
pub(super) fn modified_utc(time: Option<std::time::SystemTime>) -> String {
    let Some(secs) = time
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
    else {
        return "—".into();
    };
    if secs > 253402300799 {
        return "—".into();
    }
    // Gregorian civil date from Unix day count (UTC), with no locale or
    // timezone dependency and no invocation of external programs.
    let z = (secs / 86400) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}",
        secs / 3600 % 24,
        secs / 60 % 60
    )
}
type RowMetadata = (Option<u64>, Option<std::time::SystemTime>);

#[derive(Default)]
pub(super) struct MetadataCache {
    entries: std::collections::HashMap<
        std::path::PathBuf,
        (std::time::Instant, RowMetadata),
    >,
}

impl MetadataCache {
    pub(super) fn get(&mut self, entry: &Entry) -> RowMetadata {
        if let Some((when, metadata)) = self.entries.get(&entry.path) {
            if when.elapsed() < std::time::Duration::from_secs(2) {
                return *metadata;
            }
        }
        // Only display metadata: never use this cache for operation authorization.
        let metadata = match std::fs::symlink_metadata(&entry.path) {
            Ok(m) => (Some(m.len()), m.modified().ok()),
            Err(_) => (None, None),
        };
        if self.entries.len() >= 512 {
            self.entries.clear();
        }
        self.entries
            .insert(entry.path.clone(), (std::time::Instant::now(), metadata));
        metadata
    }

    pub(super) fn clear(&mut self) {
        self.entries.clear();
    }
}

pub(super) struct RowPresentation {
    pub(super) view: View,
    pub(super) width: u16,
    pub(super) details: bool,
    pub(super) metadata: RowMetadata,
}

pub(super) fn row_text(
    entry: &Entry,
    index: usize,
    count: usize,
    glyph: &str,
    marked: bool,
    presentation: RowPresentation,
) -> String {
    let RowPresentation {
        view,
        width,
        details,
        metadata,
    } = presentation;
    let mark = if marked { "[x]" } else { "   " };
    let name = match view {
        View::Tree => format!(
            "{} {mark} {glyph} {}",
            if index + 1 == count {
                "└──"
            } else {
                "├──"
            },
            entry.name
        ),
        _ => format!("{mark} {glyph} {}", entry.name),
    };
    if view == View::List || (view == View::Tree && width >= 48) {
        let (size, time) = metadata;
        table_columns(
            &name,
            &if entry.directory {
                "—".into()
            } else {
                size.map(human_size).unwrap_or_else(|| "—".into())
            },
            &entry_type(entry),
            &modified_utc(time),
            width,
            details,
        )
    } else {
        name
    }
}
