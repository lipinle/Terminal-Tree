//! Application preferences, settings dialogs and navigation bindings.
use super::view::View;
use super::State;
use crossterm::{
    event::{Event, KeyCode, KeyModifiers},
    style::Color,
};
use std::io;
#[cfg(feature = "native-ops")]
use std::path::PathBuf;
#[cfg(feature = "native-ops")]
use terminal_tree::display_name;
use terminal_tree::Model;

const NAV_KEYS: [KeyCode; 9] = [
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::PageUp,
    KeyCode::PageDown,
    KeyCode::Home,
    KeyCode::End,
    KeyCode::Right,
    KeyCode::Left,
    KeyCode::F(5),
];
const NAV_LABELS: [&str; 9] = [
    "Up",
    "Down",
    "Page up",
    "Page down",
    "First",
    "Last",
    "Expand / right",
    "Collapse / left",
    "Refresh",
];

impl State {
    #[cfg(feature = "native-ops")]
    fn config_path() -> Option<PathBuf> {
        std::env::var_os("TT_APP_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                crate::platform::home_dir().map(|home| {
                    std::env::var_os("XDG_CONFIG_HOME")
                        .map(PathBuf::from)
                        .filter(|p| p.is_absolute())
                        .unwrap_or_else(|| home.join(".config"))
                        .join("terminal-tree")
                })
            })
    }

    #[cfg(feature = "native-ops")]
    pub fn load_preferences(&mut self, model: &mut Model) -> io::Result<()> {
        let Some(path) = Self::config_path() else {
            return Ok(());
        };
        let store = match crate::settings::Store::new(&path) {
            Ok(store) => store,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error),
        };
        if let Some(value) = store.load_app()? {
            let (prefs, icons, preview, view, zoom) =
                crate::app_settings::Preferences::decode(&value)?;
            let bookmarks = crate::app_settings::decode_bookmarks(&value)?;
            let hidden = value
                .get("hidden")
                .map(|v| {
                    v.as_bool()
                        .ok_or_else(|| io::Error::other("invalid hidden setting"))
                })
                .transpose()?
                .unwrap_or(model.hidden);
            let order = match value.get("order").and_then(|v| v.as_str()) {
                None => model.order,
                Some("Name") => terminal_tree::Order::Name,
                Some("Size") => terminal_tree::Order::Size,
                Some("Date") => terminal_tree::Order::Date,
                Some("Type") => terminal_tree::Order::Type,
                _ => return Err(io::Error::other("invalid sort setting")),
            };
            model.set_view(hidden, order, model.kind_filter, model.query.clone())?;
            self.prefs = prefs;
            if std::env::var_os("TT_ICON_STYLE").is_none() {
                self.icons = icons;
            }
            self.preview_enabled = preview;
            self.view = match view.as_str() {
                "Grid" => View::Grid,
                "List" => View::List,
                _ => View::Tree,
            };
            self.zoom = zoom;
            self.bookmarks = bookmarks;
            self.refresh_places();
            self.last_saved = Some(self.preference_value(model)?);
        }
        self.store = Some(store);
        Ok(())
    }

    #[cfg(feature = "native-ops")]
    fn preference_value(&self, model: &Model) -> io::Result<serde_json::Value> {
        let mut v = self.prefs.encode(
            self.icons,
            self.preview_enabled,
            self.view.name(),
            self.zoom,
        );
        v["bookmarks"] = crate::app_settings::encode_bookmarks(&self.bookmarks)?;
        v["hidden"] = serde_json::json!(model.hidden);
        v["order"] = serde_json::json!(format!("{:?}", model.order));
        Ok(v)
    }

    #[cfg(feature = "native-ops")]
    fn save_preferences(&mut self, model: &Model) -> io::Result<()> {
        if self.store.is_none() {
            let path = Self::config_path()
                .ok_or_else(|| io::Error::other("configuration location unavailable"))?;
            if !path.is_absolute() {
                return Err(io::Error::other("configuration directory must be absolute"));
            }
            let label = format!("Create or use {}", display_name(path.as_os_str()));
            if crate::choose(
                "Save application preferences in this private directory?",
                &["Cancel".into(), label],
            )? != Some(1)
            {
                return Ok(());
            }
            let store = match crate::settings::Store::new(&path) {
                Ok(store) => store,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    crate::settings::Store::create_app_directory(&path)?
                }
                Err(error) => return Err(error),
            };
            if store.load_app()?.is_some() {
                return Err(io::Error::other(
                    "existing preferences found; restart to load before saving",
                ));
            }
            self.store = Some(store);
        }
        let value = self.preference_value(model)?;
        if let Some(store) = &self.store {
            store.save_app(&value)?;
            self.last_saved = Some(value);
            crate::page(
                "Preferences saved",
                &["Application preferences saved.".into()],
            )?;
        }
        Ok(())
    }

    #[cfg(feature = "native-ops")]
    pub fn save_on_exit(&mut self, model: &Model) -> io::Result<()> {
        if !self.prefs.autosave {
            return Ok(());
        }
        if let Some(store) = &self.store {
            let value = self.preference_value(model)?;
            if self.last_saved.as_ref() != Some(&value) {
                store.save_app(&value)?;
                self.last_saved = Some(value);
            }
        }
        Ok(())
    }

    fn key_settings(&mut self) -> io::Result<()> {
        loop {
            let mut labels: Vec<_> = NAV_LABELS
                .iter()
                .enumerate()
                .map(|(i, name)| format!("{name}: {:?}", NAV_KEYS[self.prefs.navigation[i]]))
                .collect();
            labels.push("Reset navigation shortcuts".into());
            let Some(action) = crate::choose("Settings / Navigation shortcuts", &labels)? else {
                return Ok(());
            };
            if action == 9 {
                self.prefs.navigation = [0, 1, 2, 3, 4, 5, 6, 7, 8];
                continue;
            }
            let keys: Vec<_> = NAV_KEYS.iter().map(|key| format!("{key:?}")).collect();
            if let Some(key) =
                crate::choose("Select key (swaps existing navigation binding)", &keys)?
            {
                if let Some(other) = self.prefs.navigation.iter().position(|&n| n == key) {
                    self.prefs.navigation.swap(action, other);
                }
            }
        }
    }

    pub fn remap_input(&self, mut event: Event, searching: bool) -> Event {
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.focused {
            return event;
        }
        if !searching && self.location.is_none() {
            if let Event::Key(key) = &mut event {
                if key.modifiers == KeyModifiers::NONE {
                    if let Some(index) = NAV_KEYS.iter().position(|&code| code == key.code) {
                        if let Some(action) = self.prefs.navigation.iter().position(|&n| n == index)
                        {
                            key.code = NAV_KEYS[action];
                        }
                    }
                }
            }
        }
        event
    }

    fn appearance(&mut self) -> io::Result<()> {
        loop {
            let labels = [
                "Terminal defaults".into(),
                "Dark theme".into(),
                "Light theme".into(),
                "Custom text color (#RRGGBB)".into(),
                "Custom background color".into(),
                "Custom accent color".into(),
                "Custom selection color".into(),
                "Font family and size help".into(),
            ];
            match crate::choose("Settings / Appearance", &labels)? {
                None => return Ok(()),
                Some(0) => {
                    (self.prefs.foreground,self.prefs.background,self.prefs.accent,self.prefs.selection) =
                        (Color::Reset,Color::Reset,Color::Cyan,Color::DarkGrey);
                }
                Some(1) => {
                    self.prefs.foreground=Color::Rgb{r:230,g:230,b:230};
                    self.prefs.background=Color::Rgb{r:24,g:25,b:27};
                    self.prefs.accent=Color::Rgb{r:94,g:169,b:250};
                    self.prefs.selection=Color::Rgb{r:31,g:70,b:115};
                }
                Some(2) => {
                    self.prefs.foreground=Color::Rgb{r:25,g:25,b:25};
                    self.prefs.background=Color::Rgb{r:245,g:245,b:245};
                    self.prefs.accent=Color::Rgb{r:0,g:80,b:160};
                    self.prefs.selection=Color::Rgb{r:190,g:215,b:240};
                }
                Some(i @ 3..=6) => {
                    #[cfg(feature = "native-ops")]
                    if let Some(value)=crate::workspace::prompt("Color (#RRGGBB or default):")? {
                        match crate::app_settings::parse_color(&value) {
                            Ok(color)=>match i {3=>self.prefs.foreground=color,4=>self.prefs.background=color,5=>self.prefs.accent=color,_=>self.prefs.selection=color},
                            Err(error)=>crate::page("Invalid color",&[error.to_string()])?,
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    { let _ = i; crate::page("Appearance",&["Custom RGB entry requires native Linux".into()])?; }
                }
                _ => crate::page("Terminal font", &[
                    "Font family and actual point size are controlled by your terminal emulator.".into(),
                    "For Seti glyphs, select a compatible Nerd Font Mono in the terminal.".into(),
                    "Ctrl+mouse wheel changes the application layout density, not the font.".into(),
                ])?,
            }
        }
    }

    fn display_settings(&mut self, model: &mut Model) -> io::Result<()> {
        loop {
            let labels = [
                format!("Sidebar: {}", self.prefs.sidebar),
                format!("Live preview: {}", self.preview_enabled),
                format!("Location toolbar: {}", self.prefs.toolbar),
                format!("File icons: {}", self.prefs.show_icons),
                format!("File-type colors: {}", self.prefs.file_colors),
                format!("Extra status details: {}", self.prefs.status_details),
                format!("Hidden files: {}", model.hidden),
                format!("Layout zoom: {}", self.zoom),
                format!("File view: {}", self.view.name()),
                "Reset layout / hide preview and terminal".into(),
                format!("Terminal panel: {}", self.prefs.terminal_visible),
                format!("List type column: {}", self.prefs.list_details),
            ];
            match crate::choose("Settings / Display (Enter toggles)", &labels)? {
                None => return Ok(()),
                Some(0) => {
                    self.prefs.sidebar = !self.prefs.sidebar;
                    self.side_focus = false;
                }
                Some(1) => {
                    self.preview_enabled = !self.preview_enabled;
                    self.preview_key = None;
                }
                Some(2) => self.prefs.toolbar = !self.prefs.toolbar,
                Some(3) => self.prefs.show_icons = !self.prefs.show_icons,
                Some(4) => self.prefs.file_colors = !self.prefs.file_colors,
                Some(5) => self.prefs.status_details = !self.prefs.status_details,
                Some(6) => {
                    model.set_view(
                        !model.hidden,
                        model.order,
                        model.kind_filter,
                        model.query.clone(),
                    )?;
                    self.pane_switched = true;
                    self.expanded_tree = None;
                }
                Some(7) => self.zoom = self.zoom % 3 + 1,
                Some(8) => self.choose_view(match self.view {
                    View::Tree => View::Grid,
                    View::Grid => View::List,
                    View::List => View::Tree,
                }),
                Some(9) => self.reset_layout(),
                Some(10) => {
                    self.prefs.terminal_visible = !self.prefs.terminal_visible;
                    #[cfg(feature = "native-ops")]
                    {
                        self.terminal_panel.focused = false;
                    }
                }
                _ => self.prefs.list_details = !self.prefs.list_details,
            }
        }
    }

    fn icon_settings(&mut self) -> io::Result<()> {
        loop {
            let labels = [
                format!("Icon style: {}", self.icons.name()),
                format!("Show icons: {}", self.prefs.show_icons),
                format!(
                    "Custom extension mappings: {} (import JSON)",
                    self.prefs.custom.len()
                ),
                "Remove custom mappings".into(),
            ];
            match crate::choose("Settings / Icons", &labels)? {
                None => return Ok(()),
                Some(0) => self.choose_icons()?,
                Some(1) => self.prefs.show_icons = !self.prefs.show_icons,
                Some(2) => {
                    #[cfg(feature = "native-ops")]
                    if let Some(path) = crate::workspace::prompt("Absolute icon JSON path:")? {
                        let result = terminal_tree::read_icon_pack(std::path::Path::new(&path))
                            .and_then(|bytes| {
                                serde_json::from_slice(&bytes).map_err(io::Error::other)
                            })
                            .and_then(|v| crate::app_settings::parse_pack(&v));
                        match result {
                            Ok(map) => self.prefs.custom = map,
                            Err(error) => crate::page(
                                "Icon import refused",
                                &[terminal_tree::display_name(std::ffi::OsStr::new(
                                    &error.to_string(),
                                ))],
                            )?,
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    crate::page("Icon import", &["Native Linux required".into()])?;
                }
                _ => self.prefs.custom.clear(),
            }
        }
    }

    pub(super) fn integrations(&mut self) -> io::Result<()> {
        #[cfg(feature = "native-ops")]
        {
            crate::addons_ui::show(&mut self.prefs, self.last_tool.as_deref())?;
            self.preview_key = None;
            self.preview_lines.clear();
            self.visual_frame = None;
            self.frame_cache.invalidate();
            Ok(())
        }
        #[cfg(not(feature = "native-ops"))]
        crate::page(
            "Add-ons",
            &["Companion integrations require native Linux.".into()],
        )
    }

    pub(super) fn settings_center(&mut self, model: &mut Model) -> io::Result<()> {
        loop {
            let labels = [
                "Appearance / text and colors".into(),
                "Icons / style and import".into(),
                "Display / panels and view".into(),
                "Add-ons / integration status".into(),
                "Save application preferences".into(),
                format!("Autosave on normal exit: {}", self.prefs.autosave),
                "Manage bookmarks".into(),
                "Navigation shortcuts".into(),
                "Reset application preferences (session)".into(),
            ];
            match crate::choose("Settings Center", &labels)? {
                None => return Ok(()),
                Some(0) => self.appearance()?,
                Some(1) => self.icon_settings()?,
                Some(2) => self.display_settings(model)?,
                Some(3) => self.integrations()?,
                Some(4) => {
                    #[cfg(feature = "native-ops")]
                    self.save_preferences(model)?;
                    #[cfg(not(feature = "native-ops"))]
                    crate::page(
                        "Save preferences",
                        &["Session only on portable builds.".into()],
                    )?;
                }
                Some(5) => {
                    #[cfg(feature = "native-ops")]
                    {
                        if self.store.is_none() {
                            crate::page("Autosave",&["Save application preferences once to establish the private configuration directory.".into()])?;
                        } else {
                            self.prefs.autosave = !self.prefs.autosave;
                            // Persist the explicit opt-in/out now, including when turning autosave off.
                            self.save_preferences(model)?;
                        }
                    }
                    #[cfg(not(feature = "native-ops"))]
                    crate::page("Autosave", &["Session only on portable builds.".into()])?;
                }
                Some(6) => self.manage_bookmarks()?,
                Some(7) => self.key_settings()?,
                _ => {
                    let labels = [
                        "Cancel".into(),
                        "Reset all application display and companion options".into(),
                    ];
                    if crate::choose("Reset preferences?", &labels)? == Some(1) {
                        self.prefs = Default::default();
                        self.icons = Default::default();
                        self.preview_enabled = true;
                        self.zoom = 1;
                        self.choose_view(View::Tree);
                    }
                }
            }
        }
    }

    pub(super) fn choose_icons(&mut self) -> io::Result<()> {
        use crate::file_icons::Style;
        let labels = [
            "Unicode — standard terminal characters".into(),
            "Seti — requires a compatible Nerd Font".into(),
            "ASCII — maximum compatibility".into(),
        ];
        if let Some(index) = crate::choose("Icon style", &labels)? {
            self.icons = [Style::Unicode, Style::Seti, Style::Ascii][index];
        }
        Ok(())
    }
}
