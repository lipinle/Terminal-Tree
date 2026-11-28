#![forbid(unsafe_code)]
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
        MouseButton, MouseEventKind,
    },
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
#[cfg(feature = "native-ops")]
use std::env;
mod cli;
use std::io::{self, IsTerminal, Write};
#[cfg(feature = "native-ops")]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use terminal_tree::{display_name, scan, KindFilter, Model, Order};
use unicode_width::UnicodeWidthChar;
mod app_settings;
mod command_palette;
mod dashboard;
mod dashboard_frame;
#[cfg(feature = "native-ops")]
mod addons_ui;
#[cfg(feature = "native-ops")]
mod visual_ui;
#[cfg(feature = "native-ops")]
mod native_graphics;
#[cfg(feature = "native-ops")]
mod x11_overlay;
#[cfg(feature = "native-ops")]
mod preview_worker;
mod edition;
mod file_icons;
mod help;
mod panes_ui;
mod places;
mod platform;
#[cfg(feature = "native-ops")]
mod recovery;
#[cfg(feature = "native-ops")]
mod settings;
mod tree_ui;
#[cfg(feature = "native-ops")]
mod terminal_panel;
#[cfg(feature = "native-ops")]
mod pty_shell;
#[cfg(feature = "native-ops")]
mod tools_ui;
#[cfg(feature = "native-ops")]
mod git_ui;
#[cfg(feature = "native-ops")]
mod worker;
#[cfg(feature = "native-ops")]
mod workspace;

fn clip(text: &str, limit: usize) -> String {
    let mut width = 0;
    text.chars()
        .take_while(|c| {
            let n = c.width().unwrap_or(0);
            if width + n > limit {
                return false;
            }
            width += n;
            true
        })
        .collect()
}

fn icon(directory: bool, symlink: bool) -> (&'static str, Color) {
    if symlink {
        ("↗", Color::Cyan)
    } else if directory {
        ("▸", Color::Blue)
    } else {
        ("·", Color::White)
    }
}

fn list(path: &Path, hidden: bool) -> io::Result<()> {
    let entries = scan(path, hidden, Order::Name, "")?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "Location: {}", display_name(path.as_os_str()))?;
    writeln!(out, "My Computer")?;
    let limit = terminal::size().map_or(20, |(_, h)| usize::from(h).saturating_sub(4).max(10));
    for (i, e) in entries.iter().take(limit).enumerate() {
        let branch = if i + 1 == entries.len() {
            "└──"
        } else {
            "├──"
        };
        writeln!(
            out,
            "{branch} {} {}",
            icon(e.directory, e.symlink).0,
            e.name
        )?;
    }
    if entries.len() > limit {
        writeln!(out, "└── … {} more", entries.len() - limit)?;
    }
    out.flush()
}

// Guard always restores terminal state on ordinary error or unwind.
struct Terminal;
impl Terminal {
    fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture, Hide)?;
        #[cfg(feature = "native-ops")]
        if x11_overlay::supported(){execute!(io::stdout(),crossterm::event::EnableFocusChange)?;}
        Ok(guard)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        #[cfg(feature = "native-ops")]
        if x11_overlay::supported(){let _=execute!(io::stdout(),crossterm::event::DisableFocusChange);}
        let _ = execute!(
            io::stdout(),
            ResetColor,
            DisableMouseCapture,
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn draw(
    model: &mut Model,
    message: &str,
    searching: bool,
    writable: bool,
    #[cfg(feature = "native-ops")] workspace: Option<&workspace::Workspace>,
) -> io::Result<()> {
    let (w, h) = terminal::size()?;
    let width = usize::from(w).saturating_sub(1);
    let mut out = io::stdout().lock();
    queue!(
        out,
        MoveTo(0, 0),
        Clear(ClearType::All),
        SetForegroundColor(Color::Cyan),
        Print(clip(
            &format!(
                "Terminal Tree [Rust / {}] | {}",
                if writable { "workspace" } else { "read-only" },
                display_name(model.directory.as_os_str())
            ),
            width
        )),
        ResetColor
    )?;
    if h < 5 {
        return out.flush();
    }
    let capacity = usize::from(h - 4);
    let (start, end) = model.viewport(capacity);
    for (row, index) in (start..end).enumerate() {
        let e = &model.entries[index];
        let (glyph, color) = icon(e.directory, e.symlink);
        #[cfg(feature = "native-ops")]
        let marked = workspace.is_some_and(|w| w.marked(&e.path));
        #[cfg(not(feature = "native-ops"))]
        let marked = false;
        queue!(out, MoveTo(0, (row + 1) as u16))?;
        if index == model.selected {
            queue!(out, SetBackgroundColor(Color::DarkGrey))?;
        }
        queue!(
            out,
            SetForegroundColor(color),
            Print(clip(
                &format!(
                    "{} {} {} {}",
                    if index + 1 == model.entries.len() {
                        "└──"
                    } else {
                        "├──"
                    },
                    if marked { "[x]" } else { "   " },
                    glyph,
                    e.name
                ),
                width
            )),
            ResetColor
        )?;
    }
    #[cfg(feature = "native-ops")]
    let marked_count = workspace.map_or(0, |w| w.marked_count());
    #[cfg(not(feature = "native-ops"))]
    let marked_count = 0;
    queue!(
        out,
        MoveTo(0, h - 3),
        Print(clip(
            &format!(
                "{}/{} | marked {} | sort {:?} | hidden {} | kind {:?} | filter: {}",
                if model.entries.is_empty() {
                    0
                } else {
                    model.selected + 1
                },
                model.entries.len(),
                marked_count,
                model.order,
                model.hidden,
                model.kind_filter,
                display_name(std::ffi::OsStr::new(&model.query))
            ),
            width
        )),
        MoveTo(0, h - 2),
        Print(clip(message, width)),
        MoveTo(0, h - 1),
        Print(clip(
            if searching {
                "Filter: type text | Enter accept | Esc cancel"
            } else if writable {
                "F1 help | Ctrl+P tools | F6 actions | Space mark | c copy | x cut | v paste | q exit"
            } else {
                "F1 help | ↑↓ select | ← parent | → enter | / filter | q exit | Ctrl+P tools"
            },
            width
        ))
    )?;
    out.flush()
}

fn page(title: &str, lines: &[String]) -> io::Result<()> {
    page_with_interrupt(title, lines, false).map(|_| ())
}

// True means Ctrl+C requested leaving the enclosing navigator.
fn page_with_interrupt(title: &str, lines: &[String], interrupt_exits: bool) -> io::Result<bool> {
    let mut top = 0usize;
    loop {
        let (w, h) = terminal::size()?;
        let width = usize::from(w).saturating_sub(1);
        let cap = usize::from(h).saturating_sub(2).max(1);
        top = top.min(lines.len().saturating_sub(cap));
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, 0),
            Clear(ClearType::All),
            Print(clip(title, width))
        )?;
        if h >= 3 {
            for (i, line) in lines.iter().skip(top).take(cap).enumerate() {
                queue!(out, MoveTo(0, (i + 1) as u16), Print(clip(line, width)))?;
            }
            queue!(
                out,
                MoveTo(0, h - 1),
                Print(clip("Up/Down | PgUp/PgDn | q/Esc close", width))
            )?;
        }
        out.flush()?;
        drop(out);
        if let Event::Key(k) = event::read()? {
            if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match k.code {
                KeyCode::Char('c')
                    if interrupt_exits && k.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    return Ok(true);
                }
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return Ok(false),
                KeyCode::Up => top = top.saturating_sub(1),
                KeyCode::Down => top = top.saturating_add(1),
                KeyCode::PageUp => top = top.saturating_sub(cap),
                KeyCode::PageDown => top = top.saturating_add(cap),
                KeyCode::Home => top = 0,
                KeyCode::End => top = lines.len(),
                _ => (),
            }
        }
    }
}
fn choose(title: &str, lines: &[String]) -> io::Result<Option<usize>> {
    if lines.is_empty() {
        return Ok(None);
    }
    let mut selected = 0usize;
    let mut top = 0usize;
    loop {
        let (w, h) = terminal::size()?;
        let width = usize::from(w).saturating_sub(1);
        let cap = usize::from(h).saturating_sub(2).max(1);
        if selected < top {
            top = selected;
        } else if selected >= top + cap {
            top = selected + 1 - cap;
        }
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, 0),
            Clear(ClearType::All),
            Print(clip(title, width))
        )?;
        if h >= 3 {
            for (i, line) in lines.iter().enumerate().skip(top).take(cap) {
                queue!(out, MoveTo(0, (i - top + 1) as u16))?;
                if i == selected {
                    queue!(out, SetBackgroundColor(Color::DarkGrey))?;
                }
                queue!(out, Print(clip(line, width)), ResetColor)?;
            }
            queue!(
                out,
                MoveTo(0, h - 1),
                Print(clip("Up/Down | Enter select | Esc cancel", width))
            )?;
        }
        out.flush()?;
        drop(out);
        let input = event::read()?;
        if let Event::Mouse(mouse) = input {
            match mouse.kind {
                MouseEventKind::ScrollUp => selected = selected.saturating_sub(1),
                MouseEventKind::ScrollDown => selected = (selected + 1).min(lines.len() - 1),
                MouseEventKind::Down(MouseButton::Left)
                    if h >= 3 && mouse.row > 0 && mouse.row < h - 1 =>
                {
                    let index = top + usize::from(mouse.row - 1);
                    if index < lines.len() {
                        return Ok(Some(index));
                    }
                }
                _ => (),
            }
        } else if let Event::Key(k) = input {
            if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            if k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c') {
                return Ok(None);
            }
            match k.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                KeyCode::Enter => return Ok(Some(selected)),
                KeyCode::Up => selected = selected.saturating_sub(1),
                KeyCode::Down => selected = (selected + 1).min(lines.len() - 1),
                KeyCode::PageUp => selected = selected.saturating_sub(cap),
                KeyCode::PageDown => selected = (selected + cap).min(lines.len() - 1),
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = lines.len() - 1,
                _ => (),
            }
        }
    }
}
#[cfg(feature = "native-ops")]
fn actions_menu(w: &mut workspace::Workspace, model: &mut Model) -> io::Result<Option<String>> {
    let actions = [
        ('c', "Copy"),
        ('x', "Cut"),
        ('v', "Paste"),
        ('r', "Rename"),
        ('n', "New directory"),
        ('t', "New file"),
        ('d', "Move to Trash"),
        ('R', "Restore"),
        ('D', "Permanent Delete"),
        ('u', "Undo move"),
        ('j', "Recovery"),
        ('S', "Save Rust view settings"),
        ('E', "Extract archive to new directory"),
        ('M', "Move one file across devices (retain original)"),
        ('N', "Bulk rename marked items"),
        ('V', "Paste one item (keep both)"),
    ];
    let labels: Vec<_> = actions.iter().map(|(_, label)| label.to_string()).collect();
    if let Some(i) = choose("Actions - writes require review", &labels)? {
        w.operate(actions[i].0, model).map(Some)
    } else {
        Ok(None)
    }
}
fn properties(path: &Path) -> io::Result<()> {
    page("Properties", &platform::properties(path)?)
}
#[cfg(feature = "native-ops")]
fn preview(path: &Path, mode: terminal_tree::preview::Mode) -> io::Result<()> {
    let lines = terminal_tree::preview::read_lines(path, mode)?;
    page("File preview", &lines)
}

fn browse(
    path: PathBuf,
    return_on_enter: bool,
    #[cfg(feature = "native-ops")] workspace: Option<workspace::Workspace>,
) -> io::Result<Option<PathBuf>> {
    browse_with_layout(
        path, return_on_enter, false,
        #[cfg(feature = "native-ops")]
        workspace,
    )
}

fn browse_with_layout(
    path: PathBuf,
    return_on_enter: bool,
    application: bool,
    #[cfg(feature = "native-ops")] mut workspace: Option<workspace::Workspace>,
) -> io::Result<Option<PathBuf>> {
    #[cfg(feature = "native-ops")]
    let writable = workspace.is_some();
    #[cfg(feature = "native-ops")]
    let personal = workspace.as_ref().is_some_and(|w| w.is_personal());
    #[cfg(not(feature = "native-ops"))]
    let writable = false;
    #[cfg(not(feature = "native-ops"))]
    let personal = false;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Interactive terminal required",
        ));
    }
    let mut model = Model::new(path)?;
    #[cfg(feature = "native-ops")]
    if let Some(w) = &mut workspace {
        w.load_settings(&mut model)?;
        w.observe(&model);
        if application {
            w.enable_application_review();
        }
    }
    let _terminal = Terminal::enter()?;
    let mut message = String::from(if personal {
        "Personal preview: approved roots; same-filesystem home Trash only."
    } else if writable {
        "Experimental scoped workspace. Review all operations."
    } else {
        "Experimental read-only build. File mutations disabled."
    });
    let mut searching = false;
    let mut previous_query = String::new();
    #[cfg(feature = "native-ops")]
    let trash = workspace.as_ref().map(|w| w.trash_path());
    #[cfg(not(feature = "native-ops"))]
    let trash = None;
    let mut dashboard = application.then(|| dashboard::State::new(trash));
    #[cfg(feature = "native-ops")]
    if let Some(ui) = &mut dashboard {
        if let Err(error) = ui.load_preferences(&mut model) {
            message = display_name(std::ffi::OsStr::new(&error.to_string()));
        }
    }
    #[cfg(feature = "native-ops")]
    if let Some(w)=&mut workspace { w.observe(&model); }
    loop {
        #[cfg(feature = "native-ops")]
        if let Some(ui)=&mut dashboard {
            if ui.tick_terminal()? {
                match model.reload() {
                    Ok(())=>{if let Some(w)=&mut workspace {w.observe_pane(&model);}},
                    Err(error)=>message=display_name(std::ffi::OsStr::new(&error.to_string())),
                }
            }
        }
        if let Some(ui) = &mut dashboard {
            #[cfg(feature = "native-ops")]
            let clipboard = workspace.as_ref().map_or(0, |w| w.clipboard_count());
            #[cfg(not(feature = "native-ops"))]
            let clipboard = 0;
            ui.draw(&mut model, &message, searching, writable, personal, clipboard, |path| {
                #[cfg(feature = "native-ops")]
                { workspace.as_ref().is_some_and(|w| w.marked(path)) }
                #[cfg(not(feature = "native-ops"))]
                { let _ = path; false }
            })?;
        } else {
            draw(
                &mut model,
                &message,
                searching,
                writable,
                #[cfg(feature = "native-ops")]
                workspace.as_ref(),
            )?;
        }
        let next_event=loop {
            #[cfg(feature = "native-ops")]
            if dashboard.as_ref().is_some_and(|ui|ui.needs_poll()) && !event::poll(std::time::Duration::from_millis(100))? {break None;}
            let input=event::read()?;
            // Mouse motion without a button does not alter the dashboard.
            // Drag, scroll, resize and all key events are still delivered.
            if dashboard.is_some()&&matches!(&input,Event::Mouse(m) if m.kind==MouseEventKind::Moved){continue;}
            break Some(input);
        };
        let Some(mut event)=next_event else{continue;};
        if let Some(ui) = &mut dashboard {
            let previous = model.directory.clone();
            event = ui.remap_input(event, searching);
            let input = ui.input(event, &mut model, searching, writable);
            let switched = ui.take_pane_switch();
            #[cfg(feature = "native-ops")]
            if let Some(w) = &mut workspace {
                if switched {
                    w.observe_pane(&model);
                } else if previous != model.directory {
                    w.observe(&model);
                }
            }
            #[cfg(not(feature = "native-ops"))]
            let _ = (previous, switched);
            match input {
                Ok(Some(next)) => event = next,
                Ok(None) => continue,
                Err(error) => {
                    message = display_name(std::ffi::OsStr::new(&error.to_string()));
                    continue;
                }
            }
        }
        #[cfg(feature = "native-ops")]
        let reload_event = match &event {
            Event::Key(k) => {
                searching
                    || matches!(
                        k.code,
                        KeyCode::Right
                            | KeyCode::Left
                            | KeyCode::Enter
                            | KeyCode::Char('h' | 's' | 'f' | 'g' | '[' | ']' | 'm')
                            | KeyCode::F(5)
                            | KeyCode::F(4)
                            | KeyCode::F(7)
                            | KeyCode::F(6)
                    )
            }
            Event::Mouse(m) => matches!(m.kind, MouseEventKind::Down(MouseButton::Left)),
            _ => false,
        };
        let result = match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                if searching {
                    match key.code {
                        KeyCode::Esc => {
                            searching = false;
                            model.set_view(
                                model.hidden,
                                model.order,
                                model.kind_filter,
                                previous_query.clone(),
                            )
                        }
                        KeyCode::Enter => {
                            searching = false;
                            Ok(())
                        }
                        KeyCode::Backspace => {
                            let mut query = model.query.clone();
                            query.pop();
                            model.set_view(model.hidden, model.order, model.kind_filter, query)
                        }
                        KeyCode::Char(c) if !c.is_control() => {
                            let mut query = model.query.clone();
                            query.push(c);
                            model.set_view(model.hidden, model.order, model.kind_filter, query)
                        }
                        _ => Ok(()),
                    }
                } else {
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
                            model.shift(-(terminal::size()?.1.saturating_sub(4) as isize));
                            Ok(())
                        }
                        KeyCode::PageDown => {
                            model.shift(terminal::size()?.1.saturating_sub(4) as isize);
                            Ok(())
                        }
                        KeyCode::Home => {
                            model.selected = 0;
                            Ok(())
                        }
                        KeyCode::End => {
                            model.selected = model.entries.len().saturating_sub(1);
                            Ok(())
                        }
                        KeyCode::Enter
                            if return_on_enter
                                && model
                                    .entries
                                    .get(model.selected)
                                    .is_some_and(|e| e.directory) =>
                        {
                            match model.enter() {
                                Ok(()) => return Ok(Some(model.directory)),
                                Err(e) => Err(e),
                            }
                        }
                        KeyCode::Right => model.enter(),
                        KeyCode::Enter => {
                            if let Some(e) = model.entries.get(model.selected) {
                                if e.directory {
                                    model.enter()
                                } else if e.path.is_file() {
                                    // Explicit external open; the chosen application may edit a file.
                                    match platform::open_file(&e.path) {
                                        Ok(mut child) => {
                                            std::thread::spawn(move || {
                                                let _ = child.wait();
                                            });
                                            message = format!("Opening: {}", e.name);
                                            Ok(())
                                        }
                                        Err(err) => Err(err),
                                    }
                                } else {
                                    Err(io::Error::other("select a regular file or folder"))
                                }
                            } else {
                                Ok(())
                            }
                        }
                        KeyCode::Left => model.parent(),
                        KeyCode::Char('[') => model.history(false),
                        KeyCode::Char(']') => model.history(true),
                        KeyCode::Char('h') => model.set_view(
                            !model.hidden,
                            model.order,
                            model.kind_filter,
                            model.query.clone(),
                        ),
                        KeyCode::Char('s') => model.set_view(
                            model.hidden,
                            model.order.next(),
                            model.kind_filter,
                            model.query.clone(),
                        ),
                        KeyCode::F(5) | KeyCode::Char('f') => model.reload(),
                        KeyCode::Char('/') => {
                            previous_query = model.query.clone();
                            searching = true;
                            Ok(())
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::Char('g') => match workspace::prompt("Go to path:")? {
                            None => Ok(()),
                            Some(s) if s.is_empty() => Ok(()),
                            Some(s) => {
                                let path = if s == "~" || s.starts_with("~/") {
                                    platform::home_dir()
                                        .map(|home| home.join(s.strip_prefix("~/").unwrap_or("")))
                                        .ok_or_else(|| io::Error::other("HOME unavailable"))
                                } else {
                                    Ok(model.directory.join(s))
                                };
                                path.and_then(|p| p.canonicalize())
                                    .and_then(|p| model.go(p))
                            }
                        },
                        KeyCode::Char('m') | KeyCode::F(4) => match places::choose()? {
                            Some(p) => model.go(p),
                            None => Ok(()),
                        },
                        KeyCode::F(7) => {
                            let labels = [
                                "All entries",
                                "Directories (not links)",
                                "Non-directory entries (not links)",
                                "Symbolic links",
                            ]
                            .map(str::to_owned);
                            if let Some(i) = choose("Filter entry type", &labels)? {
                                let kind = [
                                    KindFilter::All,
                                    KindFilter::Directories,
                                    KindFilter::Files,
                                    KindFilter::Links,
                                ][i];
                                model.set_view(model.hidden, model.order, kind, model.query.clone())
                            } else {
                                Ok(())
                            }
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::F(12) if application => {
                            let lines = workspace.as_ref().map_or_else(
                                || vec!["Clipboard unavailable in read-only mode".into()],
                                |w| w.clipboard_lines(),
                            );
                            page("Clipboard contents", &lines)
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::F(6) => {
                            if let Some(w) = &mut workspace {
                                match actions_menu(w, &mut model) {
                                    Ok(Some(msg)) => {
                                        message = msg;
                                        Ok(())
                                    }
                                    Ok(None) => Ok(()),
                                    Err(error) => Err(error),
                                }
                            } else {
                                Ok(())
                            }
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                            tools_ui::show(
                                &model.directory,
                                model.entries.get(model.selected).map(|e| e.path.as_path()),
                            )
                        }
                        KeyCode::Char('i') => match model.entries.get(model.selected) {
                            Some(e) => properties(&e.path),
                            None => Err(io::Error::other("select an item")),
                        },
                        #[cfg(feature = "native-ops")]
                        KeyCode::Char('p') | KeyCode::F(3) => {
                            match model.entries.get(model.selected) {
                                Some(e) => preview(&e.path, terminal_tree::preview::Mode::Auto),
                                None => Err(io::Error::other("select a file")),
                            }
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::F(8) => match model.entries.get(model.selected) {
                            Some(e) => preview(&e.path, terminal_tree::preview::Mode::Hex),
                            None => Err(io::Error::other("select a file")),
                        },
                        KeyCode::F(1) | KeyCode::Char('?') => {
                            help::show(writable, personal, return_on_enter)
                        }
                        KeyCode::Char(
                            'x' | 'v' | 'c' | 'd' | 'D' | 'r' | 'n' | 't' | 'R' | 'u' | 'j' | ' '
                            | 'a' | 'A' | 'S' | 'O',
                        )
                        | KeyCode::Delete
                        | KeyCode::F(2) => {
                            #[cfg(feature = "native-ops")]
                            if let Some(w) = &mut workspace {
                                let code = match key.code {
                                    KeyCode::Char(c) => c,
                                    KeyCode::Delete => 'd',
                                    _ => 'r',
                                };
                                match w.operate(code, &mut model) {
                                    Ok(msg) => {
                                        message = msg;
                                        Ok(())
                                    }
                                    Err(e) => Err(e),
                                }
                            } else {
                                message = "Disabled: read-only navigator.".into();
                                Ok(())
                            }
                            #[cfg(not(feature = "native-ops"))]
                            {
                                message = "Disabled: read-only navigator.".into();
                                Ok(())
                            }
                        }
                        _ => Ok(()),
                    }
                }
            }
            Event::Mouse(mouse) if !searching => {
                let (w, h) = terminal::size()?;
                match mouse.kind {
                    MouseEventKind::ScrollUp => {
                        model.shift(-1);
                        Ok(())
                    }
                    MouseEventKind::ScrollDown => {
                        model.shift(1);
                        Ok(())
                    }
                    MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
                        if h >= 5 && mouse.row > 0 && mouse.row <= h - 4 && mouse.column < w =>
                    {
                        let i = model.top + usize::from(mouse.row - 1);
                        if i < model.entries.len() {
                            model.selected = i;
                            #[cfg(feature = "native-ops")]
                            if let Some(w) = &mut workspace {
                                if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                                    && mouse.modifiers.contains(KeyModifiers::CONTROL)
                                {
                                    match w.operate(' ', &mut model) {
                                        Ok(msg) => message = msg,
                                        Err(e) => {
                                            message =
                                                display_name(std::ffi::OsStr::new(&e.to_string()))
                                        }
                                    }
                                    continue;
                                }
                                if mouse.kind == MouseEventKind::Down(MouseButton::Right)
                                    && mouse.modifiers.contains(KeyModifiers::SHIFT)
                                {
                                    match actions_menu(w, &mut model) {
                                        Ok(Some(msg)) => message = msg,
                                        Ok(None) => (),
                                        Err(e) => {
                                            message =
                                                display_name(std::ffi::OsStr::new(&e.to_string()))
                                        }
                                    }
                                    continue;
                                }
                            }
                            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                                model.enter()
                            } else {
                                Ok(())
                            }
                        } else {
                            Ok(())
                        }
                    }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        };
        #[cfg(feature = "native-ops")]
        if reload_event && result.is_ok() {
            if let Some(w) = &mut workspace {
                w.observe(&model);
            }
        }
        if let Err(e) = result {
            message = display_name(std::ffi::OsStr::new(&e.to_string()));
        }
    }
    #[cfg(feature = "native-ops")]
    if let Some(ui)=&mut dashboard { ui.save_on_exit(&model)?; }
    Ok(None)
}

#[cfg(feature = "native-ops")]
fn result_channel(path: PathBuf) -> io::Result<()> {
    use rustix::fs::{self as rx, Mode, OFlags};
    use std::fs::{self, File};
    use std::os::unix::fs::MetadataExt;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing result parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::other("missing result filename"))?;
    let uid = rustix::process::getuid().as_raw();
    let directory: File = rx::open(
        parent,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let dm = directory.metadata()?;
    if dm.uid() != uid || dm.mode() & 0o077 != 0 {
        return Err(io::Error::other("private owned result directory required"));
    }
    let mut file: File = rx::openat(
        &directory,
        name,
        OFlags::WRONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let original = file.metadata()?;
    if !original.is_file()
        || original.uid() != uid
        || original.mode() & 0o077 != 0
        || original.nlink() != 1
        || original.len() != 0
    {
        return Err(io::Error::other(
            "private empty regular result file required",
        ));
    }
    let Some(selected) = browse(env::current_dir()?, true, None)? else {
        return Ok(());
    };
    let destination = selected.canonicalize()?;
    if !destination.is_dir() {
        return Err(io::Error::other("result directory no longer exists"));
    }
    let current = file.metadata()?;
    let named = fs::symlink_metadata(&path)?;
    let parent_now = fs::symlink_metadata(parent)?;
    let id = |m: &fs::Metadata| {
        (
            m.dev(),
            m.ino(),
            m.mode(),
            m.uid(),
            m.nlink(),
            m.len(),
            m.ctime(),
            m.ctime_nsec(),
            m.mtime(),
            m.mtime_nsec(),
        )
    };
    if id(&current) != id(&original)
        || id(&named) != id(&original)
        || (parent_now.dev(), parent_now.ino()) != (dm.dev(), dm.ino())
    {
        return Err(io::Error::other("result channel changed"));
    }
    file.write_all(destination.as_os_str().as_bytes())?;
    file.write_all(&[0])?;
    file.flush()
}

fn main() {
    if let Err(e) = cli::run() {
        eprintln!(
            "terminal-tree: {}",
            display_name(std::ffi::OsStr::new(&e.to_string()))
        );
        std::process::exit(1);
    }
}
