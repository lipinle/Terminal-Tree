//! Responsive application shell. All mutations still dispatch through Workspace.
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::{io, path::PathBuf, time::SystemTime};
use terminal_tree::{tree::TreeModel, KindFilter, Model};

pub struct DrawStatus<'a> {
    pub message: &'a str,
    pub searching: bool,
    pub writable: bool,
    pub personal: bool,
    pub clipboard: usize,
}

mod commands;
mod file_rows;
mod geometry;
mod input;
mod markdown;
mod menus;
mod preferences;
mod preview_controller;
mod render;
mod sidebar;
mod tabs;
mod view;

use geometry::{Divider, Layout};
use markdown::PreviewStyle;
use tabs::Tab;
use view::{Cells, View};

type Place = (String, Option<PathBuf>);
type PreviewKey = (PathBuf, u64, Option<SystemTime>);

pub struct State {
    places: Vec<Place>,
    bookmarks: Vec<PathBuf>,
    trash: Option<PathBuf>,
    side_focus: bool,
    side_cursor: usize,
    side_top: usize,
    preview_enabled: bool,
    frame_cache: super::dashboard_frame::FrameCache,
    preview_key: Option<PreviewKey>,
    preview_lines: Vec<String>,
    preview_scroll: usize,
    preview_styles: Vec<PreviewStyle>,
    preview_mode: u8,
    #[cfg(feature = "native-ops")]
    visual_frame: Option<terminal_tree::visual_preview::Frame>,
    #[cfg(feature = "native-ops")]
    preview_worker: super::preview_worker::Queue,
    #[cfg(feature = "native-ops")]
    visual_bitmap: bool,
    #[cfg(feature = "native-ops")]
    graphics_slot: super::native_graphics::Slot,
    #[cfg(feature = "native-ops")]
    graphics_placed: bool,
    #[cfg(feature = "native-ops")]
    graphics_focused: bool,
    #[cfg(feature = "native-ops")]
    graphics_failed: bool,
    #[cfg(feature = "native-ops")]
    visual_size: (u32, u32),
    #[cfg(feature = "native-ops")]
    visual_page: u32,
    dual: bool,
    active_right: bool,
    other: Option<Model>,
    pane_switched: bool,
    view: View,
    zoom: usize,
    location: Option<String>,
    expanded_tree: Option<TreeModel>,
    tabs: Vec<Option<Tab>>,
    active_tab: usize,
    dragging: Option<Divider>,
    #[cfg(feature = "native-ops")]
    terminal_panel: super::terminal_panel::Panel,
    icons: super::file_icons::Style,
    prefs: super::app_settings::Preferences,
    #[cfg(feature = "native-ops")]
    store: Option<super::settings::Store>,
    #[cfg(feature = "native-ops")]
    last_tool: Option<String>,
    #[cfg(feature = "native-ops")]
    last_saved: Option<serde_json::Value>,
    #[cfg(feature = "native-ops")]
    disk_cache: Option<(PathBuf, std::time::Instant, String)>,
}

impl State {
    pub fn new(trash: Option<PathBuf>) -> Self {
        let mut state = Self {
            places: Vec::new(),
            bookmarks: Vec::new(),
            trash,
            side_focus: false,
            side_cursor: 0,
            side_top: 0,
            preview_enabled: true,
            frame_cache: Default::default(),
            preview_key: None,
            preview_lines: Vec::new(),
            preview_scroll: 0,
            preview_styles: Vec::new(),
            preview_mode: 0,
            #[cfg(feature = "native-ops")]
            visual_frame: None,
            #[cfg(feature = "native-ops")]
            preview_worker: Default::default(),
            #[cfg(feature = "native-ops")]
            visual_bitmap: false,
            #[cfg(feature = "native-ops")]
            graphics_slot: super::native_graphics::Slot::new(1),
            #[cfg(feature = "native-ops")]
            graphics_placed: false,
            #[cfg(feature = "native-ops")]
            graphics_focused: true,
            #[cfg(feature = "native-ops")]
            graphics_failed: false,
            #[cfg(feature = "native-ops")]
            visual_size: (0, 0),
            #[cfg(feature = "native-ops")]
            visual_page: 1,
            dual: false,
            active_right: false,
            other: None,
            pane_switched: false,
            view: View::Tree,
            zoom: 1,
            location: None,
            expanded_tree: None,
            tabs: vec![None],
            active_tab: 0,
            dragging: None,
            #[cfg(feature = "native-ops")]
            terminal_panel: Default::default(),
            icons: super::file_icons::Style::from_environment(),
            prefs: Default::default(),
            #[cfg(feature = "native-ops")]
            store: None,
            #[cfg(feature = "native-ops")]
            last_tool: None,
            #[cfg(feature = "native-ops")]
            last_saved: None,
            #[cfg(feature = "native-ops")]
            disk_cache: None,
        };
        state.refresh_places();
        state
    }

    #[cfg(feature = "native-ops")]
    pub fn needs_poll(&self) -> bool {
        self.terminal_panel.running()
            || self.preview_worker.busy()
            || (super::x11_overlay::requested() && self.graphics_placed)
    }
    #[cfg(feature = "native-ops")]
    pub fn tick_terminal(&mut self) -> io::Result<bool> {
        let finished = self.terminal_panel.tick()?;
        if finished {
            self.expanded_tree = None;
            self.preview_key = None;
        }
        Ok(finished)
    }
    fn choose_view(&mut self, view: View) {
        self.view = view;
        self.dual = false;
        self.expanded_tree = None;
        self.side_focus = false;
    }

    fn start_location(&mut self, model: &Model) {
        self.expanded_tree = None;
        self.side_focus = false;
        self.location = Some(model.directory.to_str().unwrap_or("").to_owned());
    }

    fn open_expanded(&mut self, model: &mut Model) -> io::Result<()> {
        let Some(tree) = &self.expanded_tree else {
            return Ok(());
        };
        let Some(row) = tree.rows().get(tree.selected()) else {
            return Ok(());
        };
        let path = row.entry.path.clone();
        let directory = row.entry.directory && !row.entry.symlink;
        let target = if directory {
            path.clone()
        } else {
            path.parent()
                .ok_or_else(|| io::Error::other("missing parent"))?
                .to_path_buf()
        };
        // Descendant rows are read-only. Navigate into their containing folder,
        // reload ordinary rows and select the exact native path before any write.
        model.go(target)?;
        if !directory {
            if let Some(index) = model.entries.iter().position(|e| e.path == path) {
                model.selected = index;
            }
        }
        self.expanded_tree = None;
        self.preview_key = None;
        self.pane_switched = true;
        Ok(())
    }

    fn expand_current(&mut self, model: &Model) -> io::Result<()> {
        if !model.query.is_empty() || model.kind_filter != KindFilter::All {
            return Err(io::Error::other("clear filters before expanding tree"));
        }
        let Some(entry) = model.entries.get(model.selected) else {
            return Ok(());
        };
        if !entry.directory || entry.symlink {
            return Ok(());
        }
        let mut tree = TreeModel::new(&model.directory, model.hidden, model.order)?;
        let index = tree
            .rows()
            .iter()
            .position(|r| r.entry.path.file_name() == entry.path.file_name())
            .ok_or_else(|| io::Error::other("tree selection changed; refresh first"))?;
        tree.select(index);
        tree.expand()?;
        self.expanded_tree = Some(tree);
        Ok(())
    }

    fn expanded_input(&mut self, event: &Event, model: &mut Model, l: Layout) -> io::Result<bool> {
        let Some(tree) = self.expanded_tree.as_mut() else {
            return Ok(false);
        };
        let cells = Cells::new(l.end() - l.first(), l.capacity(), View::Tree, self.zoom);
        match event {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if k.modifiers == KeyModifiers::NONE {
                    match k.code {
                        KeyCode::Up => tree.shift(-1),
                        KeyCode::Down => tree.shift(1),
                        KeyCode::Home => tree.select(0),
                        KeyCode::End => tree.select(usize::MAX),
                        KeyCode::PageUp => tree.shift(-(cells.capacity() as isize)),
                        KeyCode::PageDown => tree.shift(cells.capacity() as isize),
                        KeyCode::Right => tree.expand()?,
                        KeyCode::Left => tree.collapse_or_parent(),
                        KeyCode::Enter => self.open_expanded(model)?,
                        KeyCode::Esc => { self.expanded_tree = None; },
                        KeyCode::F(5) => { tree.refresh()?; },
                        KeyCode::F(8) | KeyCode::F(9) | KeyCode::F(10) | KeyCode::Char('q') => {
                            self.expanded_tree = None;
                            return Ok(false);
                        }
                        _ => return Err(io::Error::other("Expanded tree: Enter opens folder or selects file in its parent; Esc returns")),
                    }
                } else if k.modifiers == KeyModifiers::CONTROL
                    && matches!(k.code, KeyCode::Char('l' | 'f' | 'q'))
                {
                    self.expanded_tree = None;
                    return Ok(false);
                }
            }
            Event::Mouse(mouse) if l.file_row(mouse.column, mouse.row).is_some() => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => tree.shift(-1),
                    MouseEventKind::ScrollDown => tree.shift(1),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let (start, end) = tree.viewport(cells.capacity());
                        if let Some(offset) =
                            cells.hit(mouse.column - l.first(), usize::from(mouse.row - 4))
                        {
                            let index = start + offset;
                            if index < end {
                                tree.select(index);
                                if mouse.column < l.first() + 4 {
                                    if tree.rows()[index].expanded {
                                        tree.collapse_or_parent();
                                    } else {
                                        tree.expand()?;
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.expanded_tree = None;
                return Ok(false);
            }
            _ => (),
        }
        Ok(true)
    }

    fn toggle_dual(&mut self, model: &Model) -> io::Result<()> {
        if !self.dual && self.other.is_none() {
            let mut other = Model::new(model.directory.clone())?;
            other.set_view(model.hidden, model.order, model.kind_filter, String::new())?;
            self.other = Some(other);
        }
        self.expanded_tree = None;
        self.dual = !self.dual;
        self.side_focus = false;
        self.preview_key = None;
        Ok(())
    }

    fn switch_pane(&mut self, model: &mut Model) -> io::Result<()> {
        let other = self
            .other
            .as_mut()
            .ok_or_else(|| io::Error::other("second pane unavailable"))?;
        // Refresh the incoming pane before making its rows actionable.
        // A failed refresh leaves the active model and selection unchanged.
        other.reload()?;
        std::mem::swap(model, other);
        self.active_right = !self.active_right;
        self.pane_switched = true;
        self.preview_key = None;
        Ok(())
    }

    pub fn take_pane_switch(&mut self) -> bool {
        std::mem::take(&mut self.pane_switched)
    }
}

#[cfg(test)]
mod tests {
    use super::file_rows::{breadcrumb_hit, human_size, list_sort_hit, modified_utc, table_columns};
    use super::view::{preview_hit_row, toolbar_hit, toolbar_start};
    use super::*;
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
        let lines =
            ["README.md", "# Heading", "- List", "> Quote", "<img src=x>"].map(str::to_owned);
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
            let text =
                file_rows::list_columns("ภาษาไทย long name.txt", "1.0 MiB", "txt", width, true);
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
}
