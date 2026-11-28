//! Dashboard layout and presentation regression tests.
use super::file_rows::{
    self, breadcrumb_hit, human_size, list_sort_hit, modified_utc, table_columns,
};
use super::geometry::Layout;
#[cfg(feature = "native-ops")]
use super::markdown::{self, PreviewStyle};
use super::view::{preview_hit_row, toolbar_hit, toolbar_start, Cells, View};
use std::path::PathBuf;

#[test]
fn breadcrumb_and_sort_targets() {
    use std::path::Path;
    use terminal_tree::Order;
    let p = Path::new("/home/日本語/docs");
    assert_eq!(breadcrumb_hit(p, 0, 80), Some(PathBuf::from("/")));
    assert_eq!(breadcrumb_hit(p, 4, 80), Some(PathBuf::from("/home")));
    assert_eq!(breadcrumb_hit(p, 9, 80), None);
    assert_eq!(
        breadcrumb_hit(p, 11, 80),
        Some(PathBuf::from("/home/日本語"))
    );
    assert_eq!(breadcrumb_hit(p, 11, 12), None);
    assert_eq!(list_sort_hit(40, 60, true), Order::Size);
    assert_eq!(list_sort_hit(55, 60, true), Order::Type);
    assert_eq!(list_sort_hit(32, 40, false), Order::Size);
    assert_eq!(list_sort_hit(84, 100, true), Order::Date);
    assert_eq!(list_sort_hit(72, 100, true), Order::Type);
    assert_eq!(list_sort_hit(60, 100, true), Order::Size);
    assert_eq!(
        modified_utc(Some(std::time::UNIX_EPOCH)),
        "1970-01-01 00:00"
    );
    assert_eq!(
        modified_utc(Some(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(951782400)
        )),
        "2000-02-29 00:00"
    );
}
#[cfg(feature = "native-ops")]
#[test]
fn markdown_formats_without_loading_content() {
    let lines = ["README.md", "# Heading", "- List", "> Quote", "<img src=x>"].map(str::to_owned);
    let (text, styles) = markdown::markdown_preview(&lines, 80);
    assert_eq!(text[1], "Heading");
    assert_eq!(styles[1], PreviewStyle::Heading);
    assert_eq!(text[2], "• List");
    assert_eq!(styles[3], PreviewStyle::Quote);
    assert_eq!(text[4], "<img src=x>");
}
#[test]
fn list_cells_fit_and_sizes_are_readable() {
    use unicode_width::UnicodeWidthStr;
    assert_eq!(human_size(1024), "1.0 KiB");
    assert_eq!(human_size(1048576), "1.0 MiB");
    for width in 0..150 {
        let text = file_rows::list_columns("ภาษาไทย long name.txt", "1.0 MiB", "txt", width, true);
        assert!(text.width() <= usize::from(width));
        assert!(
            table_columns(
                "ภาษาไทย name",
                "1.0 MiB",
                "Rust",
                "2026-11-28 12:00",
                width,
                true
            )
            .width()
                <= usize::from(width)
        );
    }
}
#[test]
fn short_preview_and_unbounded_hit_coordinates_are_rejected() {
    for height in 0..8 {
        assert!(!preview_hit_row(height, 3));
        assert!(!preview_hit_row(height, u16::MAX));
    }
    assert!(preview_hit_row(8, 4));
    assert!(!preview_hit_row(8, 5));
    let cells = Cells::new(120, 20, View::Grid, 1);
    assert_eq!(cells.hit(0, usize::MAX), None);
}

#[test]
fn view_cells_and_toolbar_hit_testing() {
    assert_eq!(View::default(), View::Tree);
    for width in 0..150 {
        for height in 1..30 {
            for view in [View::Tree, View::Grid, View::List] {
                for zoom in 1..=3 {
                    let c = Cells::new(width, height, view, zoom);
                    assert!(c.capacity() > 0);
                    assert!(c.groups * c.stride <= height);
                    for i in 0..c.capacity() {
                        let x = (i % c.columns) as u16 * c.width;
                        let y = (i / c.columns) * c.stride;
                        if c.width > 0 {
                            assert_eq!(c.hit(x, y), Some(i));
                        }
                    }
                }
            }
        }
        if let Some(start) = toolbar_start(width) {
            assert_eq!(toolbar_hit(width, start), Some(0));
            assert_eq!(toolbar_hit(width, start + 7), Some(1));
        }
    }
}

#[test]
fn layout_is_responsive_and_only_file_rows_map_to_actions() {
    for width in 0..150 {
        for height in 0..30 {
            let l = Layout::new(width, height, true);
            assert!(l.first() <= l.end());
            assert!(l.end() <= l.width);
            assert!(l.file_row(0, 0).is_none());
            if l.sidebar > 0 {
                assert!(l.file_row(0, 4).is_none());
            }
            if l.preview > 0 {
                assert!(l.file_row(l.end() + 1, 4).is_none());
            }
            if height >= 8 && l.first() < l.end() {
                assert_eq!(l.file_row(l.first(), 4), Some(1));
                assert!(l.file_row(l.first(), height - 3).is_none());
            }
        }
    }
}
