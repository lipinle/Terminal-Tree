//! Keyboard and mouse event dispatch for the dashboard shell.
use super::file_rows::{breadcrumb_hit, list_sort_hit};
use super::geometry::{dual_split, Divider, Layout};
use super::menus::MENUS;
use super::view::{preview_hit_row, toolbar_hit, toolbar_start, Cells, View};
use super::State;
use crossterm::{
    event::{
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
        MouseEventKind,
    },
    terminal,
};
use std::io;
use terminal_tree::{display_name, Model};

impl State {
    /// Handle shell-only interactions; translate file-list coordinates back to
    /// the existing navigator so the original review and write engine is used.
    pub fn input(
        &mut self,
        event: Event,
        model: &mut Model,
        searching: bool,
        writable: bool,
    ) -> io::Result<Option<Event>> {
        #[cfg(feature = "native-ops")]
        if crate::x11_overlay::requested() {
            match &event {
                Event::FocusLost => {
                    self.graphics_slot.clear()?;
                    self.graphics_placed = false;
                    self.graphics_focused = false;
                    return Ok(None);
                }
                Event::FocusGained => {
                    crate::x11_overlay::refresh();
                    self.graphics_focused = true;
                    self.graphics_placed = false;
                    self.graphics_failed = false;
                    self.preview_key = None;
                    return Ok(None);
                }
                Event::Resize(_, _) => {
                    crate::x11_overlay::refresh();
                    self.graphics_focused = true;
                    self.graphics_failed = false;
                    self.preview_key = None;
                }
                Event::Key(_) | Event::Mouse(_) => {
                    self.graphics_focused = true;
                }
                _ => (),
            }
        }
        // Modal actions can repaint the physical screen. Plain navigation and
        // passive mouse events cannot, so keep the row cache for those.
        let passive = matches!(&event,Event::Mouse(mouse) if matches!(mouse.kind,MouseEventKind::Moved|MouseEventKind::Up(_)));
        let arrow = matches!(&event,Event::Key(k) if k.modifiers==KeyModifiers::NONE && matches!(k.code,KeyCode::Up|KeyCode::Down));
        if !passive
            && !(arrow
                && !self.dual
                && !searching
                && self.location.is_none()
                && self.expanded_tree.is_none())
        {
            self.frame_cache.invalidate();
            #[cfg(feature = "native-ops")]
            {
                self.graphics_slot.clear()?;
                self.graphics_placed = false;
            }
        }
        let (w, full_h) = terminal::size()?;
        let h = full_h - self.panel_rows(full_h);
        let l = self.layout(w, h);
        if searching {
            return Ok(Some(event));
        }
        #[cfg(feature = "native-ops")]
        if self.handle_terminal_input(&event, model, w, full_h, l)? {
            return Ok(None);
        }
        if self.handle_location_input(&event, model, l)? {
            return Ok(None);
        }
        if let Event::Key(key) = &event {
            if self.handle_view_shortcut(*key) {
                return Ok(None);
            }
        }
        if let Event::Mouse(mouse) = &event {
            if !self.dual && mouse.modifiers.contains(KeyModifiers::CONTROL) {
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        self.zoom = (self.zoom + 1).min(3);
                        return Ok(None);
                    }
                    MouseEventKind::ScrollDown => {
                        self.zoom = self.zoom.saturating_sub(1).max(1);
                        return Ok(None);
                    }
                    _ => (),
                }
            }
            if !self.dual
                && mouse.row == 1
                && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.modifiers == KeyModifiers::NONE
            {
                let path_width = if self.prefs.toolbar {
                    toolbar_start(l.width).unwrap_or(l.width)
                } else {
                    l.width
                };
                if mouse.column < path_width {
                    let nav = if path_width.saturating_sub(1) >= 52 {
                        12
                    } else {
                        0
                    };
                    if mouse.column < nav {
                        let key = match mouse.column / 4 {
                            0 => KeyCode::Char('['),
                            1 => KeyCode::Char(']'),
                            _ => KeyCode::Left,
                        };
                        return Ok(Some(Event::Key(KeyEvent::new(key, KeyModifiers::NONE))));
                    }
                    if let Some(target) = breadcrumb_hit(
                        &model.directory,
                        mouse.column - nav,
                        path_width.saturating_sub(nav),
                    ) {
                        if target != model.directory {
                            model.go(target)?;
                            self.clear_tab_transients();
                        }
                    }
                    return Ok(None);
                }
                if !self.prefs.toolbar {
                    return Ok(None);
                }
                match toolbar_hit(l.width, mouse.column) {
                    Some(0) => self.start_location(model),
                    Some(1) => {
                        self.expanded_tree = None;
                        self.side_focus = false;
                        return Ok(Some(Event::Key(KeyEvent::new(
                            KeyCode::Char('/'),
                            KeyModifiers::NONE,
                        ))));
                    }
                    Some(2) => self.choose_view(View::Grid),
                    Some(3) => self.choose_view(View::List),
                    Some(4) => self.choose_view(View::Tree),
                    _ => (),
                }
                return Ok(None);
            }
        }
        if let Event::Key(k) = &event {
            if k.modifiers == KeyModifiers::CONTROL
                && matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat)
            {
                match k.code {
                    KeyCode::Char('l') if !self.dual => {
                        self.start_location(model);
                        return Ok(None);
                    }
                    KeyCode::Char('+' | '=') if !self.dual => {
                        self.zoom = (self.zoom + 1).min(3);
                        return Ok(None);
                    }
                    KeyCode::Char('-') if !self.dual => {
                        self.zoom = self.zoom.saturating_sub(1).max(1);
                        return Ok(None);
                    }
                    KeyCode::Char('0') if !self.dual => {
                        self.zoom = 1;
                        return Ok(None);
                    }
                    _ => (),
                }
            }
        }
        if let Event::Key(k) = &event {
            if k.code == KeyCode::F(11)
                && k.modifiers == KeyModifiers::NONE
                && k.kind == KeyEventKind::Press
            {
                self.choose_icons()?;
                return Ok(None);
            }
        }
        if let Event::Key(k) = &event {
            if k.kind == KeyEventKind::Press {
                if k.code == KeyCode::Char('k') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree = None;
                    self.side_focus = false;
                    let Some(selected) = self.palette(writable)? else {
                        return Ok(None);
                    };
                    return self.input(selected, model, false, writable);
                }
                if k.code == KeyCode::Char(',') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree = None;
                    self.settings_center(model)?;
                    return Ok(None);
                }
                if k.code == KeyCode::F(11) && k.modifiers == KeyModifiers::SHIFT {
                    self.integrations()?;
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('v') && k.modifiers == KeyModifiers::ALT {
                    if !self.prefs.visual_preview {
                        return Err(io::Error::other("Enable Visual Preview in Add-ons first"));
                    }
                    let entry = if let Some(tree) = &self.expanded_tree {
                        tree.rows().get(tree.selected()).map(|r| &r.entry)
                    } else {
                        model.entries.get(model.selected)
                    };
                    if let Some(path) = entry
                        .filter(|e| {
                            !e.directory
                                && !e.symlink
                                && terminal_tree::visual_preview::format(&e.path).is_some()
                        })
                        .map(|e| e.path.clone())
                    {
                        self.suspend_visual_preview();
                        if self.preview_worker.busy() {
                            return Err(io::Error::other("Preview finishing; press Alt+V again"));
                        }
                        crate::visual_ui::show(&path)?;
                    }
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('g') && k.modifiers == KeyModifiers::ALT {
                    self.expanded_tree = None;
                    self.side_focus = false;
                    crate::git_ui::show(&model.directory, self.prefs.companions[2])?;
                    model.reload()?;
                    self.pane_switched = true;
                    self.preview_key = None;
                    return Ok(None);
                }
                #[cfg(feature = "native-ops")]
                if k.code == KeyCode::Char('p') && k.modifiers == KeyModifiers::CONTROL {
                    self.expanded_tree = None;
                    crate::tools_ui::show_filtered(
                        &model.directory,
                        model.entries.get(model.selected).map(|e| e.path.as_path()),
                        self.prefs.companions,
                        &mut self.last_tool,
                    )?;
                    return Ok(None);
                }
            }
        }
        if let Event::Key(k) = &event {
            if k.kind == KeyEventKind::Press && k.modifiers.contains(KeyModifiers::CONTROL) {
                match k.code {
                    KeyCode::Char('t') if k.modifiers == KeyModifiers::CONTROL => {
                        self.new_tab(model)?;
                        return Ok(None);
                    }
                    KeyCode::Char('w') if k.modifiers == KeyModifiers::CONTROL => {
                        self.close_tab(model)?;
                        return Ok(None);
                    }
                    KeyCode::Tab | KeyCode::BackTab => {
                        let n = self.tabs.len();
                        let index = if k.modifiers.contains(KeyModifiers::SHIFT)
                            || k.code == KeyCode::BackTab
                        {
                            (self.active_tab + n - 1) % n
                        } else {
                            (self.active_tab + 1) % n
                        };
                        self.switch_tab(model, index)?;
                        return Ok(None);
                    }
                    _ => (),
                }
            }
        }
        if let Event::Mouse(mouse) = &event {
            if let Some(divider) = self.dragging {
                match mouse.kind {
                    MouseEventKind::Drag(MouseButton::Left) => {
                        self.resize_drag(divider, mouse.column, l.width, mouse.row, full_h)
                    }
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.resize_drag(divider, mouse.column, l.width, mouse.row, full_h);
                        self.dragging = None;
                    }
                    _ => self.dragging = None,
                }
                return Ok(None);
            }
            if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.modifiers == KeyModifiers::NONE
            {
                if mouse.row == 2 && l.width >= 8 {
                    let (first, cell, count) = self.tab_slots(l.width);
                    let slot = usize::from(mouse.column / cell);
                    if slot < count {
                        let index = first + slot;
                        if cell >= 12 && self.tabs.len() > 1 && mouse.column % cell >= cell - 3 {
                            if index == self.active_tab {
                                self.close_tab(model)?;
                            } else {
                                self.tabs.remove(index);
                                if index < self.active_tab {
                                    self.active_tab -= 1;
                                }
                            }
                        } else {
                            self.switch_tab(model, index)?;
                        }
                    } else if mouse.column >= count as u16 * cell
                        && mouse.column < count as u16 * cell + 3
                    {
                        self.new_tab(model)?;
                    }
                    return Ok(None);
                }
                if h >= 8 && mouse.row >= 3 && mouse.row < h - 3 {
                    self.dragging = if self.dual {
                        (l.width >= 54
                            && mouse.column == dual_split(l.width, self.prefs.dual_percent))
                        .then_some(Divider::Dual)
                    } else if l.sidebar > 0 && mouse.column == l.sidebar {
                        Some(Divider::Sidebar)
                    } else if l.preview > 0 && mouse.column == l.end() {
                        Some(Divider::Preview)
                    } else {
                        None
                    };
                    if self.dragging.is_some() {
                        return Ok(None);
                    }
                }
            }
        }
        if !self.dual
            && self.expanded_tree.is_none()
            && (self.view == View::List || (self.view == View::Tree && l.end() - l.first() >= 48))
        {
            if let Event::Mouse(mouse) = &event {
                if h >= 8
                    && mouse.row == 3
                    && (l.first()..l.end()).contains(&mouse.column)
                    && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                    && mouse.modifiers == KeyModifiers::NONE
                {
                    let order = list_sort_hit(
                        mouse.column - l.first(),
                        l.end() - l.first(),
                        self.prefs.list_details,
                    );
                    model.set_view(model.hidden, order, model.kind_filter, model.query.clone())?;
                    self.pane_switched = true;
                    self.side_focus = false;
                    self.expanded_tree = None;
                    self.preview_key = None;
                    return Ok(None);
                }
            }
        }
        if !self.dual && l.preview > 0 {
            if let Event::Key(k) = &event {
                if k.kind == KeyEventKind::Press
                    && k.modifiers == KeyModifiers::ALT
                    && k.code == KeyCode::Char('p')
                {
                    self.preview_mode = (self.preview_mode + 1) % 3;
                    self.preview_key = None;
                    self.preview_scroll = 0;
                    return Ok(None);
                }
            }
            if let Event::Mouse(mouse) = &event {
                if mouse.column > l.end() && mouse.column < l.width && preview_hit_row(h, mouse.row)
                {
                    if mouse.row == 3
                        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.modifiers == KeyModifiers::NONE
                    {
                        let pos = mouse.column.saturating_sub(l.end() + 1);
                        if l.preview >= 46 && pos >= 37 {
                            #[cfg(feature = "native-ops")]
                            return self.input(
                                Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::ALT)),
                                model,
                                searching,
                                writable,
                            );
                        }
                        self.preview_mode = if l.preview >= 46 && (16..37).contains(&pos) {
                            ((pos - 16) / 7).min(2) as u8
                        } else {
                            (self.preview_mode + 1) % 3
                        };
                        self.preview_key = None;
                        self.preview_scroll = 0;
                        return Ok(None);
                    }
                    #[cfg(feature = "native-ops")]
                    if let Some(frame) = &self.visual_frame {
                        let page = match mouse.kind {
                            MouseEventKind::ScrollDown => {
                                self.visual_page.saturating_add(1).min(frame.pages)
                            }
                            MouseEventKind::ScrollUp => self.visual_page.saturating_sub(1).max(1),
                            _ => self.visual_page,
                        };
                        if page != self.visual_page {
                            self.visual_page = page;
                            self.preview_lines.clear();
                        }
                        return Ok(None);
                    }
                    match mouse.kind {
                        MouseEventKind::ScrollDown => {
                            self.preview_scroll = (self.preview_scroll + 3)
                                .min(self.preview_lines.len().saturating_sub(l.capacity()))
                        }
                        MouseEventKind::ScrollUp => {
                            self.preview_scroll = self.preview_scroll.saturating_sub(3)
                        }
                        _ => (),
                    }
                    return Ok(None);
                }
            }
        }
        if self.expanded_input(&event, model, l)? {
            return Ok(None);
        }
        match event {
            Event::Mouse(mouse) => self.handle_mouse_input(mouse, model, full_h, l, writable),
            Event::Key(key) => self.handle_key_input(key, model, l, writable),
            other => Ok(Some(other)),
        }
    }

    fn handle_view_shortcut(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press
            || (key.modifiers != KeyModifiers::NONE && key.modifiers != KeyModifiers::CONTROL)
        {
            return false;
        }
        match key.code {
            KeyCode::Char('1') => self.choose_view(View::Grid),
            KeyCode::Char('2') => self.choose_view(View::List),
            KeyCode::Char('3') => self.choose_view(View::Tree),
            KeyCode::Char('0') if key.modifiers == KeyModifiers::NONE => self.reset_layout(),
            _ => return false,
        }
        true
    }

    #[cfg(feature = "native-ops")]
    fn handle_terminal_input(
        &mut self,
        event: &Event,
        model: &mut Model,
        w: u16,
        full_h: u16,
        l: Layout,
    ) -> io::Result<bool> {
        let (terminal_left, terminal_top, terminal_width, terminal_rows) =
            self.terminal_rect(full_h, l);
        if self.panel_rows(full_h) == 0 {
            self.terminal_panel.focused = false;
        }
        if let Event::Mouse(mouse) = event {
            if self.location.is_none()
                && mouse.row == 0
                && l.width >= 83
                && (78..83).contains(&mouse.column)
                && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.modifiers == KeyModifiers::NONE
            {
                self.terminal_panel.focused = false;
                self.expanded_tree = None;
                self.side_focus = false;
                crate::git_ui::show(&model.directory, self.prefs.companions[2])?;
                model.reload()?;
                self.pane_switched = true;
                self.preview_key = None;
                return Ok(true);
            }
            if matches!(self.dragging, Some(Divider::Terminal)) {
                match mouse.kind {
                    MouseEventKind::Drag(MouseButton::Left) => self.resize_drag(
                        Divider::Terminal,
                        mouse.column,
                        l.width,
                        mouse.row,
                        full_h,
                    ),
                    MouseEventKind::Up(MouseButton::Left) => {
                        self.resize_drag(
                            Divider::Terminal,
                            mouse.column,
                            l.width,
                            mouse.row,
                            full_h,
                        );
                        self.dragging = None;
                    }
                    _ => self.dragging = None,
                }
                return Ok(true);
            }
            if self.location.is_none()
                && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                && mouse.modifiers == KeyModifiers::NONE
            {
                if mouse.row == 0 && l.width >= 75 && mouse.column >= 63 && mouse.column < 75 {
                    self.prefs.terminal_visible = !self.prefs.terminal_visible;
                    self.terminal_panel.focused = false;
                    return Ok(true);
                }
                if terminal_rows > 0
                    && mouse.row == terminal_top
                    && terminal_width >= 48
                    && mouse.column >= l.width - 8
                    && mouse.column < l.width
                {
                    self.dragging = Some(Divider::Terminal);
                    return Ok(true);
                }
            }
        }
        if self.location.is_none() {
            if let Event::Key(k) = event {
                if k.kind == KeyEventKind::Press && k.modifiers == KeyModifiers::ALT {
                    match k.code {
                        KeyCode::Char('s') => {
                            if self.terminal_panel.interactive() {
                                if crate::choose(
                                    "Close interactive shell? Running jobs may be interrupted",
                                    &["Cancel".into(), "Close shell".into()],
                                )? == Some(1)
                                {
                                    self.terminal_panel.close_shell();
                                    model.reload()?;
                                    self.pane_switched = true;
                                    self.preview_key = None;
                                }
                            } else {
                                if full_h < 16 {
                                    return Err(io::Error::other(
                                        "terminal panel needs at least 16 rows",
                                    ));
                                }
                                if crate::choose(
                                    "Interactive shell uses OS permissions, outside file journal",
                                    &["Cancel".into(), "Start Bash PTY".into()],
                                )? == Some(1)
                                {
                                    self.prefs.terminal_visible = true;
                                    let (_, _, width, rows) = self.terminal_rect(
                                        full_h,
                                        self.layout(w, full_h - self.panel_rows(full_h)),
                                    );
                                    self.terminal_panel.start_shell(
                                        &model.directory,
                                        width,
                                        rows.saturating_sub(3),
                                    )?;
                                    self.side_focus = false;
                                }
                            }
                            return Ok(true);
                        }
                        KeyCode::Char('h') => {
                            self.prefs.terminal_visible = !self.prefs.terminal_visible;
                            self.terminal_panel.focused = false;
                            return Ok(true);
                        }
                        KeyCode::Char('t') => {
                            self.prefs.terminal_visible = true;
                            if self.panel_rows(full_h) == 0 {
                                return Err(io::Error::other(
                                    "terminal panel needs at least 16 rows",
                                ));
                            }
                            self.terminal_panel.focused = !self.terminal_panel.focused;
                            self.side_focus = false;
                            return Ok(true);
                        }
                        _ => (),
                    }
                }
                if self.terminal_panel.focused {
                    self.terminal_panel.input(*k, &model.directory)?;
                    return Ok(true);
                }
            }
            if let Event::Mouse(mouse) = event {
                if terminal_rows > 0
                    && mouse.column >= terminal_left
                    && mouse.column < terminal_left + terminal_width
                    && mouse.row >= terminal_top
                    && mouse.row < terminal_top + terminal_rows
                {
                    match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left)
                            if mouse.row == terminal_top && mouse.column < terminal_left + 6 =>
                        {
                            self.prefs.terminal_visible = false;
                            self.terminal_panel.focused = false;
                        }
                        MouseEventKind::Down(MouseButton::Left) => {
                            self.terminal_panel.focused = true;
                            self.side_focus = false;
                        }
                        MouseEventKind::ScrollUp => self.terminal_panel.scroll(true),
                        MouseEventKind::ScrollDown => self.terminal_panel.scroll(false),
                        _ => (),
                    }
                    return Ok(true);
                }
                if self.terminal_panel.focused && matches!(mouse.kind, MouseEventKind::Down(_)) {
                    self.terminal_panel.focused = false;
                }
            }
        }
        Ok(false)
    }

    fn handle_location_input(
        &mut self,
        event: &Event,
        model: &mut Model,
        l: Layout,
    ) -> io::Result<bool> {
        if let Some(input) = &mut self.location {
            match event {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    match k.code {
                        KeyCode::Esc => self.location = None,
                        KeyCode::Char('l' | 'c') if k.modifiers == KeyModifiers::CONTROL => {
                            self.location = None
                        }
                        KeyCode::Char('a') if k.modifiers == KeyModifiers::CONTROL => input.clear(),
                        KeyCode::Backspace => {
                            input.pop();
                        }
                        KeyCode::Enter => {
                            if input.is_empty() {
                                return Ok(true);
                            }
                            let path = if input == "~" || input.starts_with("~/") {
                                crate::platform::home_dir()
                                    .ok_or_else(|| io::Error::other("home unavailable"))?
                                    .join(input.strip_prefix("~/").unwrap_or(""))
                            } else {
                                model.directory.join(input.as_str())
                            };
                            model.go(path.canonicalize()?)?;
                            self.location = None;
                            self.preview_key = None;
                        }
                        KeyCode::Char(c)
                            if !c.is_control()
                                && !k
                                    .modifiers
                                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                                && input.len() + c.len_utf8() <= 4096 =>
                        {
                            input.push(c)
                        }
                        _ => (),
                    }
                }
                Event::Mouse(mouse)
                    if mouse.row == 1
                        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && toolbar_hit(l.width, mouse.column) == Some(0) =>
                {
                    self.location = None
                }
                _ => (),
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn handle_mouse_input(
        &mut self,
        mouse: MouseEvent,
        model: &mut Model,
        full_h: u16,
        l: Layout,
        writable: bool,
    ) -> io::Result<Option<Event>> {
        let h = l.height;
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && mouse.row == 0 {
            if let Some((i, _)) = MENUS
                .iter()
                .enumerate()
                .find(|(_, (name, x))| mouse.column >= *x && mouse.column < *x + name.len() as u16)
            {
                self.side_focus = false;
                let Some(selected) = self.menu(Some(i), writable)? else {
                    return Ok(None);
                };
                return self.input(selected, model, false, writable);
            }
            return Ok(None);
        }
        if self.dual {
            let width = l.width;
            if h < 8 || mouse.row < 3 || mouse.row >= h - 3 || mouse.column >= width {
                return Ok(None);
            }
            if width >= 54 {
                let split = dual_split(width, self.prefs.dual_percent);
                if mouse.column == split {
                    return Ok(None);
                }
                let right = mouse.column > split;
                if right != self.active_right {
                    if matches!(
                        mouse.kind,
                        MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
                    ) {
                        // Focus first; never dispatch an action against rows that
                        // may change when the inactive pane is refreshed.
                        self.switch_pane(model)?;
                    }
                    return Ok(None);
                }
            }
            if mouse.row == 3 {
                return Ok(None);
            }
            let index = model.top + usize::from(mouse.row - 4);
            match mouse.kind {
                MouseEventKind::ScrollUp => model.shift(-1),
                MouseEventKind::ScrollDown => model.shift(1),
                MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
                    if index < model.entries.len() =>
                {
                    model.selected = index;
                    if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                        && mouse.modifiers == KeyModifiers::NONE
                    {
                        return self.context_menu(writable);
                    }
                    if writable
                        && mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && mouse.modifiers == KeyModifiers::CONTROL
                    {
                        return Ok(Some(Event::Key(KeyEvent::new(
                            KeyCode::Char(' '),
                            KeyModifiers::NONE,
                        ))));
                    }
                }
                _ => (),
            }
            return Ok(None);
        }
        if l.sidebar > 0
            && mouse.column < l.sidebar
            && mouse.row >= 4
            && h >= 8
            && mouse.row < full_h - 3
        {
            self.side_focus = true;
            match mouse.kind {
                MouseEventKind::ScrollUp => self.move_side(false),
                MouseEventKind::ScrollDown => self.move_side(true),
                MouseEventKind::Down(MouseButton::Left) => {
                    let i = self.side_top + usize::from(mouse.row - 4);
                    if self.places.get(i).is_some_and(|p| p.1.is_some()) {
                        self.side_cursor = i;
                        self.open_side(model)?;
                    }
                }
                _ => (),
            }
            return Ok(None);
        }
        if l.file_row(mouse.column, mouse.row).is_some() {
            self.side_focus = false;
            let cells = Cells::new(l.end() - l.first(), l.capacity(), self.view, self.zoom);
            match mouse.kind {
                MouseEventKind::ScrollUp => model.shift(-(cells.columns as isize)),
                MouseEventKind::ScrollDown => model.shift(cells.columns as isize),
                MouseEventKind::Down(MouseButton::Left | MouseButton::Right) => {
                    if let Some(offset) =
                        cells.hit(mouse.column - l.first(), usize::from(mouse.row - 4))
                    {
                        let index = model.top + offset;
                        if index < model.entries.len() {
                            model.selected = index;
                            if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                                && mouse.modifiers == KeyModifiers::NONE
                            {
                                return self.context_menu(writable);
                            }
                            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                                if writable && mouse.modifiers == KeyModifiers::CONTROL {
                                    return Ok(Some(Event::Key(KeyEvent::new(
                                        KeyCode::Char(' '),
                                        KeyModifiers::NONE,
                                    ))));
                                }
                                if mouse.modifiers == KeyModifiers::NONE
                                    && self.view == View::Tree
                                    && mouse.column < l.first() + 4
                                {
                                    self.expand_current(model)?;
                                }
                            }
                        }
                    }
                }
                _ => (),
            }
            return Ok(None);
        }
        Ok(None)
    }

    fn handle_key_input(
        &mut self,
        key: KeyEvent,
        model: &mut Model,
        l: Layout,
        writable: bool,
    ) -> io::Result<Option<Event>> {
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(None);
        }
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
            let Some(selected) = self.menu(None, writable)? else {
                return Ok(None);
            };
            return self.input(selected, model, false, writable);
        }
        if code == KeyCode::F(8) && key.modifiers == KeyModifiers::NONE {
            if key.kind == KeyEventKind::Press {
                self.toggle_dual(model)?;
            }
            return Ok(None);
        }
        if code == KeyCode::F(8) && key.modifiers == KeyModifiers::SHIFT {
            self.side_focus = false;
            if cfg!(feature = "native-ops") {
                return Ok(Some(Event::Key(KeyEvent::new(
                    KeyCode::F(8),
                    KeyModifiers::NONE,
                ))));
            }
            return Ok(None);
        }
        if code == KeyCode::Tab || code == KeyCode::BackTab {
            if self.dual {
                if key.kind == KeyEventKind::Press {
                    self.switch_pane(model)?;
                }
            } else {
                self.side_focus = l.sidebar > 0 && !self.side_focus;
            }
            return Ok(None);
        }
        if code == KeyCode::F(9) {
            if self.dual {
                self.dual = false;
                self.preview_enabled = true;
            } else {
                self.preview_enabled = !self.preview_enabled;
            }
            self.preview_key = None;
            return Ok(None);
        }
        if code == KeyCode::F(5) {
            self.metadata_cache.borrow_mut().clear();
            self.preview_key = None;
            self.refresh_places();
        }
        if self.side_focus {
            match code {
                KeyCode::Up => self.move_side(false),
                KeyCode::Down => self.move_side(true),
                KeyCode::Enter | KeyCode::Right => self.open_side(model)?,
                KeyCode::Esc | KeyCode::Left => self.side_focus = false,
                KeyCode::Char('q') => return Ok(Some(Event::Key(key))),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(Some(Event::Key(key)))
                }
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
            let labels: Vec<_> = self
                .bookmarks
                .iter()
                .map(|p| display_name(p.as_os_str()))
                .collect();
            if let Some(i) = crate::choose("Session bookmarks / Saved locations", &labels)? {
                model.go(self.bookmarks[i].clone())?;
            }
            return Ok(None);
        }
        if !self.dual && key.modifiers == KeyModifiers::NONE {
            let cells = Cells::new(l.end() - l.first(), l.capacity(), self.view, self.zoom);
            match code {
                KeyCode::Right if self.view == View::Tree => {
                    self.expand_current(model)?;
                    return Ok(None);
                }
                KeyCode::Up if self.view == View::Grid => {
                    model.shift(-(cells.columns as isize));
                    return Ok(None);
                }
                KeyCode::Down if self.view == View::Grid => {
                    model.shift(cells.columns as isize);
                    return Ok(None);
                }
                KeyCode::Left if self.view == View::Grid => {
                    model.shift(-1);
                    return Ok(None);
                }
                KeyCode::Right if self.view == View::Grid => {
                    model.shift(1);
                    return Ok(None);
                }
                KeyCode::PageUp => {
                    model.shift(-(cells.capacity() as isize));
                    return Ok(None);
                }
                KeyCode::PageDown => {
                    model.shift(cells.capacity() as isize);
                    return Ok(None);
                }
                _ => (),
            }
        }
        let replacement = match code {
            KeyCode::F(4) if writable => Some(KeyCode::F(6)),
            KeyCode::Backspace => Some(KeyCode::Left),
            KeyCode::Null => return Ok(None),
            _ => None,
        };
        if let Some(code) = replacement {
            return Ok(Some(Event::Key(KeyEvent::new(code, KeyModifiers::NONE))));
        }
        Ok(Some(Event::Key(key)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::tests::Fixture;

    #[test]
    fn terminal_compatible_view_keys_and_reset() {
        let mut state = State::new(None);
        for (digit, view) in [('1', View::Grid), ('2', View::List), ('3', View::Tree)] {
            assert!(state.handle_view_shortcut(KeyEvent::new(
                KeyCode::Char(digit),
                KeyModifiers::NONE
            )));
            assert_eq!(state.view, view);
        }
        state.zoom = 3;
        state.prefs.sidebar = false;
        state.dual = true;
        state.dragging = Some(Divider::Dual);
        assert!(state.handle_view_shortcut(KeyEvent::new(
            KeyCode::Char('0'),
            KeyModifiers::NONE
        )));
        assert_eq!(state.zoom, 1);
        assert_eq!(state.view, View::Tree);
        assert!(!state.dual);
        assert!(state.dragging.is_none());
        assert!(state.prefs.sidebar);
        assert!(!state.preview_enabled);
        assert!(!state.prefs.terminal_visible);
    }

    #[test]
    fn modified_write_shortcuts_remain_scoped() {
        let f = Fixture::new();
        let mut model = Model::new(f.0.clone()).unwrap();
        let mut state = State::new(None);
        let l = Layout::new(120, 26, true);
        let cut = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(
            state.handle_key_input(cut, &mut model, l, false).unwrap(),
            None
        );
        assert_eq!(
            state.handle_key_input(cut, &mut model, l, true).unwrap(),
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char('x'),
                KeyModifiers::NONE
            )))
        );
        let delete = KeyEvent::new(KeyCode::Delete, KeyModifiers::ALT);
        assert_eq!(
            state.handle_key_input(delete, &mut model, l, true).unwrap(),
            None
        );
        let release =
            KeyEvent::new_with_kind(KeyCode::Delete, KeyModifiers::NONE, KeyEventKind::Release);
        assert_eq!(
            state
                .handle_key_input(release, &mut model, l, true)
                .unwrap(),
            None
        );
    }

    #[test]
    fn location_consumes_keys_until_cancelled() {
        let f = Fixture::new();
        let mut model = Model::new(f.0.clone()).unwrap();
        let mut state = State::new(None);
        let l = Layout::new(120, 26, true);
        state.location = Some(String::new());
        let key = Event::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(state.handle_location_input(&key, &mut model, l).unwrap());
        assert_eq!(state.location.as_deref(), Some("q"));
        let cancel = Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(state.handle_location_input(&cancel, &mut model, l).unwrap());
        assert!(state.location.is_none());
        assert!(!state.handle_location_input(&key, &mut model, l).unwrap());
        assert_eq!(model.directory, f.0);
    }

    #[test]
    fn mouse_selection_respects_bounds_and_write_permission() {
        let f = Fixture::new();
        std::fs::write(f.0.join("first"), b"a").unwrap();
        std::fs::write(f.0.join("second"), b"b").unwrap();
        let mut model = Model::new(f.0.clone()).unwrap();
        let mut state = State::new(None);
        let l = Layout::new(120, 26, true);
        let mut click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column: u16::MAX,
            row: u16::MAX,
            modifiers: KeyModifiers::CONTROL,
        };
        assert_eq!(
            state
                .handle_mouse_input(click, &mut model, 26, l, true)
                .unwrap(),
            None
        );
        assert_eq!(model.selected, 0);
        click.column = l.first();
        click.row = 5;
        assert_eq!(
            state
                .handle_mouse_input(click, &mut model, 26, l, false)
                .unwrap(),
            None
        );
        assert_eq!(model.selected, 1);
        assert_eq!(
            state
                .handle_mouse_input(click, &mut model, 26, l, true)
                .unwrap(),
            Some(Event::Key(KeyEvent::new(
                KeyCode::Char(' '),
                KeyModifiers::NONE
            )))
        );
        assert_eq!(std::fs::read(f.0.join("second")).unwrap(), b"b");
    }
}
