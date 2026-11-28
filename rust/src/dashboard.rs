//! Responsive application shell. All mutations still dispatch through Workspace.
#[cfg(feature = "native-ops")]
use std::io;
use std::{path::PathBuf, time::SystemTime};
use terminal_tree::{tree::TreeModel, Model};

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
mod navigation;
mod preferences;
mod preview_controller;
mod render;
mod sidebar;
mod tabs;
mod view;

use geometry::Divider;
use markdown::PreviewStyle;
use tabs::Tab;
use view::View;

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
}

#[cfg(test)]
mod tests;
