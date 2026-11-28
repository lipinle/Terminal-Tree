//! Searchable action menu; labels are display text, never shell commands.
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    queue,
    style::{Color, Print, ResetColor, SetBackgroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};

fn matches(label: &str, query: &str) -> bool {
    let label = label.to_ascii_lowercase();
    query.split_whitespace().all(|word| {
        let word = word.to_ascii_lowercase();
        if label.contains(&word) {
            return true;
        }
        let mut chars = label.chars();
        word.chars().all(|c| chars.by_ref().any(|x| x == c))
    })
}

pub fn choose(labels: &[String]) -> io::Result<Option<usize>> {
    let mut query = String::new();
    let mut selected = 0usize;
    let mut top = 0usize;
    loop {
        let needle = query.trim().to_ascii_lowercase();
        let direct = labels
            .iter()
            .any(|label| label.to_ascii_lowercase().contains(&needle));
        let rows: Vec<_> = labels
            .iter()
            .enumerate()
            .filter(|(_, label)| {
                if direct {
                    label.to_ascii_lowercase().contains(&needle)
                } else {
                    matches(label, &query)
                }
            })
            .collect();
        selected = selected.min(rows.len().saturating_sub(1));
        let (w, h) = terminal::size()?;
        let width = usize::from(w.saturating_sub(1));
        let capacity = usize::from(h.saturating_sub(3)).max(1);
        if selected < top {
            top = selected;
        }
        if selected >= top + capacity {
            top = selected + 1 - capacity;
        }
        top = top.min(rows.len().saturating_sub(capacity));
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, 0),
            Clear(ClearType::All),
            Print(super::clip("Command Palette | Type to search", width))
        )?;
        if h >= 2 {
            let safe = terminal_tree::display_name(std::ffi::OsStr::new(&query));
            queue!(
                out,
                MoveTo(0, 1),
                Print(super::clip(
                    &format!("Search: {safe} | {} actions", rows.len()),
                    width
                ))
            )?;
        }
        if h >= 4 {
            for (row, (_, label)) in rows.iter().skip(top).take(capacity).enumerate() {
                queue!(out, MoveTo(0, row as u16 + 2))?;
                if top + row == selected {
                    queue!(out, SetBackgroundColor(Color::DarkGrey))?;
                }
                queue!(out, Print(super::clip(label, width)), ResetColor)?;
            }
            queue!(
                out,
                MoveTo(0, h - 1),
                Print(super::clip("Up/Down | Enter run | Esc cancel", width))
            )?;
        }
        out.flush()?;
        drop(out);
        match event::read()? {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if k.code == KeyCode::Esc
                    || (k.code == KeyCode::Char('c') && k.modifiers.contains(KeyModifiers::CONTROL))
                {
                    return Ok(None);
                }
                match k.code {
                    KeyCode::Enter if k.kind == KeyEventKind::Press => {
                        return Ok(rows.get(selected).map(|(i, _)| *i))
                    }
                    KeyCode::Up => selected = selected.saturating_sub(1),
                    KeyCode::Down => selected = (selected + 1).min(rows.len().saturating_sub(1)),
                    KeyCode::PageUp => selected = selected.saturating_sub(capacity),
                    KeyCode::PageDown => {
                        selected = (selected + capacity).min(rows.len().saturating_sub(1))
                    }
                    KeyCode::Backspace => {
                        query.pop();
                        selected = 0;
                        top = 0;
                    }
                    KeyCode::Char('a') if k.modifiers == KeyModifiers::CONTROL => {
                        query.clear();
                        selected = 0;
                        top = 0;
                    }
                    KeyCode::Char(c)
                        if !c.is_control()
                            && !k
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && query.len() + c.len_utf8() <= 128 =>
                    {
                        query.push(c);
                        selected = 0;
                        top = 0;
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => selected = selected.saturating_sub(1),
                MouseEventKind::ScrollDown => {
                    selected = (selected + 1).min(rows.len().saturating_sub(1))
                }
                MouseEventKind::Down(MouseButton::Left)
                    if h >= 4 && mouse.row >= 2 && mouse.row < h - 1 =>
                {
                    if let Some((index, _)) = rows.get(top + usize::from(mouse.row - 2)) {
                        return Ok(Some(*index));
                    }
                }
                _ => (),
            },
            _ => (),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn search_matches_words_and_subsequences() {
        assert!(matches("View / Toggle live preview", "live prev"));
        assert!(matches("Settings / Preferences", "stng"));
        assert!(!matches("Edit / Copy", "delete"));
        assert!(matches("Go / Bookmarks", ""));
    }
}
