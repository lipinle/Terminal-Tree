//! Read-only dual-pane tree navigation. Pane state never authorizes writes.
use crate::{clip, Terminal};
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use terminal_tree::{
    display_name,
    layout::{Layout, Pane},
    tree::TreeModel,
    Order,
};

const TAB_LIMIT: usize = 4;

#[derive(Clone, Copy)]
struct Region {
    pane: Pane,
    x: u16,
    width: u16,
    start: usize,
    end: usize,
}

impl Region {
    fn hit(self, column: u16, row: u16, height: u16) -> Option<usize> {
        if column < self.x
            || column >= self.x + self.width
            || row < 2
            || row >= height.saturating_sub(2)
        {
            return None;
        }
        let index = self.start + usize::from(row - 2);
        (index < self.end).then_some(index)
    }
}

fn regions(width: u16, height: u16) -> Option<[Region; 2]> {
    if width < 40 || height < 7 {
        return None;
    }
    let left_width = (width - 2) / 2;
    let right_x = left_width + 1;
    Some([
        Region {
            pane: Pane::Left,
            x: 0,
            width: left_width,
            start: 0,
            end: 0,
        },
        Region {
            pane: Pane::Right,
            x: right_x,
            width: width - right_x - 1,
            start: 0,
            end: 0,
        },
    ])
}

fn tree(path: &Path) -> io::Result<TreeModel> {
    TreeModel::new(path, false, Order::Name)
}

fn draw(
    layout: &mut Layout<TreeModel>,
    message: &str,
    width: u16,
    height: u16,
) -> io::Result<Option<[Region; 2]>> {
    let mut out = io::stdout().lock();
    let limit = usize::from(width.saturating_sub(1));
    queue!(out, MoveTo(0, 0), Clear(ClearType::All))?;
    let Some(mut regions) = regions(width, height) else {
        queue!(out, Print(clip("Resize to at least 40x7; q exits", limit)))?;
        out.flush()?;
        return Ok(None);
    };
    let focus = layout.focused_pane();
    queue!(
        out,
        Print(clip(
            &format!(
                "Terminal Tree [panes / read-only] | Tab {}/{} | Focus: {:?} | Row: {}",
                layout.active() + 1,
                layout.tab_count(),
                focus,
                if layout.focused().rows().is_empty() {
                    0
                } else {
                    layout.focused().selected() + 1
                }
            ),
            limit
        ))
    )?;
    for region in &mut regions {
        let model = layout.pane_mut(region.pane);
        let width = usize::from(region.width);
        let active = region.pane == focus;
        queue!(
            out,
            MoveTo(region.x, 1),
            SetForegroundColor(if active { Color::Cyan } else { Color::White }),
            Print(clip(
                &format!("{:?}: {}", region.pane, display_name(model.root().as_os_str())),
                width
            )),
            ResetColor
        )?;
        (region.start, region.end) = model.viewport(usize::from(height - 4));
        for (row, index) in (region.start..region.end).enumerate() {
            let item = &model.rows()[index];
            let marker = if item.entry.symlink {
                "@"
            } else if item.expanded {
                "-"
            } else if item.entry.directory {
                "+"
            } else {
                "."
            };
            queue!(out, MoveTo(region.x, (row + 2) as u16))?;
            if index == model.selected() {
                queue!(
                    out,
                    SetBackgroundColor(if active { Color::DarkGrey } else { Color::Black }),
                    SetForegroundColor(Color::White)
                )?;
            }
            queue!(
                out,
                Print(clip(
                    &format!(
                        "{}{} {}",
                        "  ".repeat(item.depth - 1),
                        marker,
                        item.entry.name
                    ),
                    width
                )),
                ResetColor
            )?;
        }
    }
    queue!(
        out,
        MoveTo(0, height - 2),
        Print(clip(message, limit)),
        MoveTo(0, height - 1),
        Print(clip(
            "Tab focus | n new | [ ] switch | w close | arrows navigate | F5 refresh | q exit",
            limit
        ))
    )?;
    out.flush()?;
    Ok(Some(regions))
}

fn new_tab(layout: &mut Layout<TreeModel>) -> io::Result<()> {
    if layout.tab_count() >= TAB_LIMIT {
        return Err(io::Error::other("tab limit reached"));
    }
    let left = tree(layout.pane(Pane::Left).root())?;
    let right = tree(layout.pane(Pane::Right).root())?;
    if !layout.add(left, right) {
        return Err(io::Error::other("tab limit reached"));
    }
    Ok(())
}

fn toggle(model: &mut TreeModel) -> io::Result<()> {
    if model
        .rows()
        .get(model.selected())
        .is_some_and(|row| row.expanded)
    {
        model.collapse_or_parent();
        Ok(())
    } else {
        model.expand()
    }
}

fn navigate(model: &mut TreeModel, code: KeyCode, capacity: usize) -> io::Result<()> {
    match code {
        KeyCode::Up => model.shift(-1),
        KeyCode::Down => model.shift(1),
        KeyCode::PageUp => model.shift(-(capacity as isize)),
        KeyCode::PageDown => model.shift(capacity as isize),
        KeyCode::Home => model.select(0),
        KeyCode::End => model.select(usize::MAX),
        KeyCode::Right => model.expand()?,
        KeyCode::Left => model.collapse_or_parent(),
        KeyCode::Enter => toggle(model)?,
        KeyCode::F(5) => model.refresh()?,
        _ => {}
    }
    Ok(())
}

pub(crate) fn browse(left: &Path, right: &Path) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other("Interactive terminal required"));
    }
    let mut layout = Layout::new(tree(left)?, tree(right)?, TAB_LIMIT)
        .ok_or_else(|| io::Error::other("invalid tab limit"))?;
    let _terminal = Terminal::enter()?;
    let mut message = String::from("Read-only panes. File mutations disabled.");
    loop {
        let (width, height) = terminal::size()?;
        let regions = draw(&mut layout, &message, width, height)?;
        let result = match event::read()? {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if key.code == KeyCode::Esc
                    || key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    break;
                }
                if regions.is_none() {
                    continue;
                }
                match key.code {
                    KeyCode::Tab => {
                        layout.toggle_focus();
                        Ok(())
                    }
                    KeyCode::Char('n') => new_tab(&mut layout),
                    KeyCode::Char('[') | KeyCode::Char(']') => {
                        let count = layout.tab_count();
                        let offset = if key.code == KeyCode::Char('[') {
                            count - 1
                        } else {
                            1
                        };
                        layout.switch((layout.active() + offset) % count);
                        Ok(())
                    }
                    KeyCode::Char('w') => {
                        if layout.close(layout.active()) {
                            Ok(())
                        } else {
                            Err(io::Error::other("cannot close the last tab"))
                        }
                    }
                    _ => navigate(layout.focused_mut(), key.code, usize::from(height - 4)),
                }
            }
            Event::Mouse(mouse) => {
                if terminal::size()? != (width, height) {
                    continue;
                }
                if let Some(regions) = regions {
                    if let MouseEventKind::Down(MouseButton::Left | MouseButton::Right) = mouse.kind
                    {
                        for region in regions {
                            if let Some(index) = region.hit(mouse.column, mouse.row, height) {
                                layout.focus(region.pane);
                                layout.focused_mut().select(index);
                                break;
                            }
                        }
                    }
                }
                Ok(())
            }
            _ => Ok(()),
        };
        message = match result {
            Ok(()) => String::from("Read-only panes. File mutations disabled."),
            Err(error) => display_name(OsStr::new(&error.to_string())),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pane_geometry_has_no_overlapping_or_out_of_bounds_hits() {
        assert!(regions(39, 24).is_none());
        assert!(regions(80, 6).is_none());
        for width in [40, 41, 80, 81, u16::MAX] {
            let [mut left, mut right] = regions(width, 10).unwrap();
            left.start = 4;
            left.end = 6;
            right.start = 8;
            right.end = 9;
            assert_eq!(left.hit(0, 2, 10), Some(4));
            assert_eq!(left.hit(left.width, 2, 10), None);
            assert_eq!(right.hit(right.x, 2, 10), Some(8));
            assert_eq!(right.hit(right.x, 3, 10), None);
            assert_eq!(left.hit(0, 1, 10), None);
            assert_eq!(right.hit(width - 1, 2, 10), None);
        }
    }
}
