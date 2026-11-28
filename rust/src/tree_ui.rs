//! Separate read-only navigator: never hands descendant rows to the write workspace.
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
    tree::{TreeModel, MAX_DEPTH},
    Order,
};

struct Frame {
    width: u16,
    height: u16,
    start: usize,
    end: usize,
}

impl Frame {
    fn capacity(&self) -> usize {
        usize::from(self.height.saturating_sub(4)).max(1)
    }

    fn hit(&self, row: u16, column: u16) -> Option<usize> {
        if self.height < 5 || row == 0 || column >= self.width.saturating_sub(1) {
            return None;
        }
        let index = self.start + usize::from(row - 1);
        (row <= self.height - 4 && index < self.end).then_some(index)
    }
}

fn draw(model: &mut TreeModel, message: &str) -> io::Result<Frame> {
    let (width, height) = terminal::size()?;
    let limit = usize::from(width.saturating_sub(1));
    let mut frame = Frame {
        width,
        height,
        start: 0,
        end: 0,
    };
    let mut out = io::stdout().lock();
    queue!(
        out,
        MoveTo(0, 0),
        Clear(ClearType::All),
        SetForegroundColor(Color::Cyan),
        Print(clip(
            &format!(
                "Terminal Tree [tree / read-only] | {}",
                display_name(model.root().as_os_str())
            ),
            limit
        )),
        ResetColor
    )?;
    if height >= 5 {
        (frame.start, frame.end) = model.viewport(frame.capacity());
        for (row, index) in (frame.start..frame.end).enumerate() {
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
            queue!(out, MoveTo(0, (row + 1) as u16))?;
            if index == model.selected() {
                queue!(out, SetBackgroundColor(Color::DarkGrey))?;
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
                    limit
                )),
                ResetColor
            )?;
        }
        let selection = if model.rows().is_empty() {
            0
        } else {
            model.selected() + 1
        };
        queue!(
            out,
            MoveTo(0, height - 3),
            Print(clip(
                &format!(
                    "Depth: {} | Rows: {} | Selected: {}",
                    model.depth_limit(),
                    model.rows().len(),
                    selection
                ),
                limit
            )),
            MoveTo(0, height - 2),
            Print(clip(message, limit)),
            MoveTo(0, height - 1),
            Print(clip(
                "Arrows navigate | +/- depth | F5 refresh | i details | F1 help | q exit",
                limit
            ))
        )?;
    }
    out.flush()?;
    Ok(frame)
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

fn selected_details(model: &TreeModel) -> io::Result<Vec<String>> {
    let row = model
        .rows()
        .get(model.selected())
        .ok_or_else(|| io::Error::other("no selected tree row"))?;
    let entry = &row.entry;
    let kind = if entry.symlink {
        "Symbolic link"
    } else if entry.directory {
        "Directory"
    } else {
        "Non-directory"
    };
    Ok(vec![
        "Cached scan information; press F5 in the tree to refresh.".into(),
        format!("Name: {}", entry.name),
        format!("Path: {}", display_name(entry.path.as_os_str())),
        format!("Kind: {kind}"),
        format!("Observed size: {} bytes", entry.size),
        format!("Depth: {}", row.depth),
        format!("Expanded: {}", row.expanded),
        "Read-only; this page does not reopen the selected path.".into(),
    ])
}

pub(crate) fn browse(path: &Path) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other("Interactive terminal required"));
    }
    let mut model = TreeModel::new(path, false, Order::Name)?;
    let _terminal = Terminal::enter()?;
    let mut message = String::from("Read-only tree. File mutations disabled.");
    loop {
        let frame = draw(&mut model, &message)?;
        let result = match event::read()? {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => break,
                    KeyCode::Up => {
                        model.shift(-1);
                        Ok(())
                    }
                    KeyCode::Down => {
                        model.shift(1);
                        Ok(())
                    }
                    KeyCode::PageUp => {
                        model.shift(-(frame.capacity() as isize));
                        Ok(())
                    }
                    KeyCode::PageDown => {
                        model.shift(frame.capacity() as isize);
                        Ok(())
                    }
                    KeyCode::Home => {
                        model.select(0);
                        Ok(())
                    }
                    KeyCode::End => {
                        model.select(usize::MAX);
                        Ok(())
                    }
                    KeyCode::Right => model.expand(),
                    KeyCode::Enter => toggle(&mut model),
                    KeyCode::Left => {
                        model.collapse_or_parent();
                        Ok(())
                    }
                    KeyCode::Char('+') => {
                        model.set_depth_limit((model.depth_limit() + 1).min(MAX_DEPTH))
                    }
                    KeyCode::Char('-') => {
                        model.set_depth_limit(model.depth_limit().saturating_sub(1).max(1))
                    }
                    KeyCode::F(5) => model.refresh(),
                    KeyCode::Char('i') if key.modifiers == KeyModifiers::NONE => {
                        match selected_details(&model).and_then(|lines| {
                            crate::page_with_interrupt("Tree details", &lines, true)
                        }) {
                            Ok(true) => break,
                            Ok(false) => Ok(()),
                            Err(error) => Err(error),
                        }
                    }
                    KeyCode::F(1) if key.modifiers == KeyModifiers::NONE => {
                        let lines = [
                            "Up/Down, PgUp/PgDn, Home/End: select a row",
                            "Right: expand | Left: collapse or select parent",
                            "Enter or left click: toggle | Right click: select",
                            "+/-: depth limit | F5: refresh",
                            "i: cached details | q/Esc: exit | Ctrl+C: interrupt",
                            "Read-only tree; no mutation shortcuts.",
                        ]
                        .map(String::from);
                        match crate::page_with_interrupt("Tree help", &lines, true) {
                            Ok(true) => break,
                            Ok(false) => Ok(()),
                            Err(error) => Err(error),
                        }
                    }
                    _ => Ok(()),
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => {
                    model.shift(-1);
                    Ok(())
                }
                MouseEventKind::ScrollDown => {
                    model.shift(1);
                    Ok(())
                }
                MouseEventKind::Down(MouseButton::Left | MouseButton::Right) => {
                    // A resize may make the last rendered row map stale.
                    if terminal::size()? != (frame.width, frame.height) {
                        continue;
                    }
                    if let Some(index) = frame.hit(mouse.row, mouse.column) {
                        model.select(index);
                        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                            toggle(&mut model)
                        } else {
                            Ok(())
                        }
                    } else {
                        Ok(())
                    }
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        };
        message = match result {
            Ok(()) => String::from("Read-only tree. File mutations disabled."),
            Err(error) => display_name(OsStr::new(&error.to_string())),
        };
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_testing_excludes_header_footer_and_empty_rows() {
        let frame = Frame {
            width: 80,
            height: 10,
            start: 7,
            end: 9,
        };
        assert_eq!(frame.hit(1, 0), Some(7));
        assert_eq!(frame.hit(2, 78), Some(8));
        assert_eq!(frame.hit(0, 0), None);
        assert_eq!(frame.hit(1, 79), None);
        assert_eq!(frame.hit(3, 0), None);
        assert_eq!(frame.hit(7, 0), None);
    }
}
