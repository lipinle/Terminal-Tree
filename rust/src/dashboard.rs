//! Responsive application shell. All mutations still dispatch through Workspace.
use crossterm::{
    cursor::MoveTo,
    event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::{
    io::{self, Write},
    path::PathBuf,
    time::SystemTime,
};
use terminal_tree::{display_name, Entry, Model};

const MENUS: [(&str, u16); 6] = [
    ("File", 15), ("Edit", 21), ("View", 27), ("Go", 33), ("Tools", 37), ("Settings", 44),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Layout {
    width: u16,
    height: u16,
    sidebar: u16,
    preview: u16,
}

impl Layout {
    fn new(width: u16, height: u16, preview: bool) -> Self {
        let width = width.saturating_sub(1);
        let sidebar = if width >= 54 { 19.min(width / 4) } else { 0 };
        let preview = if preview && width >= 92 { width / 3 } else { 0 };
        Self { width, height, sidebar, preview }
    }

    fn first(&self) -> u16 { if self.sidebar > 0 { self.sidebar + 1 } else { 0 } }
    fn end(&self) -> u16 {
        if self.preview > 0 { self.width - self.preview - 1 } else { self.width }
    }
    fn capacity(&self) -> usize { usize::from(self.height.saturating_sub(7)).max(1) }
    fn file_row(&self, column: u16, row: u16) -> Option<u16> {
        if self.height >= 8 && row >= 4 && row < self.height - 3
            && column >= self.first() && column < self.end()
        {
            Some(row - 3)
        } else { None }
    }
}

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
    preview_key: Option<PreviewKey>,
    preview_lines: Vec<String>,
    dual: bool,
    active_right: bool,
    other: Option<Model>,
    pane_switched: bool,
}

impl State {
    pub fn new(trash: Option<PathBuf>) -> Self {
        let mut state = Self {
            places: Vec::new(), bookmarks: Vec::new(), trash,
            side_focus: false, side_cursor: 0, side_top: 0, preview_enabled: true,
            preview_key: None, preview_lines: Vec::new(),
            dual: false, active_right: false, other: None, pane_switched: false,
        };
        state.refresh_places();
        state
    }

    fn refresh_places(&mut self) {
        let mut places = vec![("PLACES".into(), None)];
        if let Some(home) = super::platform::home_dir() {
            places.push(("Home".into(), Some(home.clone())));
            for name in ["Desktop", "Documents", "Downloads", "Pictures", "Music", "Videos"] {
                let p = home.join(name);
                if p.is_dir() { places.push((name.into(), Some(p))); }
            }
            let trash = self.trash.clone().unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("Trash/files")
            });
            if trash.is_dir() { places.push(("Trash".into(), Some(trash))); }
        }
        places.push(("DEVICES".into(), None));
        if let Ok(root) = super::platform::filesystem_root() {
            places.push(("File System".into(), Some(root)));
        }
        #[cfg(target_os = "linux")]
        if let Ok(paths) = std::fs::read("/proc/self/mountinfo")
            .and_then(|bytes| super::places::media_mounts(&bytes))
        {
            for p in paths.into_iter().take(128) {
                let label = p.file_name().map(display_name)
                    .unwrap_or_else(|| display_name(p.as_os_str()));
                places.push((label, Some(p)));
            }
        }
        places.push(("BOOKMARKS".into(), None));
        for p in &self.bookmarks {
            places.push((p.file_name().map(display_name)
                .unwrap_or_else(|| display_name(p.as_os_str())), Some(p.clone())));
        }
        self.places = places;
        self.side_cursor = self.side_cursor.min(self.places.len().saturating_sub(1));
        if self.places[self.side_cursor].1.is_none() {
            self.side_cursor = self.places.iter().position(|p| p.1.is_some()).unwrap_or(0);
        }
    }

    fn move_side(&mut self, forward: bool) {
        let next = if forward {
            (self.side_cursor + 1..self.places.len()).find(|&i| self.places[i].1.is_some())
        } else {
            (0..self.side_cursor).rev().find(|&i| self.places[i].1.is_some())
        };
        if let Some(i) = next { self.side_cursor = i; }
    }

    fn open_side(&mut self, model: &mut Model) -> io::Result<()> {
        if let Some(path) = self.places.get(self.side_cursor).and_then(|p| p.1.clone()) {
            model.go(path)?;
            self.side_focus = false;
            self.preview_key = None;
        }
        Ok(())
    }

    fn preview(&mut self, entry: Option<&Entry>) {
        let key = entry.map(|e| (e.path.clone(), e.size, e.modified));
        if key == self.preview_key && !self.preview_lines.is_empty() { return; }
        self.preview_key = key;
        self.preview_lines = match entry {
            None => vec!["No selection".into()],
            Some(e) if e.directory => vec![e.name.clone(), "Folder".into(), "Enter to open".into()],
            Some(e) if e.symlink => vec![e.name.clone(), "Symbolic link".into(), "Preview not followed".into()],
            Some(e) => {
                #[cfg(feature = "native-ops")]
                {
                    match terminal_tree::preview::read_lines(&e.path, terminal_tree::preview::Mode::Auto) {
                        Ok(mut lines) => {
                            if !lines.is_empty() { lines[0] = e.name.clone(); }
                            lines
                        }
                        Err(error) => vec![e.name.clone(), display_name(std::ffi::OsStr::new(&error.to_string()))],
                    }
                }
                #[cfg(not(feature = "native-ops"))]
                vec![e.name.clone(), format!("{} bytes", e.size), "Content preview requires native Linux".into()]
            }
        };
    }

    fn toggle_dual(&mut self, model: &Model) -> io::Result<()> {
        if !self.dual && self.other.is_none() {
            let mut other = Model::new(model.directory.clone())?;
            other.set_view(model.hidden, model.order, model.kind_filter, String::new())?;
            self.other = Some(other);
        }
        self.dual = !self.dual;
        self.side_focus = false;
        self.preview_key = None;
        Ok(())
    }

    fn switch_pane(&mut self, model: &mut Model) -> io::Result<()> {
        let other = self.other.as_mut().ok_or_else(|| io::Error::other("second pane unavailable"))?;
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

    fn draw_dual(
        &mut self,
        model: &mut Model,
        message: &str,
        searching: bool,
        writable: bool,
        personal: bool,
        clipboard: usize,
        marked: &impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        let (w, h) = terminal::size()?;
        let width = w.saturating_sub(1);
        let mut out = io::stdout().lock();
        queue!(out, MoveTo(0, 0), Clear(ClearType::All),
            SetForegroundColor(Color::Cyan),
            Print(super::clip("Terminal Tree  File  Edit  View  Go  Tools  Settings", usize::from(width))),
            ResetColor)?;
        if h < 8 { return out.flush(); }
        let side = if self.active_right { "Right" } else { "Left" };
        let mode = if personal { "Personal" } else if writable { "Workspace" } else { "Read-only" };
        let header = format!("Dual Pane | Active: {side} | [{} / {mode}]", super::edition::NAME);
        queue!(out, MoveTo(0, 1), Print(super::clip(&header, usize::from(width))),
            MoveTo(0, 2), Print("─".repeat(usize::from(width))))?;
        let cap = usize::from(h - 7);
        if width >= 54 {
            let split = width / 2;
            let other = self.other.as_mut().ok_or_else(|| io::Error::other("second pane unavailable"))?;
            if self.active_right {
                draw_column(&mut out, other, "Left", false, 0, split, cap, marked)?;
                draw_column(&mut out, model, "Right", true, split + 1, width - split - 1, cap, marked)?;
            } else {
                draw_column(&mut out, model, "Left", true, 0, split, cap, marked)?;
                draw_column(&mut out, other, "Right", false, split + 1, width - split - 1, cap, marked)?;
            }
            for row in 3..h - 3 {
                queue!(out, MoveTo(split, row), Print("│"))?;
            }
        } else {
            draw_column(&mut out, model, side, true, 0, width, cap, marked)?;
        }
        let safety = if writable { "Scoped" } else { "Read-only" };
        let status = format!("Clipboard: {clipboard} files | Tasks: idle | Safety: {safety} | {} entries", model.entries.len());
        let detail = if searching || !model.query.is_empty() {
            format!("Filter: {} | {message}", display_name(std::ffi::OsStr::new(&model.query)))
        } else { message.to_owned() };
        queue!(out, MoveTo(0, h - 3), Print(super::clip(&status, usize::from(width))),
            MoveTo(0, h - 2), Print(super::clip(&detail, usize::from(width))),
            MoveTo(0, h - 1), Print(super::clip(if searching {
                "Filter: type text | Enter accept | Esc cancel"
            } else if writable {
                "F8 Single  Tab Switch  Ctrl+C/X/V Transfer  F4 Actions  F10 Menu"
            } else {
                "F8 Single  Tab Switch  F10 Menu  Ctrl+F Filter  q Exit"
            }, usize::from(width))))?;
        out.flush()
    }

    pub fn draw(
        &mut self, model: &mut Model, message: &str, searching: bool,
        writable: bool, personal: bool, clipboard: usize, marked: impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        if self.dual {
            return self.draw_dual(model, message, searching, writable, personal, clipboard, &marked);
        }
        let (w,h) = terminal::size()?;
        let l = Layout::new(w,h,self.preview_enabled);
        if l.sidebar == 0 { self.side_focus = false; }
        let mut out = io::stdout().lock();
        queue!(out, MoveTo(0,0), Clear(ClearType::All),
            SetForegroundColor(Color::Cyan), Print(super::clip("Terminal Tree  File  Edit  View  Go  Tools  Settings", usize::from(l.width))), ResetColor)?;
        if h < 8 { return out.flush(); }
        let mode = if personal { "Personal" } else if writable { "Workspace" } else { "Read-only" };
        let path = display_name(model.directory.as_os_str());
        let badge = format!("[{} / {mode}]", super::edition::NAME);
        let path_width = usize::from(l.width).saturating_sub(badge.len()+2);
        if path_width > 0 {
            queue!(out, MoveTo(0,1), Print(super::clip(&path,path_width)),
                MoveTo(l.width.saturating_sub(badge.len() as u16),1), Print(&badge))?;
        } else {
            queue!(out, MoveTo(0,1), Print(super::clip(&path,usize::from(l.width))))?;
        }
        queue!(out, MoveTo(0,2), Print("─".repeat(usize::from(l.width))))?;
        if l.sidebar > 0 {
            queue!(out, MoveTo(0,3), SetForegroundColor(Color::Cyan),
                Print(if self.side_focus { "Places *" } else { "Places" }), ResetColor)?;
        }
        queue!(out, MoveTo(l.first(),3), SetForegroundColor(Color::Cyan),
            Print(if self.side_focus { "Files" } else { "Files *" }), ResetColor)?;
        if l.preview > 0 {
            queue!(out, MoveTo(l.end()+1,3), SetForegroundColor(Color::Cyan), Print("Preview"), ResetColor)?;
            self.preview(model.entries.get(model.selected));
        }
        let cap = l.capacity();
        let (start,end) = model.viewport(cap);
        if self.side_cursor < self.side_top { self.side_top = self.side_cursor; }
        if self.side_cursor >= self.side_top + cap { self.side_top = self.side_cursor + 1 - cap; }
        self.side_top = self.side_top.min(self.places.len().saturating_sub(cap));
        for row in 0..cap {
            let y = row as u16 + 4;
            if l.sidebar > 0 {
                if let Some((label,path)) = self.places.get(self.side_top+row) {
                    queue!(out, MoveTo(0,y))?;
                    if path.is_none() { queue!(out, SetForegroundColor(Color::Cyan))?; }
                    else if self.side_focus && self.side_top+row == self.side_cursor {
                        queue!(out, SetBackgroundColor(Color::DarkGrey))?;
                    }
                    let text = if path.is_some() { format!("  {label}") } else { label.clone() };
                    queue!(out, Print(super::clip(&text,usize::from(l.sidebar))), ResetColor)?;
                }
                queue!(out, MoveTo(l.sidebar,y), Print("│"))?;
            }
            if start+row < end {
                let e = &model.entries[start+row];
                let (glyph,color) = super::icon(e.directory,e.symlink);
                let text = format!("{} {} {}", if marked(&e.path) { "[x]" } else { "   " },glyph,e.name);
                queue!(out, MoveTo(l.first(),y))?;
                if start+row == model.selected { queue!(out, SetBackgroundColor(Color::DarkGrey))?; }
                queue!(out, SetForegroundColor(color),
                    Print(super::clip(&text,usize::from(l.end()-l.first()))), ResetColor)?;
            }
            if l.preview > 0 {
                queue!(out, MoveTo(l.end(),y), Print("│"))?;
                if let Some(line) = self.preview_lines.get(row) {
                    queue!(out, MoveTo(l.end()+1,y), Print(super::clip(line,usize::from(l.preview))))?;
                }
            }
        }
        let safety = if writable { "Scoped" } else { "Read-only" };
        let status = format!("Clipboard: {clipboard} files | Tasks: idle | Safety: {safety} | {} entries",model.entries.len());
        queue!(out, MoveTo(0,h-3), Print(super::clip(&status,usize::from(l.width))),
            MoveTo(0,h-2), Print(super::clip(message,usize::from(l.width))),
            MoveTo(0,h-1), Print(super::clip(if searching {
                "Filter: type text | Enter accept | Esc cancel"
            } else if writable {
                "F1 Help  F2 Rename  F3 Preview  F4 Actions  F8 Dual  F10 Menu  Ctrl+C/X/V"
            } else {
                "F1 Help  F3 Preview  F8 Dual  F10 Menu  Tab Focus  Ctrl+F Filter  q Exit"
            },usize::from(l.width))))?;
        out.flush()
    }

    fn menu(&mut self, category: Option<usize>, writable: bool) -> io::Result<Option<Event>> {
        let category = match category {
            Some(i) => i,
            None => {
                let labels = MENUS.map(|(name,_)| name.to_owned());
                match super::choose("Menu", &labels)? { Some(i) => i, None => return Ok(None) }
            }
        };
        let plain = KeyModifiers::NONE;
        let mut commands = match category {
            0 => vec![("Open",KeyCode::Enter,plain),("Properties",KeyCode::Char('i'),plain),("Exit",KeyCode::Char('q'),plain)],
            1 if writable => vec![("Copy (Ctrl+C)",KeyCode::Char('c'),plain),("Cut (Ctrl+X)",KeyCode::Char('x'),plain),("Paste (Ctrl+V)",KeyCode::Char('v'),plain),("View clipboard",KeyCode::F(12),plain),("Rename",KeyCode::F(2),plain),("New folder",KeyCode::Char('n'),plain),("New file",KeyCode::Char('t'),plain),("Trash",KeyCode::Delete,plain),("Undo",KeyCode::Char('u'),plain)],
            1 => vec![("Select a scoped workspace to enable file operations",KeyCode::Null,plain)],
            2 => vec![("Toggle dual pane (F8)",KeyCode::F(8),plain),("Toggle live preview",KeyCode::F(9),plain),("Open preview",KeyCode::F(3),plain),("Hidden files",KeyCode::Char('h'),plain),("Sort",KeyCode::Char('s'),plain),("Refresh",KeyCode::F(5),plain),("Filter",KeyCode::Char('/'),plain)],
            3 => vec![("Parent",KeyCode::Left,plain),("Back",KeyCode::Char('['),plain),("Forward",KeyCode::Char(']'),plain),("Locations",KeyCode::Char('m'),plain),("Bookmark current directory",KeyCode::Char('b'),plain),("Bookmarks",KeyCode::Char('B'),plain)],
            4 => vec![("Companion tools",KeyCode::Char('p'),KeyModifiers::CONTROL),("Help",KeyCode::F(1),plain)],
            _ => vec![("Toggle live preview",KeyCode::F(9),plain),("Hidden files",KeyCode::Char('h'),plain),("Sort",KeyCode::Char('s'),plain)],
        };
        if category == 2 && cfg!(feature = "native-ops") {
            commands.push(("Hex preview (Shift+F8)", KeyCode::F(8), KeyModifiers::SHIFT));
        }
        if category == 3 && cfg!(feature = "native-ops") {
            commands.insert(0,("Go to path",KeyCode::Char('g'),plain));
        }
        if category == 4 && writable { commands.push(("File actions",KeyCode::F(6),plain)); commands.push(("Recovery",KeyCode::Char('j'),plain)); }
        if category == 5 && writable { commands.push(("Save view settings",KeyCode::Char('S'),plain)); }
        if !cfg!(feature = "native-ops") {
            commands.retain(|(_,key,mods)| *key != KeyCode::F(3) && *mods != KeyModifiers::CONTROL);
        }
        let labels: Vec<_> = commands.iter().map(|(label,_,_)| label.to_string()).collect();
        Ok(super::choose(MENUS[category].0,&labels)?.map(|i| {
            let (_,key,modifiers) = commands[i];
            Event::Key(KeyEvent::new(key,modifiers))
        }))
    }

    fn context_menu(&self, writable: bool) -> io::Result<Option<Event>> {
        let mut actions = vec![
            ("Open", KeyCode::Enter),
            ("Properties", KeyCode::Char('i')),
        ];
        if cfg!(feature = "native-ops") {
            actions.push(("Preview", KeyCode::F(3)));
        }
        if writable {
            actions.extend([
                ("Copy", KeyCode::Char('c')),
                ("Cut", KeyCode::Char('x')),
                ("Paste", KeyCode::Char('v')),
                ("Rename", KeyCode::F(2)),
                ("Select / deselect", KeyCode::Char(' ')),
                ("Move to Trash", KeyCode::Delete),
                ("More actions", KeyCode::F(6)),
            ]);
        }
        let labels: Vec<_> = actions.iter().map(|(label, _)| label.to_string()).collect();
        Ok(super::choose("File context menu", &labels)?.map(|i| {
            Event::Key(KeyEvent::new(actions[i].1, KeyModifiers::NONE))
        }))
    }

    /// Handle shell-only interactions; translate file-list coordinates back to
    /// the existing navigator so the original review and write engine is used.
    pub fn input(&mut self, mut event: Event, model: &mut Model, searching: bool, writable: bool) -> io::Result<Option<Event>> {
        let (w,h) = terminal::size()?;
        let l = Layout::new(w,h,self.preview_enabled);
        if searching { return Ok(Some(event)); }
        if let Event::Mouse(mouse) = &mut event {
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.row == 0 {
                if let Some((i,_)) = MENUS.iter().enumerate().find(|(_, (name,x))| mouse.column >= *x && mouse.column < *x + name.len() as u16) {
                    self.side_focus = false;
                    let Some(selected) = self.menu(Some(i),writable)? else { return Ok(None); };
                    return self.input(selected,model,false,writable);
                }
                return Ok(None);
            }
            if self.dual {
                let width = w.saturating_sub(1);
                if h < 8 || mouse.row < 3 || mouse.row >= h - 3 || mouse.column >= width {
                    return Ok(None);
                }
                if width >= 54 {
                    let split = width / 2;
                    if mouse.column == split { return Ok(None); }
                    let right = mouse.column > split;
                    if right != self.active_right {
                        if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left | MouseButton::Right)) {
                            // Focus first; never dispatch an action against rows that
                            // may change when the inactive pane is refreshed.
                            self.switch_pane(model)?;
                        }
                        return Ok(None);
                    }
                }
                if mouse.row == 3 { return Ok(None); }
                let index = model.top + usize::from(mouse.row - 4);
                match mouse.kind {
                    MouseEventKind::ScrollUp => model.shift(-1),
                    MouseEventKind::ScrollDown => model.shift(1),
                    MouseEventKind::Down(MouseButton::Left | MouseButton::Right) if index < model.entries.len() => {
                        model.selected = index;
                        if mouse.kind == MouseEventKind::Down(MouseButton::Right) && mouse.modifiers == KeyModifiers::NONE {
                            return self.context_menu(writable);
                        }
                        if writable && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                            && mouse.modifiers == KeyModifiers::CONTROL {
                            return Ok(Some(Event::Key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE))));
                        }
                    }
                    _ => (),
                }
                return Ok(None);
            }
            if l.sidebar > 0 && mouse.column < l.sidebar && mouse.row >= 4 && h >= 8 && mouse.row < h-3 {
                self.side_focus = true;
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.move_side(false),
                    MouseEventKind::ScrollDown => self.move_side(true),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let i = self.side_top + usize::from(mouse.row-4);
                        if self.places.get(i).is_some_and(|p| p.1.is_some()) {
                            self.side_cursor = i;
                            self.open_side(model)?;
                        }
                    }
                    _ => (),
                }
                return Ok(None);
            }
            if let Some(row) = l.file_row(mouse.column,mouse.row) {
                self.side_focus = false;
                mouse.row = row;
                mouse.column = 0;
                if mouse.kind == MouseEventKind::Down(MouseButton::Right) && mouse.modifiers == KeyModifiers::NONE {
                    let index = model.top + usize::from(row - 1);
                    if index >= model.entries.len() {
                        return Ok(None);
                    }
                    model.selected = index;
                    return self.context_menu(writable);
                }
                if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.modifiers == KeyModifiers::NONE {
                    mouse.kind = MouseEventKind::Down(MouseButton::Right);
                }
                return Ok(Some(event));
            }
            return Ok(None);
        }
        if let Event::Key(key) = event {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) { return Ok(None); }
            let code = key.code;
            // Modified keys must never accidentally dispatch a plain destructive shortcut.
            if key.modifiers.contains(KeyModifiers::CONTROL) {
                let mapped = match code {
                    KeyCode::Char('q') => Some(KeyCode::Char('q')),
                    KeyCode::Char('c') if writable => Some(KeyCode::Char('c')),
                    KeyCode::Char('x') if writable => Some(KeyCode::Char('x')),
                    KeyCode::Char('v') if writable => Some(KeyCode::Char('v')),
                    KeyCode::Char('a') if writable => Some(KeyCode::Char('a')),
                    KeyCode::Char('z') if writable => Some(KeyCode::Char('u')),
                    KeyCode::Char('f') => Some(KeyCode::Char('/')),
                    KeyCode::Char('l') if cfg!(feature = "native-ops") => Some(KeyCode::Char('g')),
                    KeyCode::Char('c') | KeyCode::Char('p') => {
                        self.side_focus = false;
                        return Ok(Some(Event::Key(key)));
                    }
                    _ => None,
                };
                if let Some(code) = mapped {
                    self.side_focus = false;
                    return Ok(Some(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
                }
                return Ok(None);
            }
            if key.modifiers.contains(KeyModifiers::ALT) {
                let mapped = match code {
                    KeyCode::Left => Some(KeyCode::Char('[')),
                    KeyCode::Right => Some(KeyCode::Char(']')),
                    _ => None,
                };
                self.side_focus = false;
                return Ok(mapped.map(|code| Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
            }
            if code == KeyCode::F(10) {
                self.side_focus = false;
                let Some(selected) = self.menu(None,writable)? else { return Ok(None); };
                return self.input(selected,model,false,writable);
            }
            if code == KeyCode::F(8) && key.modifiers == KeyModifiers::NONE {
                if key.kind == KeyEventKind::Press { self.toggle_dual(model)?; }
                return Ok(None);
            }
            if code == KeyCode::F(8) && key.modifiers == KeyModifiers::SHIFT {
                self.side_focus = false;
                if cfg!(feature = "native-ops") {
                    return Ok(Some(Event::Key(KeyEvent::new(KeyCode::F(8), KeyModifiers::NONE))));
                }
                return Ok(None);
            }
            if code == KeyCode::Tab || code == KeyCode::BackTab {
                if self.dual {
                    if key.kind == KeyEventKind::Press { self.switch_pane(model)?; }
                } else {
                    self.side_focus = l.sidebar > 0 && !self.side_focus;
                }
                return Ok(None);
            }
            if code == KeyCode::F(9) {
                if self.dual { self.dual = false; self.preview_enabled = true; }
                else { self.preview_enabled = !self.preview_enabled; }
                self.preview_key = None;
                return Ok(None);
            }
            if code == KeyCode::F(5) { self.preview_key = None; self.refresh_places(); }
            if self.side_focus {
                match code {
                    KeyCode::Up => self.move_side(false),
                    KeyCode::Down => self.move_side(true),
                    KeyCode::Enter | KeyCode::Right => self.open_side(model)?,
                    KeyCode::Esc | KeyCode::Left => self.side_focus = false,
                    KeyCode::Char('q') => return Ok(Some(Event::Key(key))),
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return Ok(Some(Event::Key(key))),
                    _ => (),
                }
                return Ok(None);
            }
            if key.modifiers == KeyModifiers::NONE && code == KeyCode::Char('b') {
                if !self.bookmarks.contains(&model.directory) && self.bookmarks.len() < 64 {
                    self.bookmarks.push(model.directory.clone());
                    self.refresh_places();
                }
                return Ok(None);
            }
            if code == KeyCode::Char('B') {
                let labels: Vec<_> = self.bookmarks.iter().map(|p| display_name(p.as_os_str())).collect();
                if let Some(i) = super::choose("Session bookmarks",&labels)? { model.go(self.bookmarks[i].clone())?; }
                return Ok(None);
            }
            let replacement = match code {
                KeyCode::F(4) if writable => Some(KeyCode::F(6)),
                KeyCode::Backspace => Some(KeyCode::Left),
                KeyCode::Null => return Ok(None),
                _ => None,
            };
            if let Some(code) = replacement { return Ok(Some(Event::Key(KeyEvent::new(code,KeyModifiers::NONE)))); }
            return Ok(Some(Event::Key(key)));
        }
        Ok(Some(event))
    }
}

fn draw_column(
    out: &mut impl Write,
    model: &mut Model,
    label: &str,
    active: bool,
    x: u16,
    width: u16,
    cap: usize,
    marked: &impl Fn(&std::path::Path) -> bool,
) -> io::Result<()> {
    let path = display_name(model.directory.as_os_str());
    let header = format!("{label}{} | {path}", if active { " *" } else { "" });
    queue!(out, MoveTo(x, 3), SetForegroundColor(if active { Color::Cyan } else { Color::Grey }),
        Print(super::clip(&header, usize::from(width))), ResetColor)?;
    let (start, end) = model.viewport(cap);
    for (row, index) in (start..end).enumerate() {
        let entry = &model.entries[index];
        let (glyph, color) = super::icon(entry.directory, entry.symlink);
        let text = format!("{} {} {}", if active && marked(&entry.path) { "[x]" } else { "   " }, glyph, entry.name);
        queue!(out, MoveTo(x, row as u16 + 4))?;
        if active && index == model.selected {
            queue!(out, SetBackgroundColor(Color::DarkGrey))?;
        }
        queue!(out, SetForegroundColor(color), Print(super::clip(&text, usize::from(width))), ResetColor)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_is_responsive_and_only_file_rows_map_to_actions() {
        for width in 0..150 {
            for height in 0..30 {
                let l = Layout::new(width,height,true);
                assert!(l.first() <= l.end());
                assert!(l.end() <= l.width);
                assert!(l.file_row(0,0).is_none());
                if l.sidebar > 0 { assert!(l.file_row(0,4).is_none()); }
                if l.preview > 0 { assert!(l.file_row(l.end()+1,4).is_none()); }
                if height >= 8 && l.first() < l.end() {
                    assert_eq!(l.file_row(l.first(),4),Some(1));
                    assert!(l.file_row(l.first(),height-3).is_none());
                }
            }
        }
    }
}
