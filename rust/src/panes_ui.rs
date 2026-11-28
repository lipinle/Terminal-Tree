//! Read-only dual-pane tree navigation. Pane state never authorizes writes.
use crate::{clip, Terminal};
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind},
    queue,
    style::{Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType},
};
use std::ffi::OsStr;
use std::io::{self, IsTerminal, Write};
use std::path::Path;
use terminal_tree::{
    bookmarks::Bookmarks,
    display_name,
    keymap::{Action, Binding, Keymap},
    layout::{Layout, Pane},
    theme::Theme,
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
    theme: Theme,
    snapshot_enabled: bool,
) -> io::Result<Option<[Region; 2]>> {
    let mut out = io::stdout().lock();
    let limit = usize::from(width.saturating_sub(1));
    let palette = theme.palette();
    queue!(
        out,
        SetBackgroundColor(palette.background),
        SetForegroundColor(palette.text),
        MoveTo(0, 0),
        Clear(ClearType::All)
    )?;
    let Some(mut regions) = regions(width, height) else {
        queue!(
            out,
            Print(clip("Resize to at least 40x7; q exits", limit)),
            ResetColor
        )?;
        out.flush()?;
        return Ok(None);
    };
    let focus = layout.focused_pane();
    queue!(
        out,
        Print(clip(
            &format!(
                "Terminal Tree [panes / read-only] | Tab {}/{} | Focus: {:?} | Row: {} | Theme: {}",
                layout.active() + 1,
                layout.tab_count(),
                focus,
                if layout.focused().rows().is_empty() {
                    0
                } else {
                    layout.focused().selected() + 1
                },
                theme.name()
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
            SetForegroundColor(if active {
                palette.heading
            } else {
                palette.text
            }),
            Print(clip(
                &format!(
                    "{:?}: {}",
                    region.pane,
                    display_name(model.root().as_os_str())
                ),
                width
            )),
            SetForegroundColor(palette.text)
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
                    SetBackgroundColor(palette.selected_background),
                    SetForegroundColor(palette.selected_text)
                )?;
            } else {
                let color = if item.entry.symlink {
                    palette.link
                } else if item.entry.directory {
                    palette.directory
                } else {
                    palette.text
                };
                queue!(out, SetForegroundColor(color))?;
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
                SetBackgroundColor(palette.background),
                SetForegroundColor(palette.text)
            )?;
        }
    }
    queue!(
        out,
        MoveTo(0, height - 2),
        Print(clip(message, limit)),
        MoveTo(0, height - 1),
        Print(clip(
            if snapshot_enabled {
                "Tab focus | n new | [ ] switch | w close | b/B bookmarks | f/F find | p/F8/I/A preview | S export snapshot | ? keys | F1 help | t theme | q exit"
            } else {
                "Tab focus | n new | [ ] switch | w close | b/B bookmarks | f/F find | p/F8/I/A preview | ? keys | F1 help | t theme | q exit"
            },
            limit
        )),
        ResetColor
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

fn navigate(model: &mut TreeModel, action: Action, capacity: usize) -> io::Result<()> {
    match action {
        Action::Up => model.shift(-1),
        Action::Down => model.shift(1),
        Action::PageUp => model.shift(-(capacity as isize)),
        Action::PageDown => model.shift(capacity as isize),
        Action::First => model.select(0),
        Action::Last => model.select(usize::MAX),
        Action::Expand => model.expand()?,
        Action::Collapse => model.collapse_or_parent(),
        Action::Refresh => model.refresh()?,
    }
    Ok(())
}

fn save_location(model: &TreeModel, marks: &mut Bookmarks) -> io::Result<()> {
    let path = match model.rows().get(model.selected()) {
        Some(row) if row.entry.directory && !row.entry.symlink => row.entry.path.as_path(),
        _ => model.root(),
    };
    // Saved locations are read-only path values; refuse observed aliases.
    if path.canonicalize()? != path || !std::fs::symlink_metadata(path)?.is_dir() {
        return Err(io::Error::other("bookmark requires an unchanged directory"));
    }
    marks
        .add(path.to_path_buf())
        .map_err(|error| io::Error::other(format!("cannot save bookmark: {error:?}")))
}

enum BookmarkChoice {
    Cancel,
    Exit,
    Open(usize),
}

fn bookmark_menu(marks: &mut Bookmarks) -> io::Result<BookmarkChoice> {
    let mut selected = 0usize;
    let mut top = 0usize;
    loop {
        if marks.paths().is_empty() {
            return Ok(BookmarkChoice::Cancel);
        }
        let (width, height) = terminal::size()?;
        let limit = usize::from(width.saturating_sub(1));
        let cap = usize::from(height.saturating_sub(2)).max(1);
        selected = selected.min(marks.paths().len() - 1);
        if selected < top {
            top = selected;
        } else if selected >= top + cap {
            top = selected + 1 - cap;
        }
        let mut out = io::stdout().lock();
        queue!(
            out,
            ResetColor,
            MoveTo(0, 0),
            Clear(ClearType::All),
            Print(clip("Bookmarks - session only", limit))
        )?;
        if height >= 3 {
            for (index, path) in marks.paths().iter().enumerate().skip(top).take(cap) {
                queue!(out, MoveTo(0, (index - top + 1) as u16))?;
                if index == selected {
                    queue!(
                        out,
                        SetBackgroundColor(crossterm::style::Color::DarkGrey),
                        SetForegroundColor(crossterm::style::Color::White)
                    )?;
                }
                queue!(
                    out,
                    Print(clip(&display_name(path.as_os_str()), limit)),
                    ResetColor
                )?;
            }
            queue!(
                out,
                MoveTo(0, height - 1),
                Print(clip(
                    "Up/Down | Enter open | J/K reorder | x remove | Esc back",
                    limit
                ))
            )?;
        }
        out.flush()?;
        drop(out);
        if let Event::Key(key) = event::read()? {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                return Ok(BookmarkChoice::Exit);
            }
            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(BookmarkChoice::Cancel),
                KeyCode::Enter if height >= 3 => return Ok(BookmarkChoice::Open(selected)),
                KeyCode::Char('x') if height >= 3 => {
                    marks
                        .remove(selected)
                        .map_err(|_| io::Error::other("invalid bookmark selection"))?;
                }
                KeyCode::Char('K') if selected > 0 => {
                    marks
                        .move_to(selected, selected - 1)
                        .map_err(|_| io::Error::other("invalid bookmark selection"))?;
                    selected -= 1;
                }
                KeyCode::Char('J') if selected + 1 < marks.paths().len() => {
                    marks
                        .move_to(selected, selected + 1)
                        .map_err(|_| io::Error::other("invalid bookmark selection"))?;
                    selected += 1;
                }
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => selected = (selected + 1).min(marks.paths().len() - 1),
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = marks.paths().len() - 1,
                _ => {}
            }
        }
    }
}

fn open_location(model: &mut TreeModel, marks: &Bookmarks, index: usize) -> io::Result<()> {
    let path = marks
        .get(index)
        .ok_or_else(|| io::Error::other("invalid bookmark selection"))?;
    if path.canonicalize()? != path || !std::fs::symlink_metadata(path)?.is_dir() {
        return Err(io::Error::other("bookmark location changed"));
    }
    // Construct replacement first. Failure preserves the old pane.
    let replacement = tree(path)?;
    *model = replacement;
    Ok(())
}

#[cfg(all(feature = "native-ops", target_os = "linux"))]
fn find_location(model: &mut TreeModel, content: bool) -> io::Result<()> {
    use terminal_tree::search::{
        content::{text, ContentLimits},
        filenames, SearchLimits,
    };

    let label = if content {
        "Find text (case-sensitive):"
    } else {
        "Find filename:"
    };
    let Some(query) = crate::workspace::prompt(label)? else {
        return Ok(());
    };
    if query.is_empty() {
        return Err(io::Error::other("search query must not be empty"));
    }
    let root = model.root().to_path_buf();
    let limits = SearchLimits {
        entries: 16_384,
        depth: 16,
        matches: 4096,
    };
    let (paths, title) = crate::worker::run_search(|progress| {
        progress.item(display_name(root.as_os_str()), 0)?;
        let result = if content {
            let report = text(
                &root,
                &query,
                false,
                ContentLimits {
                    traversal: limits,
                    file_bytes: 1_048_576,
                    total_bytes: 64 * 1_048_576,
                },
                progress.cancellation_flag(),
            )?;
            let title = format!(
                "Text search: {} matches; skipped {} large, {} binary, {} nonregular",
                report.matches.len(),
                report.skipped_large,
                report.skipped_binary,
                report.skipped_nonregular
            );
            (report.matches, title)
        } else {
            let entries = filenames(&root, &query, false, limits, progress.cancellation_flag())?;
            let title = format!("Filename search: {} matches", entries.len());
            (entries.into_iter().map(|entry| entry.path).collect(), title)
        };
        progress.cancelled()?;
        Ok(result)
    })?;
    if paths.is_empty() {
        return crate::page(&title, &["No matching entries".into()]);
    }
    let labels: Vec<_> = paths
        .iter()
        .map(|path| display_name(path.as_os_str()))
        .collect();
    if let Some(index) = crate::choose(&title, &labels)? {
        let path = &paths[index];
        let metadata = std::fs::symlink_metadata(path)?;
        let directory = if metadata.is_dir() && !metadata.file_type().is_symlink() {
            path.as_path()
        } else {
            path.parent()
                .ok_or_else(|| io::Error::other("missing result parent"))?
        };
        if !directory.starts_with(&root) || directory.canonicalize()? != directory {
            return Err(io::Error::other("search result location changed"));
        }
        let mut replacement = tree(directory)?;
        if let Some(index) = replacement
            .rows()
            .iter()
            .position(|row| row.entry.path == *path)
        {
            replacement.select(index);
        }
        *model = replacement;
    }
    Ok(())
}

fn pane_help(
    keymap: &Keymap,
    snapshot_enabled: bool,
    preferences_enabled: bool,
) -> io::Result<bool> {
    let mut lines = vec![
        "Navigation: use the configured bindings below.".into(),
        "Ctrl+P: eza, 7-Zip and trusted Git status tools (native Linux).".into(),
        keymap.summary(),
        "Tab: change focus | n: new tab | [ / ]: switch tabs | w: close tab".into(),
        "b: save bookmark | B: open bookmark menu | J/K reorder within menu".into(),
        "f: recursive filename search | F: recursive text search (native Linux)".into(),
        "Esc / Ctrl+C cancels a running search; select a result to open its location.".into(),
        "t: cycle theme | ?: show configured keys | F1: this guide".into(),
        "p / F3: text or automatic hex | F8: forced hex".into(),
        "I: PPM image | A: USTAR listing (native Linux only)".into(),
        "Previews require regular non-link files. No file mutations.".into(),
        "q / Esc: close this page | Ctrl+C: exit navigator".into(),
    ];
    if snapshot_enabled {
        lines.push("S: explicitly export a new bookmark snapshot; never overwrite.".into());
    }
    if preferences_enabled {
        lines.push("T: explicitly save current theme and navigation keys to the private preferences directory.".into());
    }
    crate::page_with_interrupt("Pane help", &lines, true)
}

/// Preview the focused row without changing its selection or granting writes.
fn show_preview(model: &TreeModel, code: KeyCode) -> io::Result<bool> {
    #[cfg(all(feature = "native-ops", target_os = "linux"))]
    {
        use terminal_tree::{archive, image_preview, preview};

        let row = model
            .rows()
            .get(model.selected())
            .ok_or_else(|| io::Error::other("select a file to preview"))?;
        if row.entry.directory || row.entry.symlink {
            return Err(io::Error::other("preview requires a regular non-link file"));
        }
        let path = &row.entry.path;
        let (title, lines) = match code {
            KeyCode::Char('I') => {
                let (width, height) = terminal::size()?;
                let thumbnail = image_preview::native::thumbnail_file(
                    path,
                    usize::from(width.saturating_sub(1)).clamp(1, 80),
                    usize::from(height.saturating_sub(3)).clamp(1, 40),
                )?;
                let mut lines = vec![format!("PPM {}x{}", thumbnail.width, thumbnail.height)];
                lines.extend(thumbnail.rows);
                ("Image preview", lines)
            }
            KeyCode::Char('A') => {
                let members = archive::native::list_file(path)?;
                let mut lines = vec![format!("USTAR members: {}", members.len())];
                lines.extend(members.into_iter().map(|member| {
                    let kind = match member.kind {
                        archive::Kind::File => "file",
                        archive::Kind::Directory => "directory",
                    };
                    format!("{} {} bytes | {}", kind, member.size, member.name)
                }));
                ("Archive listing (read-only)", lines)
            }
            KeyCode::F(8) => (
                "File preview",
                preview::read_lines(path, preview::Mode::Hex)?,
            ),
            KeyCode::Char('p') | KeyCode::F(3) => (
                "File preview",
                preview::read_lines(path, preview::Mode::Auto)?,
            ),
            _ => return Err(io::Error::other("unsupported preview action")),
        };
        crate::page_with_interrupt(title, &lines, true)
    }
    #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
    {
        let _ = (model, code);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "file previews require the native Linux build",
        ))
    }
}

fn load_session(input: Option<&Path>, output: Option<&Path>) -> io::Result<Bookmarks> {
    #[cfg(all(feature = "native-ops", target_os = "linux"))]
    {
        let _ = output;
        match input {
            Some(path) => terminal_tree::bookmarks::snapshot::load(path),
            None => Ok(Bookmarks::default()),
        }
    }
    #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
    {
        if input.is_some() || output.is_some() {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "bookmark snapshots require the native Linux build",
            ));
        }
        Ok(Bookmarks::default())
    }
}

fn export_session(output: Option<&Path>, marks: &Bookmarks) -> io::Result<()> {
    let path = output.ok_or_else(|| io::Error::other("set --bookmarks-out to export"))?;
    #[cfg(all(feature = "native-ops", target_os = "linux"))]
    {
        terminal_tree::bookmarks::snapshot::save_new(path, marks)
    }
    #[cfg(not(all(feature = "native-ops", target_os = "linux")))]
    {
        let _ = (path, marks);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "bookmark snapshots require the native Linux build",
        ))
    }
}

pub(crate) fn browse(
    left: &Path,
    right: &Path,
    keymap: Option<Keymap>,
    theme: Option<Theme>,
    preferences: Option<&Path>,
    snapshot_in: Option<&Path>,
    snapshot_out: Option<&Path>,
) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::other("Interactive terminal required"));
    }
    #[cfg(not(feature = "native-ops"))]
    if preferences.is_some() {
        return Err(io::Error::other(
            "pane preferences require native Linux support",
        ));
    }
    #[cfg(feature = "native-ops")]
    let store = preferences.map(super::settings::Store::new).transpose()?;
    #[cfg(feature = "native-ops")]
    let (saved_theme, saved_keys) = store
        .as_ref()
        .map(|store| store.load_panes())
        .transpose()?
        .flatten()
        .unwrap_or_default();
    #[cfg(not(feature = "native-ops"))]
    let (saved_theme, saved_keys) = (Theme::default(), Keymap::default());
    let keymap = keymap.unwrap_or(saved_keys);
    let mut theme = theme.unwrap_or(saved_theme);
    let mut marks = load_session(snapshot_in, snapshot_out)?;
    let mut layout = Layout::new(tree(left)?, tree(right)?, TAB_LIMIT)
        .ok_or_else(|| io::Error::other("invalid tab limit"))?;
    let _terminal = Terminal::enter()?;
    let idle = if snapshot_out.is_some() {
        "Read-only panes. S exports bookmark snapshot only."
    } else {
        "Read-only panes. File mutations disabled."
    };
    let mut message = String::from(idle);
    loop {
        let (width, height) = terminal::size()?;
        let regions = draw(
            &mut layout,
            &message,
            width,
            height,
            theme,
            snapshot_out.is_some(),
        )?;
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
                if key.code == KeyCode::Char('?') {
                    message = keymap.summary();
                    continue;
                }
                match key.code {
                    KeyCode::F(1) if key.modifiers == KeyModifiers::NONE => {
                        match pane_help(&keymap, snapshot_out.is_some(), preferences.is_some()) {
                            Ok(true) => break,
                            Ok(false) => Ok(()),
                            Err(error) => Err(error),
                        }
                    }
                    code @ (KeyCode::Char('p' | 'I' | 'A') | KeyCode::F(3 | 8))
                        if key.kind == KeyEventKind::Press
                            && (key.modifiers == KeyModifiers::NONE
                                || key.modifiers == KeyModifiers::SHIFT) =>
                    {
                        match show_preview(layout.focused(), code) {
                            Ok(true) => break,
                            Ok(false) => Ok(()),
                            Err(error) => Err(error),
                        }
                    }
                    #[cfg(feature = "native-ops")]
                    KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                        let model = layout.focused();
                        let selected = model.rows().get(model.selected()).map(|r| &r.entry);
                        let directory = match selected {
                            Some(entry) if entry.directory && !entry.symlink => entry.path.as_path(),
                            Some(entry) => entry.path.parent().unwrap_or(model.root()),
                            None => model.root(),
                        };
                        crate::tools_ui::show(directory, selected.map(|e| e.path.as_path()))
                    }
                    KeyCode::Tab => {
                        layout.toggle_focus();
                        Ok(())
                    }
                    KeyCode::Char('n') => new_tab(&mut layout),
                    #[cfg(all(feature = "native-ops", target_os = "linux"))]
                    KeyCode::Char('f' | 'F') if key.kind == KeyEventKind::Press => {
                        find_location(layout.focused_mut(), key.code == KeyCode::Char('F'))
                    }
                    KeyCode::Char('b') => match save_location(layout.focused(), &mut marks) {
                        Ok(()) => {
                            message = format!("Bookmark saved | {} locations", marks.paths().len());
                            continue;
                        }
                        Err(error) => Err(error),
                    },
                    KeyCode::Char('B') => {
                        if marks.paths().is_empty() {
                            Err(io::Error::other("no bookmarks; press b to save"))
                        } else {
                            match bookmark_menu(&mut marks)? {
                                BookmarkChoice::Exit => break,
                                BookmarkChoice::Cancel => Ok(()),
                                BookmarkChoice::Open(index) => {
                                    open_location(layout.focused_mut(), &marks, index)
                                }
                            }
                        }
                    }
                    KeyCode::Char('S') if key.kind == KeyEventKind::Press => {
                        match export_session(snapshot_out, &marks) {
                            Ok(()) => {
                                message = String::from("Bookmark snapshot exported");
                                continue;
                            }
                            Err(error) => Err(error),
                        }
                    }
                    #[cfg(feature = "native-ops")]
                    KeyCode::Char('T') if key.kind == KeyEventKind::Press => {
                        if let Some(store) = &store {
                            match store.save_panes(theme, &keymap) {
                                Ok(()) => {
                                    message = String::from("Pane preferences saved");
                                    continue;
                                }
                                Err(error) => Err(error),
                            }
                        } else {
                            Err(io::Error::other(
                                "use --preferences DIR to enable explicit saves",
                            ))
                        }
                    }
                    KeyCode::Char('t') => {
                        theme = theme.next();
                        Ok(())
                    }
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
                    KeyCode::Enter => toggle(layout.focused_mut()),
                    _ => match keymap.action(Binding {
                        code: key.code,
                        modifiers: key.modifiers,
                    }) {
                        Some(action) => {
                            navigate(layout.focused_mut(), action, usize::from(height.saturating_sub(4)).max(1))
                        }
                        None => Ok(()),
                    },
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
            Ok(()) => String::from(idle),
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
