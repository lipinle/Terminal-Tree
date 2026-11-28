//! View modes, cell geometry and toolbar rendering.
use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Print, SetBackgroundColor, SetForegroundColor},
};
use std::io::{self, Write};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum View {
    #[default]
    Tree,
    Grid,
    List,
}

impl View {
    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Tree => "Tree",
            Self::Grid => "Grid",
            Self::List => "List",
        }
    }
}

const TOOLBAR: [(&str, &str); 5] = [
    ("Path", "[Path]"),
    ("Search", "[Search]"),
    ("Grid", "[Grid]"),
    ("List", "[List]"),
    ("Tree", "[Tree]"),
];

pub(super) fn preview_hit_row(height: u16, row: u16) -> bool {
    height >= 8 && row >= 3 && row < height.saturating_sub(3)
}

pub(super) fn toolbar_start(width: u16) -> Option<u16> {
    let length = TOOLBAR.iter().map(|(_, s)| s.len() + 1).sum::<usize>() as u16;
    (width >= length + 10).then(|| width - length)
}

pub(super) fn toolbar_hit(width: u16, column: u16) -> Option<usize> {
    let mut x = toolbar_start(width)?;
    for (i, (_, label)) in TOOLBAR.iter().enumerate() {
        let end = x + label.len() as u16;
        if column >= x && column < end {
            return Some(i);
        }
        x = end + 1;
    }
    None
}

#[derive(Clone, Copy)]
pub(super) struct Cells {
    pub(super) columns: usize,
    pub(super) width: u16,
    pub(super) stride: usize,
    pub(super) groups: usize,
}

impl Cells {
    pub(super) fn new(width: u16, height: usize, view: View, zoom: usize) -> Self {
        let stride = if view == View::Grid {
            (zoom + 2).min(height.max(1))
        } else {
            zoom.max(1).min(height.max(1))
        };
        let columns = if view == View::Grid {
            (usize::from(width) / (12 + zoom * 5)).max(1)
        } else {
            1
        };
        Self {
            columns,
            width: width / columns as u16,
            stride,
            groups: (height / stride).max(1),
        }
    }
    pub(super) fn capacity(self) -> usize {
        self.columns * self.groups
    }
    pub(super) fn hit(self, x: u16, y: usize) -> Option<usize> {
        if self.width == 0 {
            return None;
        }
        let col = usize::from(x / self.width);
        let row = y / self.stride;
        if col < self.columns && row < self.groups {
            row.checked_mul(self.columns)?.checked_add(col)
        } else {
            None
        }
    }
}

pub(super) fn draw_toolbar(
    out: &mut impl Write,
    width: u16,
    view: View,
    prefs: &crate::app_settings::Preferences,
) -> io::Result<u16> {
    let Some(start) = toolbar_start(width) else {
        return Ok(width);
    };
    let mut x = start;
    for (name, label) in TOOLBAR {
        queue!(out, MoveTo(x, 1))?;
        if name == view.name() {
            queue!(
                out,
                SetBackgroundColor(prefs.selection),
                SetForegroundColor(prefs.accent)
            )?;
        }
        queue!(
            out,
            Print(label),
            SetForegroundColor(prefs.foreground),
            SetBackgroundColor(prefs.background)
        )?;
        x += label.len() as u16 + 1;
    }
    Ok(start.saturating_sub(1))
}
