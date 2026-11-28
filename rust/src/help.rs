//! In-app command reference. Keep this in sync with the actual event handlers.
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    queue,
    style::{Color, Print, ResetColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

type Row = (&'static str, &'static str);
struct Topic {
    title: &'static str,
    rows: &'static [Row],
}
const TOPICS:&[Topic]=&[
    Topic{title:"Navigation",rows:&[
        ("Up / Down","Select the previous / next entry."),
        ("PgUp / PgDn","Move by one visible page."),
        ("Home / End","Select the first / last entry."),
        ("Left / Right","Go to parent / enter selected folder."),
        ("Enter","Enter a folder or open a regular file where a desktop opener is supported. Linux uses xdg-open / gio; macOS uses open. An external application may edit the file."),
        ("[ / ]","Go backward / forward in navigation history."),
        ("g","Go to a path (native build). Relative paths, absolute paths and ~/ are supported. Enter accepts; Esc cancels."),
        ("m / F4","My Computer: choose Home, existing home Trash, File System or mounted media. Discovery does not grant write access."),
        ("q / Esc / Ctrl+C","Exit the navigator outside a prompt or operation."),
        ("F1 / ?","Open this guide."),
        ("Shell navigation","In tt_rust_nav, Enter on a folder exits to Bash in that folder. Right continues browsing."),
    ]},
    Topic{title:"View, search and settings",rows:&[
        ("h","Toggle hidden entries."),
        ("s","Cycle sort: Name, Size, Date, Type. Directories remain first."),
        ("f / F5","Refresh from disk."),
        ("/","Filter the current directory by name; type to update results. Backspace removes text, Enter accepts, Esc restores the previous filter. Not recursive."),
        ("F7","Filter by entry type: All, Directories, non-directory non-links, or symbolic links."),
        ("i","Properties: path, mode, size, owner, modification time, inode and links."),
        ("p / F3","Preview a regular non-link file (native build); up to 64 KiB. NUL bytes in the first 64 KiB trigger hexadecimal preview; F8 handles other binary data."),
        ("F8","Force hexadecimal preview of a regular non-link file (native build)."),
        ("S","Save hidden, sort and type preferences for the current Rust workspace/personal journal. The name query is not saved."),
        ("Menus","Up / Down, PgUp / PgDn, Home / End; Enter selects, q / Esc cancels."),
        ("Read-only pages","Up / Down, PgUp / PgDn, Home / End; Enter, q or Esc closes properties, preview and recovery."),
    ]},
    Topic{title:"Mouse and selection",rows:&[
        ("Wheel up / down","Move selection up / down."),
        ("Left click","Select an entry; enter it if it is a folder."),
        ("Right click","Select an entry without entering."),
        ("Ctrl + left click","Toggle a mark on the clicked item (workspace/personal)."),
        ("Shift + right click","Open actions for the clicked selection (workspace/personal). Some terminals intercept modifier clicks; use F6 or Space instead."),
        ("Space","Toggle a mark on the selected item."),
        ("a / A","Mark all visible entries / clear all marks."),
        ("F6","Open the actions menu (workspace/personal); selecting an action still uses the normal safety checks."),
        ("Marks","Up to 1,000 items. Marked items take precedence over the cursor for batch actions. Directory changes clear marks; hidden, missing or changed marks are refused. Use A to clear them before renaming."),
    ]},
    Topic{title:"Copy, move and create",rows:&[
        ("c","Copy selection or marked items into the reviewed clipboard."),
        ("x","Cut selection or marked items into the reviewed clipboard. No move happens yet."),
        ("v","Paste into the current directory. Review paths and type yes."),
        ("r / F2","Rename one item; clear marks first. Enter a new name, review and type yes."),
        ("Actions: Bulk rename","Mark items, choose Bulk rename in F6, enter literal search and replacement, review every pair and type yes. Stops on first failure."),
        ("Actions: Keep both","For one clipboard item, choose Paste one item (keep both) in F6. Review the suggested unoccupied name and type yes. Never overwrites."),
        ("n","Create a directory: enter one name, review and type yes."),
        ("t","Create an empty file: enter one name, review and type yes."),
        ("Actions: Extract archive","Select one supported USTAR or gzip-compressed USTAR archive, review the layout, enter a new directory name and type yes. No overwrite or metadata restoration."),
        ("u","Undo the most recent eligible move/rename in this session. Review and type yes; changed results or occupied destinations are refused."),
        ("Example: copy","Select a file (or mark several), press c, navigate to the target, press v, review paths, then type yes."),
        ("Example: move","Use x instead of c. Cross-filesystem moves are refused. Existing destinations are never overwritten."),
        ("Clipboard","Copy/cut replaces the old clipboard and clears marks after successful review. A successful move or failed transfer clears the clipboard."),
        ("Batch","Each item commits separately. A failure stops the batch; already committed items remain. No automatic rollback."),
    ]},
    Topic{title:"Trash and permanent deletion",rows:&[
        ("d / Delete","Move selected or marked items to Trash. Review and type yes."),
        ("R","Restore selected or marked top-level Trash items to their original locations. Review and type yes."),
        ("D","Permanently delete selected or marked top-level Trash items. Review and type DELETE exactly. Irreversible."),
        ("Example: restore","Trash an item with d. Open the appropriate Trash/files directory using g (m / F4 may show home Trash). Select the item, press R, review and confirm."),
        ("Workspace Trash","Use the workspace's supplied data directory followed by Trash/files, not the real home Trash."),
        ("Personal Trash","Native home Trash must already exist with private permissions. Same-filesystem only."),
        ("Safety","No fallback to permanent deletion if Trash fails. Restore refuses occupied destinations. A failed permanent deletion can be partial."),
    ]},
    Topic{title:"Review, recovery and safety",rows:&[
        ("j","Open the read-only pending journal. Inspect source, destination and staging/metadata; pending does not mean failed."),
        ("Review paths","Up / Down scroll one line; PgUp / PgDn scroll a page. Enter advances to the end, then opens the confirmation prompt."),
        ("Confirmation","Type yes (or DELETE for permanent deletion). Enter submits, Backspace edits, Esc / Ctrl+C cancels. Anything other than the required word declines the operation."),
        ("During worker","Esc / Ctrl+C requests cancellation. It may wait for a safe checkpoint. Once commit begins, the current item must finish safely."),
        ("After interruption","Inspect j and the actual paths before retrying. Staging and pending records can remain. Recovery does not replay, delete or roll back automatically."),
        ("Undo limits","Undo is session-local and only for eligible moves/renames, not permanent deletion."),
        ("Write scope","Writes require an explicit workspace or personal mode and approval by the safety policy. Root, protected paths and unapproved mounts are refused."),
        ("Backups","A journal is not a backup. Keep independent copies of important files; never run as root."),
    ]},
    Topic{title:"Modes and shell commands",rows:&[
        ("Read-only","--browse PATH; navigator writes and settings save are disabled. External file opening can still modify files."),
        ("Workspace","--workspace ROOT JOURNAL DATA; private owned workspace, separate private journal and private data directory inside the workspace."),
        ("Personal","--personal JOURNAL [EXTRA_MOUNT ...]; explicit approved roots, existing private journal, real account HOME."),
        ("tt_rust_list","List a directory from Bash after sourcing preview integration.sh."),
        ("tt_rust_ls","Use optional eza for a directory listing, or the native list if unavailable. No eza output is used for file operations."),
        ("tt_rust_nav","Browse from the current directory, then select a directory to return to Bash."),
        ("tt_rust_workspace","Launch the scoped workspace with ROOT JOURNAL DATA."),
        ("tt_rust_personal","Launch personal mode with JOURNAL and optional approved mount roots."),
        ("Preview install","Rust preview is separate from Python; it does not replace existing Bash integration."),
    ]},
];

// Unicode-width wrapping with hard breaks for long words; never drop characters.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut out = Vec::new();
    let mut line = String::new();
    let mut used = 0;
    for word in text.split_whitespace() {
        let n = word.width();
        if !line.is_empty() && used + 1 + n > width {
            out.push(std::mem::take(&mut line));
            used = 0;
        }
        if !line.is_empty() {
            line.push(' ');
            used += 1;
        }
        for c in word.chars() {
            let n = c.width().unwrap_or(0);
            if used + n > width && !line.is_empty() {
                out.push(std::mem::take(&mut line));
                used = 0;
            }
            line.push(c);
            used += n;
        }
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}
fn layout(rows: &[Row], width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for &(key, description) in rows {
        if width >= 56 && key.width() <= 23 {
            let indent = 25;
            for (i, line) in wrap(description, width - indent).into_iter().enumerate() {
                if i == 0 {
                    lines.push(format!("{key:<23}  {line}"));
                } else {
                    lines.push(format!("{:indent$}{line}", ""));
                }
            }
        } else {
            lines.extend(wrap(key, width));
            let indent = if width >= 12 { 2 } else { 0 };
            lines.extend(
                wrap(description, width.saturating_sub(indent))
                    .into_iter()
                    .map(|s| format!("{:indent$}{s}", "")),
            );
            lines.push(String::new());
        }
    }
    lines
}
fn topic(t: &Topic) -> io::Result<()> {
    let mut top = 0usize;
    loop {
        let (w, h) = terminal::size()?;
        let width = usize::from(w).saturating_sub(1).max(1);
        let lines = layout(t.rows, width);
        let cap = usize::from(h).saturating_sub(3).max(1);
        top = top.min(lines.len().saturating_sub(cap));
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, 0),
            Clear(ClearType::All),
            SetForegroundColor(Color::Cyan),
            Print(super::clip(t.title, width)),
            ResetColor
        )?;
        if h >= 4 {
            for (i, line) in lines.iter().skip(top).take(cap).enumerate() {
                queue!(
                    out,
                    MoveTo(0, (i + 1) as u16),
                    Print(super::clip(line, width))
                )?;
            }
            queue!(
                out,
                MoveTo(0, h - 2),
                Print(super::clip(
                    &format!(
                        "Lines {}-{}/{}",
                        top + 1,
                        (top + cap).min(lines.len()),
                        lines.len()
                    ),
                    width
                )),
                MoveTo(0, h - 1),
                Print(super::clip("Scroll / PgUp PgDn | Esc back", width))
            )?;
        } else if h >= 2 {
            queue!(out, MoveTo(0, h - 1), Print(super::clip("Esc back", width)))?;
        }
        out.flush()?;
        drop(out);
        match event::read()? {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                    return Ok(());
                }
                match k.code {
                    KeyCode::Esc
                    | KeyCode::Char('q')
                    | KeyCode::Enter
                    | KeyCode::Left
                    | KeyCode::F(1) => return Ok(()),
                    KeyCode::Up => top = top.saturating_sub(1),
                    KeyCode::Down => top = top.saturating_add(1),
                    KeyCode::PageUp => top = top.saturating_sub(cap),
                    KeyCode::PageDown => top = top.saturating_add(cap),
                    KeyCode::Home => top = 0,
                    KeyCode::End => top = lines.len(),
                    _ => (),
                }
            }
            Event::Mouse(m) => match m.kind {
                MouseEventKind::ScrollUp => top = top.saturating_sub(1),
                MouseEventKind::ScrollDown => top = top.saturating_add(1),
                _ => (),
            },
            _ => (),
        }
    }
}
pub fn show(writable: bool, personal: bool, return_on_enter: bool) -> io::Result<()> {
    let mode = if personal {
        "personal"
    } else if writable {
        "workspace"
    } else if return_on_enter {
        "shell navigation"
    } else {
        "read-only"
    };
    let title = format!("Terminal Tree help | {mode}");
    let labels: Vec<_> = TOPICS
        .iter()
        .enumerate()
        .map(|(i, t)| format!("{}. {}", i + 1, t.title))
        .collect();
    loop {
        let Some(i) = super::choose(&title, &labels)? else {
            return Ok(());
        };
        topic(&TOPICS[i])?;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layouts_keep_text_visible_at_small_and_large_widths() {
        for width in [16, 24, 40, 56, 80, 120] {
            for t in TOPICS {
                let lines = layout(t.rows, width);
                assert!(lines.iter().all(|l| l.width() <= width));
                assert!(!lines.is_empty());
                let actual = lines
                    .join(" ")
                    .split_whitespace()
                    .collect::<Vec<_>>()
                    .join("");
                let expected = t
                    .rows
                    .iter()
                    .flat_map(|(k, d)| [k, d])
                    .flat_map(|s| s.split_whitespace())
                    .collect::<Vec<_>>()
                    .join("");
                assert_eq!(actual, expected);
            }
        }
    }
    #[test]
    fn every_command_group_exists() {
        let keys: Vec<_> = TOPICS
            .iter()
            .flat_map(|t| t.rows.iter().map(|r| r.0))
            .collect();
        for key in [
            "F1 / ?",
            "m / F4",
            "F6",
            "F7",
            "Space",
            "a / A",
            "c",
            "x",
            "v",
            "r / F2",
            "n",
            "t",
            "u",
            "d / Delete",
            "R",
            "D",
            "j",
            "S",
            "Ctrl + left click",
            "Shift + right click",
        ] {
            assert!(keys.contains(&key), "missing {key}");
        }
    }
}
