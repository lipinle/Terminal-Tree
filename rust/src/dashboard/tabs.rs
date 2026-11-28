//! Tab lifecycle, navigation and rendering.
use super::file_rows::pad_cell;
use super::State;
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Print, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, Write};
use terminal_tree::{display_name, Model};

pub(super) struct Tab {
    model: Model,
    other: Option<Model>,
    dual: bool,
    active_right: bool,
}

impl State {
    pub(super) fn tab_slots(&self, width: u16) -> (usize, u16, usize) {
        // Keep the active tab visible even on a compact terminal.
        let cell = 18u16.min(width.saturating_sub(4).max(4));
        let count = usize::from(width.saturating_sub(4) / cell)
            .max(1)
            .min(self.tabs.len());
        let first = self
            .active_tab
            .saturating_sub(count.saturating_sub(1))
            .min(self.tabs.len().saturating_sub(count));
        (first, cell, count)
    }

    pub(super) fn draw_tabs(
        &self,
        out: &mut impl Write,
        model: &Model,
        width: u16,
    ) -> io::Result<()> {
        if width < 8 {
            return Ok(());
        }
        let (first, cell, count) = self.tab_slots(width);
        for (slot, index) in (first..first + count).enumerate() {
            let path = if index == self.active_tab {
                &model.directory
            } else if let Some(tab) = &self.tabs[index] {
                &tab.model.directory
            } else {
                continue;
            };
            let name = path
                .file_name()
                .map(display_name)
                .unwrap_or_else(|| display_name(path.as_os_str()));
            let label = format!("[{} {}]", index + 1, name);
            queue!(out, MoveTo(slot as u16 * cell, 2))?;
            if index == self.active_tab {
                queue!(
                    out,
                    SetBackgroundColor(self.prefs.selection),
                    SetForegroundColor(self.prefs.accent)
                )?;
            }
            if cell >= 12 && self.tabs.len() > 1 {
                queue!(
                    out,
                    Print(pad_cell(&label, usize::from(cell - 3))),
                    Print(" ×"),
                    SetForegroundColor(self.prefs.foreground),
                    SetBackgroundColor(self.prefs.background)
                )?;
            } else {
                queue!(
                    out,
                    Print(crate::clip(&label, usize::from(cell.saturating_sub(1)))),
                    SetForegroundColor(self.prefs.foreground),
                    SetBackgroundColor(self.prefs.background)
                )?;
            }
        }
        if count as u16 * cell + 3 <= width {
            queue!(out, MoveTo(count as u16 * cell, 2), Print("[+]"))?;
        }
        Ok(())
    }

    pub(super) fn clear_tab_transients(&mut self) {
        self.side_focus = false;
        self.expanded_tree = None;
        self.location = None;
        self.preview_key = None;
        self.dragging = None;
        self.pane_switched = true;
    }

    fn park(&mut self, model: &mut Model, mut incoming: Tab) -> Tab {
        std::mem::swap(model, &mut incoming.model);
        std::mem::swap(&mut self.other, &mut incoming.other);
        std::mem::swap(&mut self.dual, &mut incoming.dual);
        std::mem::swap(&mut self.active_right, &mut incoming.active_right);
        self.clear_tab_transients();
        incoming
    }

    pub(super) fn new_tab(&mut self, model: &mut Model) -> io::Result<()> {
        if self.tabs.len() >= 8 {
            return Err(io::Error::other("maximum eight tabs"));
        }
        let mut next = Model::new(model.directory.clone())?;
        next.set_view(model.hidden, model.order, model.kind_filter, String::new())?;
        let parked = self.park(
            model,
            Tab {
                model: next,
                other: None,
                dual: false,
                active_right: false,
            },
        );
        self.tabs[self.active_tab] = Some(parked);
        self.tabs.push(None);
        self.active_tab = self.tabs.len() - 1;
        Ok(())
    }

    pub(super) fn switch_tab(&mut self, model: &mut Model, index: usize) -> io::Result<()> {
        if index == self.active_tab || index >= self.tabs.len() {
            return Ok(());
        }
        let tab = self.tabs[index]
            .as_mut()
            .ok_or_else(|| io::Error::other("tab unavailable"))?;
        tab.model.reload()?;
        let incoming = self.tabs[index]
            .take()
            .ok_or_else(|| io::Error::other("tab unavailable"))?;
        let parked = self.park(model, incoming);
        self.tabs[self.active_tab] = Some(parked);
        self.active_tab = index;
        Ok(())
    }

    pub(super) fn close_tab(&mut self, model: &mut Model) -> io::Result<()> {
        if self.tabs.len() == 1 {
            return Ok(());
        }
        let previous = self.active_tab;
        let next = if previous > 0 { previous - 1 } else { 1 };
        self.switch_tab(model, next)?;
        self.tabs.remove(previous);
        if previous < self.active_tab {
            self.active_tab -= 1;
        }
        Ok(())
    }
}
