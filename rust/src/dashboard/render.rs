//! Dashboard rendering and terminal status presentation.
use super::file_rows::{
    breadcrumb, entry_type, human_size, list_heading, modified_utc, pad_cell, row_metadata,
    row_text, table_columns, RowPresentation,
};
use super::geometry::{dual_split, Layout};
use super::markdown::PreviewStyle;
use super::view::{draw_toolbar, Cells, View};
use super::{DrawStatus, State};
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};
use terminal_tree::{display_name, Model};

impl State {
    #[cfg(feature = "native-ops")]
    fn draw_terminal(
        &mut self,
        out: &mut impl Write,
        h: u16,
        l: Layout,
        model: &Model,
    ) -> io::Result<()> {
        let (left, top, width, rows) = self.terminal_rect(h, l);
        if rows > 0 && width > 0 {
            self.terminal_panel
                .resize_shell(width, rows.saturating_sub(3))?;
            self.terminal_panel.draw(
                out,
                crate::terminal_panel::PanelRect {
                    left,
                    top,
                    rows,
                    width,
                },
                &model.directory,
                &self.prefs,
            )?;
        }
        Ok(())
    }
    #[cfg(feature = "native-ops")]
    fn terminal_button(&self, out: &mut impl Write, width: u16) -> io::Result<()> {
        const X: u16 = 63;
        let label = if self.terminal_panel.running() {
            "[Terminal*]"
        } else if self.prefs.terminal_visible {
            "[Terminal -]"
        } else {
            "[Terminal +]"
        };
        if width >= X + label.len() as u16 {
            queue!(
                out,
                MoveTo(X, 0),
                SetForegroundColor(self.prefs.accent),
                Print(label),
                SetForegroundColor(self.prefs.foreground)
            )?;
        }
        if width >= 83 {
            queue!(
                out,
                MoveTo(78, 0),
                SetForegroundColor(self.prefs.accent),
                Print(if self.prefs.companions[2] {
                    "[Git]"
                } else {
                    "[git]"
                }),
                SetForegroundColor(self.prefs.foreground)
            )?;
        }
        Ok(())
    }
    fn disk_status(&mut self, path: &std::path::Path) -> String {
        #[cfg(feature = "native-ops")]
        {
            if let Some((cached, when, label)) = &self.disk_cache {
                if cached == path && when.elapsed() < std::time::Duration::from_secs(2) {
                    return label.clone();
                }
            }
            let label = match rustix::fs::statvfs(path) {
                Ok(s) => format!(
                    "Free: {}",
                    human_size(s.f_bavail.saturating_mul(s.f_frsize))
                ),
                Err(_) => "Free: unavailable".into(),
            };
            self.disk_cache = Some((path.to_path_buf(), std::time::Instant::now(), label.clone()));
            label
        }
        #[cfg(not(feature = "native-ops"))]
        {
            let _ = path;
            String::new()
        }
    }
    fn task_label(&self) -> &'static str {
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.interactive() {
            return "PTY active";
        }
        #[cfg(feature = "native-ops")]
        if self.terminal_panel.running() {
            return "Bash running";
        }
        #[cfg(feature = "native-ops")]
        if self.preview_worker.busy() {
            return "Preview loading";
        }
        "idle"
    }

    fn draw_dual(
        &mut self,
        model: &mut Model,
        status: DrawStatus<'_>,
        marked: &impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        let DrawStatus {
            message,
            searching,
            writable,
            personal,
            clipboard,
        } = status;
        let (w, full_h) = terminal::size()?;
        let h = full_h - self.panel_rows(full_h);
        let width = w.saturating_sub(1);
        let mut out = Vec::new();
        queue!(
            out,
            MoveTo(0, 0),
            SetForegroundColor(self.prefs.foreground),
            SetBackgroundColor(self.prefs.background),
            Clear(ClearType::All),
            SetForegroundColor(self.prefs.accent),
            Print(crate::clip(
                "Terminal Tree  File  Edit  View  Go  Tools  Settings  Add-ons",
                usize::from(width)
            )),
            SetForegroundColor(self.prefs.foreground),
            SetBackgroundColor(self.prefs.background)
        )?;
        #[cfg(feature = "native-ops")]
        self.terminal_button(&mut out, width)?;
        if h < 8 {
            queue!(out, ResetColor)?;
            return self
                .frame_cache
                .present(out, w, full_h, self.prefs.background);
        }
        let side = if self.active_right { "Right" } else { "Left" };
        let mode = if personal {
            "Personal"
        } else if writable {
            "Workspace"
        } else {
            "Read-only"
        };
        let header = format!(
            "Dual Pane | Active: {side} | [{} / {mode}]",
            crate::edition::NAME
        );
        queue!(
            out,
            MoveTo(0, 1),
            Print(crate::clip(&header, usize::from(width)))
        )?;
        self.draw_tabs(&mut out, model, width)?;
        let cap = usize::from(h - 7);
        if width >= 54 {
            let split = dual_split(width, self.prefs.dual_percent);
            let other = self
                .other
                .as_mut()
                .ok_or_else(|| io::Error::other("second pane unavailable"))?;
            if self.active_right {
                draw_column(
                    &mut out,
                    other,
                    Column {
                        label: "Left",
                        active: false,
                        x: 0,
                        width: split,
                        cap,
                    },
                    marked,
                    self.icons,
                    &self.prefs,
                )?;
                draw_column(
                    &mut out,
                    model,
                    Column {
                        label: "Right",
                        active: true,
                        x: split + 1,
                        width: width - split - 1,
                        cap,
                    },
                    marked,
                    self.icons,
                    &self.prefs,
                )?;
            } else {
                draw_column(
                    &mut out,
                    model,
                    Column {
                        label: "Left",
                        active: true,
                        x: 0,
                        width: split,
                        cap,
                    },
                    marked,
                    self.icons,
                    &self.prefs,
                )?;
                draw_column(
                    &mut out,
                    other,
                    Column {
                        label: "Right",
                        active: false,
                        x: split + 1,
                        width: width - split - 1,
                        cap,
                    },
                    marked,
                    self.icons,
                    &self.prefs,
                )?;
            }
            for row in 3..h - 3 {
                queue!(out, MoveTo(split, row), Print("│"))?;
            }
        } else {
            draw_column(
                &mut out,
                model,
                Column {
                    label: side,
                    active: true,
                    x: 0,
                    width,
                    cap,
                },
                marked,
                self.icons,
                &self.prefs,
            )?;
        }
        let safety = if writable { "Scoped" } else { "Read-only" };
        let disk = if self.prefs.status_details {
            self.disk_status(&model.directory)
        } else {
            String::new()
        };
        let status = if self.prefs.status_details {
            format!(
                "Clipboard: {clipboard} files | Tasks: {} | Safety: {safety} | {} entries | {disk}",
                self.task_label(),
                model.entries.len()
            )
        } else {
            format!("Safety: {safety}")
        };
        let detail = if searching || !model.query.is_empty() {
            format!(
                "Filter: {} | {message}",
                display_name(std::ffi::OsStr::new(&model.query))
            )
        } else {
            message.to_owned()
        };
        queue!(
            out,
            MoveTo(0, h - 3),
            Print(crate::clip(&status, usize::from(width))),
            MoveTo(0, h - 2),
            Print(crate::clip(&detail, usize::from(width))),
            MoveTo(0, h - 1),
            Print(crate::clip(
                if searching {
                    "Filter: type text | Enter accept | Esc cancel"
                } else if writable {
                    "F8 Single  Tab Switch  Ctrl+C/X/V Transfer  F4 Actions  F10 Menu"
                } else {
                    "F8 Single  Tab Switch  F10 Menu  Ctrl+F Filter  q Exit"
                },
                usize::from(width)
            ))
        )?;
        #[cfg(feature = "native-ops")]
        self.draw_terminal(&mut out, full_h, self.layout(w, h), model)?;
        queue!(out, ResetColor)?;
        self.frame_cache
            .present(out, w, full_h, self.prefs.background)
    }

    pub fn draw(
        &mut self,
        model: &mut Model,
        status: DrawStatus<'_>,
        marked: impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        if self.dual {
            #[cfg(feature = "native-ops")]
            {
                self.suspend_visual_preview();
                self.graphics_slot.clear()?;
                self.graphics_placed = false;
            }
            return self.draw_dual(model, status, &marked);
        }
        let DrawStatus {
            message,
            searching,
            writable,
            personal,
            clipboard,
        } = status;
        let (w, full_h) = terminal::size()?;
        let h = full_h - self.panel_rows(full_h);
        let l = self.layout(w, h);
        if l.sidebar == 0 {
            self.side_focus = false;
        }
        let mut out = Vec::new();
        queue!(
            out,
            MoveTo(0, 0),
            SetForegroundColor(self.prefs.foreground),
            SetBackgroundColor(self.prefs.background),
            Clear(ClearType::All),
            SetForegroundColor(self.prefs.accent),
            Print(crate::clip(
                "Terminal Tree  File  Edit  View  Go  Tools  Settings  Add-ons",
                usize::from(l.width)
            )),
            SetForegroundColor(self.prefs.foreground),
            SetBackgroundColor(self.prefs.background)
        )?;
        #[cfg(feature = "native-ops")]
        self.terminal_button(&mut out, l.width)?;
        if h < 8 {
            #[cfg(feature = "native-ops")]
            {
                self.suspend_visual_preview();
                self.graphics_slot.clear()?;
                self.graphics_placed = false;
            }
            queue!(out, ResetColor)?;
            return self
                .frame_cache
                .present(out, w, full_h, self.prefs.background);
        }
        let mode = if personal {
            "Personal"
        } else if writable {
            "Workspace"
        } else {
            "Read-only"
        };
        let path_width = if self.prefs.toolbar {
            draw_toolbar(&mut out, l.width, self.view, &self.prefs)?
        } else {
            l.width
        };
        let path = if let Some(input) = &self.location {
            format!("Location: {}_", display_name(std::ffi::OsStr::new(input)))
        } else {
            breadcrumb(&model.directory)
        };
        let nav = if self.location.is_none() && path_width >= 52 {
            12
        } else {
            0
        };
        if nav > 0 {
            queue!(
                out,
                MoveTo(0, 1),
                SetForegroundColor(self.prefs.accent),
                Print("[←] [→] [↑] "),
                SetForegroundColor(self.prefs.foreground)
            )?;
        }
        queue!(
            out,
            MoveTo(nav, 1),
            Print(crate::clip(
                &path,
                usize::from(path_width.saturating_sub(nav))
            ))
        )?;
        self.draw_tabs(&mut out, model, l.width)?;
        if l.sidebar > 0 {
            queue!(
                out,
                MoveTo(0, 3),
                SetForegroundColor(self.prefs.accent),
                Print(if self.side_focus {
                    "Places *"
                } else {
                    "Places"
                }),
                SetForegroundColor(self.prefs.foreground),
                SetBackgroundColor(self.prefs.background)
            )?;
        }
        queue!(
            out,
            MoveTo(l.first(), 3),
            SetForegroundColor(self.prefs.accent),
            Print(
                if self.view == View::List || (self.view == View::Tree && l.end() - l.first() >= 48)
                {
                    list_heading(
                        model.order,
                        !self.side_focus,
                        l.end() - l.first(),
                        self.prefs.list_details,
                    )
                } else if self.side_focus {
                    "Files".into()
                } else {
                    "Files *".into()
                }
            ),
            SetForegroundColor(self.prefs.foreground),
            SetBackgroundColor(self.prefs.background)
        )?;
        if l.preview > 0 {
            queue!(
                out,
                MoveTo(l.end() + 1, 3),
                SetForegroundColor(self.prefs.accent),
                Print(crate::clip(
                    match self.preview_mode {
                        1 => "Preview [Info]",
                        2 => "Preview [Hex]",
                        _ => "Preview [Auto]",
                    },
                    usize::from(l.preview)
                )),
                SetForegroundColor(self.prefs.foreground),
                SetBackgroundColor(self.prefs.background)
            )?;
            if l.preview >= 46 {
                queue!(
                    out,
                    MoveTo(l.end() + 17, 3),
                    SetForegroundColor(self.prefs.foreground),
                    Print("[Auto] [Info] [Hex] [↗]")
                )?;
            }
            let entry = if let Some(tree) = &self.expanded_tree {
                tree.rows().get(tree.selected()).map(|r| r.entry.clone())
            } else {
                model.entries.get(model.selected).cloned()
            };
            self.preview(entry.as_ref(), l.preview, l.capacity());
            #[cfg(feature = "native-ops")]
            {
                self.poll_visual_preview();
                if self.graphics_slot.overlay_expired() {
                    self.graphics_placed = false;
                    self.graphics_focused = false;
                }
            }
        } else {
            #[cfg(feature = "native-ops")]
            self.suspend_visual_preview();
        }
        let cap = l.capacity();
        let cells = Cells::new(l.end() - l.first(), cap, self.view, self.zoom);
        let (start, end) = if let Some(tree) = &mut self.expanded_tree {
            tree.viewport(cells.capacity())
        } else {
            model.viewport(cells.capacity())
        };
        self.draw_sidebar(&mut out, model, full_h, l)?;
        self.draw_preview_rows(&mut out, l, cap)?;
        self.draw_file_rows(&mut out, model, l, cells, start..end, &marked)?;
        let safety = if self.expanded_tree.is_some() {
            "Tree inspection"
        } else if writable {
            "Scoped"
        } else {
            "Read-only"
        };
        let disk = if self.prefs.status_details {
            self.disk_status(&model.directory)
        } else {
            String::new()
        };
        let status = if self.prefs.status_details {
            format!("Clipboard: {clipboard} files | Tasks: {} | Safety: {safety} | {} | {} View | Zoom {} | {disk}",self.task_label(),model.entries.len(),self.view.name(),self.zoom)
        } else {
            format!(
                "Safety: {safety} | {} View | Zoom {}",
                self.view.name(),
                self.zoom
            )
        };
        let detail = if self.expanded_tree.is_some() {
            "Expanded tree: Right expand / Left collapse / Enter open / Esc back".to_owned()
        } else if searching || !model.query.is_empty() {
            format!(
                "Search: {} | {message}",
                display_name(std::ffi::OsStr::new(&model.query))
            )
        } else {
            format!(
                "[{} / {mode}] Icons: {} | {message}",
                crate::edition::NAME,
                self.icons.name()
            )
        };
        queue!(
            out,
            MoveTo(0, full_h - 3),
            Print(crate::clip(&status, usize::from(l.width))),
            MoveTo(0, full_h - 2),
            Print(crate::clip(&detail, usize::from(l.width))),
            MoveTo(0, full_h - 1),
            Print(crate::clip(
                if searching {
                    "Filter: type text | Enter accept | Esc cancel"
                } else if writable {
                    "F1 Help  F2 Rename  F3 Preview  F8 Dual  Ctrl+T Tab  Ctrl+K Commands"
                } else {
                    "F1 Help  F3 Preview  F8 Dual  Ctrl+T Tab  Ctrl+K Commands  q Exit"
                },
                usize::from(l.width)
            ))
        )?;
        #[cfg(feature = "native-ops")]
        self.draw_terminal(&mut out, full_h, l, model)?;
        queue!(out, ResetColor)?;
        #[cfg(feature = "native-ops")]
        if l.preview == 0
            || !self.visual_bitmap
            || self.visual_frame.is_none()
            || !self.graphics_placed
        {
            self.graphics_slot.clear()?;
            self.graphics_placed = false;
        }
        self.frame_cache
            .present(out, w, full_h, self.prefs.background)?;
        #[cfg(feature = "native-ops")]
        if l.preview > 0
            && self.visual_bitmap
            && !self.graphics_placed
            && self.graphics_focused
            && cap > 2
        {
            if let Some(frame) = &self.visual_frame {
                let area = (l.preview, (cap - 2).min(u16::MAX as usize) as u16);
                let (cols, rows) = if crate::x11_overlay::requested() {
                    area
                } else {
                    crate::native_graphics::fit(frame, area.0, area.1)
                };
                match self.graphics_slot.show(
                    frame,
                    l.end() + 1 + (l.preview - cols) / 2,
                    6,
                    cols,
                    rows,
                ) {
                    Ok(()) => self.graphics_placed = true,
                    Err(_) if crate::x11_overlay::requested() => {
                        self.graphics_failed = true;
                        self.visual_bitmap = false;
                        self.visual_frame = None;
                        self.preview_key = None;
                        self.preview_lines =
                            vec!["X11 placement unavailable; using Unicode".into()];
                        // Queue the fallback now so the idle event loop will poll it.
                        let e = if let Some(tree) = &self.expanded_tree {
                            tree.rows().get(tree.selected()).map(|r| r.entry.clone())
                        } else {
                            model.entries.get(model.selected).cloned()
                        };
                        self.preview(e.as_ref(), l.preview, cap);
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        Ok(())
    }

    fn draw_sidebar(
        &mut self,
        out: &mut impl Write,
        model: &Model,
        full_h: u16,
        l: Layout,
    ) -> io::Result<()> {
        let side_cap = usize::from(full_h.saturating_sub(7)).max(1);
        if self.side_cursor < self.side_top {
            self.side_top = self.side_cursor;
        }
        if self.side_cursor >= self.side_top + side_cap {
            self.side_top = self.side_cursor + 1 - side_cap;
        }
        self.side_top = self
            .side_top
            .min(self.places.len().saturating_sub(side_cap));
        for row in 0..side_cap {
            let y = row as u16 + 4;
            if l.sidebar > 0 {
                if let Some((label, path)) = self.places.get(self.side_top + row) {
                    queue!(out, MoveTo(0, y))?;
                    if path.is_none() {
                        queue!(out, SetForegroundColor(self.prefs.accent))?;
                    } else if (self.side_focus && self.side_top + row == self.side_cursor)
                        || path.as_ref() == Some(&model.directory)
                    {
                        queue!(out, SetBackgroundColor(self.prefs.selection))?;
                    }
                    let text = if let Some(path) = path {
                        let (builtin, _) = self.prefs.icon(path, true, false, self.icons);
                        let glyph = if self.prefs.show_icons
                            && self.icons == crate::file_icons::Style::Unicode
                        {
                            match label.as_str() {
                                "Home" => "⌂",
                                "Desktop" => "▣",
                                "Documents" => "▤",
                                "Downloads" => "↓",
                                "Pictures" => "▧",
                                "Music" => "♫",
                                "Videos" => "▶",
                                "Trash" => "♲",
                                "File System" => "▥",
                                _ => builtin,
                            }
                        } else {
                            builtin
                        };
                        format!(" {glyph} {label}")
                    } else {
                        label.clone()
                    };
                    queue!(
                        out,
                        Print(pad_cell(&text, usize::from(l.sidebar))),
                        SetForegroundColor(self.prefs.foreground),
                        SetBackgroundColor(self.prefs.background)
                    )?;
                }
                queue!(out, MoveTo(l.sidebar, y), Print("│"))?;
            }
        }
        Ok(())
    }

    fn draw_preview_rows(&self, out: &mut impl Write, l: Layout, cap: usize) -> io::Result<()> {
        for row in 0..cap {
            let y = row as u16 + 4;
            if l.preview > 0 {
                queue!(out, MoveTo(l.end(), y), Print("│"))?;
                if let Some(line) = self.preview_lines.get(row + self.preview_scroll) {
                    let style = self
                        .preview_styles
                        .get(row + self.preview_scroll)
                        .copied()
                        .unwrap_or_default();
                    queue!(out, MoveTo(l.end() + 1, y))?;
                    match style {
                        PreviewStyle::Heading => queue!(
                            out,
                            SetAttribute(Attribute::Bold),
                            SetForegroundColor(self.prefs.accent)
                        )?,
                        PreviewStyle::Code => queue!(out, SetForegroundColor(Color::Yellow))?,
                        PreviewStyle::Quote => queue!(out, SetForegroundColor(self.prefs.accent))?,
                        PreviewStyle::Plain => (),
                    }
                    queue!(
                        out,
                        Print(crate::clip(line, usize::from(l.preview))),
                        SetAttribute(Attribute::NormalIntensity),
                        SetForegroundColor(self.prefs.foreground)
                    )?;
                } else {
                    #[cfg(feature = "native-ops")]
                    if let Some(frame) = self.visual_frame.as_ref().filter(|_| !self.visual_bitmap)
                    {
                        let py = row.saturating_sub(2) * 2;
                        if py < frame.height {
                            queue!(out, MoveTo(l.end() + 1, y))?;
                            for x in 0..frame.width.min(usize::from(l.preview)) {
                                let [r, g, b] = frame.pixel(x, py);
                                let [r2, g2, b2] = frame.pixel(x, py + 1);
                                queue!(
                                    out,
                                    SetForegroundColor(Color::Rgb { r, g, b }),
                                    SetBackgroundColor(Color::Rgb {
                                        r: r2,
                                        g: g2,
                                        b: b2
                                    }),
                                    Print("▀")
                                )?;
                            }
                            queue!(
                                out,
                                SetForegroundColor(self.prefs.foreground),
                                SetBackgroundColor(self.prefs.background)
                            )?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn draw_file_rows(
        &self,
        out: &mut impl Write,
        model: &Model,
        l: Layout,
        cells: Cells,
        range: std::ops::Range<usize>,
        marked: &impl Fn(&std::path::Path) -> bool,
    ) -> io::Result<()> {
        let h = l.height;
        let start = range.start;
        for index in range {
            let offset = index - start;
            let x = l.first() + (offset % cells.columns) as u16 * cells.width;
            let y = 4 + ((offset / cells.columns) * cells.stride) as u16;
            if y >= h - 3 {
                break;
            }
            let (e, selected, tree_depth, expanded) = if let Some(tree) = &self.expanded_tree {
                let row = &tree.rows()[index];
                (
                    &row.entry,
                    tree.selected() == index,
                    Some(row.depth),
                    row.expanded,
                )
            } else {
                (&model.entries[index], model.selected == index, None, false)
            };
            let (glyph, color) = self.prefs.icon(&e.path, e.directory, e.symlink, self.icons);
            queue!(out, MoveTo(x, y))?;
            if selected {
                queue!(out, SetBackgroundColor(self.prefs.selection))?;
            }
            queue!(
                out,
                SetForegroundColor(if selected || self.view != View::Grid {
                    self.prefs.foreground
                } else {
                    color
                })
            )?;
            if self.view == View::Grid {
                let mark = if marked(&e.path) { "[x] " } else { "" };
                queue!(
                    out,
                    Print(crate::clip(
                        &format!("{mark}{glyph}"),
                        usize::from(cells.width)
                    ))
                )?;
                if y + 1 < h - 3 {
                    queue!(
                        out,
                        MoveTo(x, y + 1),
                        Print(crate::clip(&e.name, usize::from(cells.width)))
                    )?;
                }
            } else {
                let text = if let Some(depth) = tree_depth {
                    let name = format!(
                        "{}{} {glyph} {}",
                        "  ".repeat(depth - 1),
                        if expanded {
                            "▾"
                        } else if e.directory && !e.symlink {
                            "▸"
                        } else {
                            "·"
                        },
                        e.name
                    );
                    if cells.width >= 48 {
                        let (size, time) = row_metadata(e);
                        table_columns(
                            &name,
                            &if e.directory {
                                "—".into()
                            } else {
                                size.map(human_size).unwrap_or_else(|| "—".into())
                            },
                            &entry_type(e),
                            &modified_utc(time),
                            cells.width,
                            self.prefs.list_details,
                        )
                    } else {
                        name
                    }
                } else {
                    row_text(
                        e,
                        index,
                        model.entries.len(),
                        glyph,
                        marked(&e.path),
                        RowPresentation {
                            view: self.view,
                            width: cells.width,
                            details: self.prefs.list_details,
                        },
                    )
                };
                let label = if selected {
                    pad_cell(&text, usize::from(cells.width))
                } else {
                    crate::clip(&text, usize::from(cells.width))
                };
                queue!(out, Print(label))?;
                // Color just the icon, keeping names and metadata neutral.
                use unicode_width::UnicodeWidthStr;
                let prefix = if let Some(depth) = tree_depth {
                    2 * (depth - 1) + 2
                } else if self.view == View::Tree {
                    8
                } else {
                    4
                };
                if !glyph.is_empty() && prefix + glyph.width() <= usize::from(cells.width) {
                    queue!(
                        out,
                        MoveTo(x + prefix as u16, y),
                        SetForegroundColor(color),
                        Print(glyph)
                    )?;
                }
            }
            queue!(
                out,
                SetForegroundColor(self.prefs.foreground),
                SetBackgroundColor(self.prefs.background)
            )?;
        }
        Ok(())
    }
}

struct Column<'a> {
    label: &'a str,
    active: bool,
    x: u16,
    width: u16,
    cap: usize,
}

fn draw_column(
    out: &mut impl Write,
    model: &mut Model,
    column: Column<'_>,
    marked: &impl Fn(&std::path::Path) -> bool,
    icons: crate::file_icons::Style,
    prefs: &crate::app_settings::Preferences,
) -> io::Result<()> {
    let Column {
        label,
        active,
        x,
        width,
        cap,
    } = column;
    let path = display_name(model.directory.as_os_str());
    let header = format!("{label}{} | {path}", if active { " *" } else { "" });
    queue!(
        out,
        MoveTo(x, 3),
        SetForegroundColor(if active {
            prefs.accent
        } else {
            prefs.foreground
        }),
        Print(crate::clip(&header, usize::from(width))),
        SetForegroundColor(prefs.foreground),
        SetBackgroundColor(prefs.background)
    )?;
    let (start, end) = model.viewport(cap);
    for (row, index) in (start..end).enumerate() {
        let entry = &model.entries[index];
        let (glyph, color) = prefs.icon(&entry.path, entry.directory, entry.symlink, icons);
        let text = format!(
            "{} {} {}",
            if active && marked(&entry.path) {
                "[x]"
            } else {
                "   "
            },
            glyph,
            entry.name
        );
        queue!(out, MoveTo(x, row as u16 + 4))?;
        if active && index == model.selected {
            queue!(out, SetBackgroundColor(prefs.selection))?;
        }
        queue!(
            out,
            SetForegroundColor(color),
            Print(crate::clip(&text, usize::from(width))),
            SetForegroundColor(prefs.foreground),
            SetBackgroundColor(prefs.background)
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dashboard::tests::Fixture;

    #[test]
    fn file_rows_render_visible_entries_and_marks() {
        let f = Fixture::new();
        std::fs::write(f.0.join("alpha.txt"), b"a").unwrap();
        std::fs::write(f.0.join("beta.txt"), b"b").unwrap();
        let model = Model::new(f.0.clone()).unwrap();
        let mut state = State::new(None);
        state.view = View::List;
        let l = Layout::new(120, 26, true);
        let cells = Cells::new(l.end() - l.first(), l.capacity(), state.view, state.zoom);
        let mut out = Vec::new();
        state.draw_file_rows(&mut out, &model, l, cells, 0..1, &|_| true).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("alpha.txt"));
        assert!(text.contains("[x]"));
        assert!(!text.contains("beta.txt"));
    }

    #[test]
    fn preview_rows_respect_scroll_and_hidden_panel() {
        let mut state = State::new(None);
        state.preview_lines = vec!["first-marker".into(), "second-marker".into()];
        state.preview_scroll = 1;
        let l = Layout::new(120, 26, true);
        let mut out = Vec::new();
        state.draw_preview_rows(&mut out, l, 1).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("second-marker"));
        assert!(!text.contains("first-marker"));
        let mut out = Vec::new();
        state.draw_preview_rows(&mut out, Layout::new(45, 26, true), 1).unwrap();
        assert!(out.is_empty());
    }

    #[test]
    fn sidebar_respects_hidden_panel() {
        let f = Fixture::new();
        let model = Model::new(f.0.clone()).unwrap();
        let mut state = State::new(None);
        state.places = vec![("fixture-marker".into(), Some(f.0.clone()))];
        state.side_cursor = 0;
        state.side_top = 0;
        let mut out = Vec::new();
        state.draw_sidebar(&mut out, &model, 26, Layout::new(120, 26, true)).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("fixture-marker"));
        let mut out = Vec::new();
        state.draw_sidebar(&mut out, &model, 26, Layout::new(45, 26, true)).unwrap();
        assert!(out.is_empty());
    }
}
