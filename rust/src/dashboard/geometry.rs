//! Dashboard layout geometry and panel resizing.
use super::State;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Layout {
    pub(super) width: u16,
    pub(super) height: u16,
    pub(super) sidebar: u16,
    pub(super) preview: u16,
}

impl Layout {
    pub(super) fn new(width: u16, height: u16, preview: bool) -> Self {
        let width = width.saturating_sub(1);
        let sidebar = if width >= 54 { 19.min(width / 4) } else { 0 };
        let preview = if preview && width >= 92 { width / 3 } else { 0 };
        Self {
            width,
            height,
            sidebar,
            preview,
        }
    }

    pub(super) fn first(&self) -> u16 {
        if self.sidebar > 0 {
            self.sidebar + 1
        } else {
            0
        }
    }
    pub(super) fn end(&self) -> u16 {
        if self.preview > 0 {
            self.width - self.preview - 1
        } else {
            self.width
        }
    }
    pub(super) fn capacity(&self) -> usize {
        usize::from(self.height.saturating_sub(7)).max(1)
    }
    pub(super) fn file_row(&self, column: u16, row: u16) -> Option<u16> {
        if self.height >= 8
            && row >= 4
            && row < self.height - 3
            && column >= self.first()
            && column < self.end()
        {
            Some(row - 3)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy)]
pub(super) enum Divider {
    Sidebar,
    Preview,
    Dual,
    Terminal,
}

fn percent(width: u16, ratio: u16) -> u16 {
    (u32::from(width) * u32::from(ratio) / 100) as u16
}
pub(super) fn dual_split(width: u16, ratio: u16) -> u16 {
    percent(width, ratio).clamp(16, width.saturating_sub(17).max(16))
}

impl State {
    pub(super) fn panel_rows(&self, h: u16) -> u16 {
        if cfg!(feature = "native-ops") && self.prefs.terminal_visible && h >= 16 {
            percent(h, self.prefs.terminal_percent).clamp(5, h - 9)
        } else {
            0
        }
    }

    // Single-pane layout keeps the status strip at the bottom, and the
    // sidebar beside the shell. Dual pane retains its full-width console.
    pub(super) fn terminal_rect(&self, full_h: u16, l: Layout) -> (u16, u16, u16, u16) {
        let rows = self.panel_rows(full_h);
        let left = if self.dual { 0 } else { l.first() };
        let top = if self.dual || rows == 0 {
            full_h - rows
        } else {
            full_h - rows - 3
        };
        (left, top, l.width.saturating_sub(left), rows)
    }

    pub(super) fn layout(&self, w: u16, h: u16) -> Layout {
        let mut layout = Layout::new(w, h, self.preview_enabled);
        if !self.prefs.sidebar {
            layout.sidebar = 0;
        } else if layout.sidebar > 0 {
            layout.sidebar = self.prefs.sidebar_width.min(layout.width / 3);
        }
        if layout.preview > 0 {
            layout.preview = percent(layout.width, self.prefs.preview_percent);
        }
        layout
    }

    pub(super) fn resize_drag(
        &mut self,
        divider: Divider,
        x: u16,
        width: u16,
        y: u16,
        height: u16,
    ) {
        let ratio = if width == 0 {
            50
        } else {
            (u32::from(x.min(width)) * 100 / u32::from(width)) as u16
        };
        match divider {
            Divider::Sidebar => self.prefs.sidebar_width = x.clamp(12, 48),
            Divider::Preview => self.prefs.preview_percent = (100 - ratio).clamp(18, 45),
            Divider::Dual => self.prefs.dual_percent = ratio.clamp(25, 75),
            Divider::Terminal => {
                if height > 0 {
                    let top = if self.dual { y } else { y.saturating_add(3) };
                    self.prefs.terminal_percent = (u32::from(height.saturating_sub(top)) * 100
                        / u32::from(height))
                    .clamp(20, 60) as u16;
                }
            }
        }
    }
}
