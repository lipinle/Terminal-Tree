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
use std::env;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use terminal_tree::{display_name, scan, KindFilter, Model, Order};
use unicode_width::UnicodeWidthChar;
mod edition;
mod help;
mod places;
mod platform;
#[cfg(feature = "native-ops")]
mod recovery;
#[cfg(feature = "native-ops")]
mod settings;
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
        Ok(guard)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
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
                "Space mark | a all | A clear | c/x/v transfer | d/R/D trash/restore/purge | F1 help | q exit"
            } else {
                "↑↓ select | ← parent | →/Enter folder | / filter | h hidden | s sort | F5 refresh | q exit"
            },
            width
        ))
    )?;
    out.flush()
}

fn page(title: &str, lines: &[String]) -> io::Result<()> {
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
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return Ok(()),
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
        if let Event::Key(k) = event::read()? {
            if !matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
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
fn preview(path: &Path) -> io::Result<()> {
    use rustix::fs::{self as rx, Mode, OFlags};
    use std::io::Read;
    use std::os::unix::fs::MetadataExt;
    let before = std::fs::symlink_metadata(path)?;
    if !before.is_file() {
        return Err(io::Error::other("preview requires a regular non-link file"));
    }
    let f: std::fs::File = rx::open(
        path,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let m = f.metadata()?;
    if !m.is_file() || (m.dev(), m.ino()) != (before.dev(), before.ino()) {
        return Err(io::Error::other("file changed before preview"));
    }
    let mut bytes = Vec::new();
    f.take(65537).read_to_end(&mut bytes)?;
    let mut lines = vec![display_name(path.as_os_str())];
    if bytes.contains(&0) {
        lines.push("Binary file; preview unavailable".into());
    } else {
        let truncated = bytes.len() > 65536;
        bytes.truncate(65536);
        for line in bytes.split(|&b| b == b'\n') {
            lines.push(display_name(std::ffi::OsStr::from_bytes(line)));
        }
        if truncated {
            lines.push("[Preview limited to 64 KiB]".into());
        }
    }
    page("File preview", &lines)
}

fn browse(
    path: PathBuf,
    return_on_enter: bool,
    #[cfg(feature = "native-ops")] mut workspace: Option<workspace::Workspace>,
) -> io::Result<PathBuf> {
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
    loop {
        draw(
            &mut model,
            &message,
            searching,
            writable,
            #[cfg(feature = "native-ops")]
            workspace.as_ref(),
        )?;
        let event = event::read()?;
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
                            model.query = previous_query.clone();
                            model.reload()
                        }
                        KeyCode::Enter => {
                            searching = false;
                            Ok(())
                        }
                        KeyCode::Backspace => {
                            model.query.pop();
                            model.reload()
                        }
                        KeyCode::Char(c) if !c.is_control() => {
                            model.query.push(c);
                            model.reload()
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
                                Ok(()) => break,
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
                        KeyCode::Char('h') => {
                            model.hidden = !model.hidden;
                            model.reload()
                        }
                        KeyCode::Char('s') => {
                            model.order = model.order.next();
                            model.reload()
                        }
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
                                    match env::var_os("HOME") {
                                        Some(home) => PathBuf::from(home)
                                            .join(s.strip_prefix("~/").unwrap_or("")),
                                        None => return Err(io::Error::other("HOME unavailable")),
                                    }
                                } else {
                                    model.directory.join(s)
                                };
                                path.canonicalize().and_then(|p| model.go(p))
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
                                let old = model.kind_filter;
                                model.kind_filter = [
                                    KindFilter::All,
                                    KindFilter::Directories,
                                    KindFilter::Files,
                                    KindFilter::Links,
                                ][i];
                                if let Err(e) = model.reload() {
                                    model.kind_filter = old;
                                    Err(e)
                                } else {
                                    Ok(())
                                }
                            } else {
                                Ok(())
                            }
                        }
                        #[cfg(feature = "native-ops")]
                        KeyCode::F(6) => {
                            if let Some(w) = &mut workspace {
                                match actions_menu(w, &mut model)? {
                                    Some(msg) => {
                                        message = msg;
                                        Ok(())
                                    }
                                    None => Ok(()),
                                }
                            } else {
                                Ok(())
                            }
                        }
                        KeyCode::Char('i') => match model.entries.get(model.selected) {
                            Some(e) => properties(&e.path),
                            None => Err(io::Error::other("select an item")),
                        },
                        #[cfg(feature = "native-ops")]
                        KeyCode::Char('p') | KeyCode::F(3) => {
                            match model.entries.get(model.selected) {
                                Some(e) => preview(&e.path),
                                None => Err(io::Error::other("select a file")),
                            }
                        }
                        KeyCode::F(1) | KeyCode::Char('?') => {
                            help::show(writable, personal, return_on_enter)
                        }
                        KeyCode::Char(
                            'x' | 'v' | 'c' | 'd' | 'D' | 'r' | 'n' | 't' | 'R' | 'u' | 'j' | ' '
                            | 'a' | 'A' | 'S',
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
    Ok(model.directory)
}

fn path_arg(value: Option<OsString>) -> io::Result<PathBuf> {
    match value {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() {
                Ok(p)
            } else {
                Ok(env::current_dir()?.join(p))
            }
        }
        None => env::current_dir(),
    }
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
    let destination = browse(env::current_dir()?, true, None)?.canonicalize()?;
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

fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(|x| x.to_str()) {
        Some("--edition") => {
            if args.next().is_some() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "unexpected argument",
                ));
            }
            println!("{}", edition::NAME);
            Ok(())
        }
        Some("--version") => {
            println!(
                "terminal-tree Rust experimental {}",
                env!("CARGO_PKG_VERSION")
            );
            Ok(())
        }
        Some("--list") => {
            let first = args.next();
            let hidden = first.as_deref() == Some(std::ffi::OsStr::new("-a"));
            let path = path_arg(if hidden { args.next() } else { first })?;
            list(&path, hidden)
        }
        Some("--browse") => {
            let p = path_arg(args.next())?;
            #[cfg(feature = "native-ops")]
            {
                browse(p, false, None).map(|_| ())
            }
            #[cfg(not(feature = "native-ops"))]
            {
                browse(p, false).map(|_| ())
            }
        }
        #[cfg(feature = "native-ops")]
        Some("--personal") => {
            // Never infer a broader scope from an arbitrary HOME override.
            // Resolve the real account's home from NSS via getent, then require
            // the environment HOME to agree. This mode is explicitly opt-in.
            let uid = rustix::process::getuid().as_raw();
            let output = std::process::Command::new("getent")
                .args(["passwd", &uid.to_string()])
                .output()?;
            if !output.status.success() {
                return Err(io::Error::other("account home unavailable"));
            }
            let raw = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
            let fields: Vec<_> = raw.split(|b| *b == b':').collect();
            if fields.len() != 7 || fields[2] != uid.to_string().as_bytes() || raw.contains(&b'\n')
            {
                return Err(io::Error::other("ambiguous account record"));
            }
            let home = PathBuf::from(std::ffi::OsStr::from_bytes(fields[5]));
            if env::var_os("HOME").as_deref() != Some(home.as_os_str()) {
                return Err(io::Error::other("HOME differs from account home"));
            }
            let journal =
                path_arg(Some(args.next().ok_or_else(|| {
                    io::Error::other("private existing journal path required")
                })?))?;
            let extras = args
                .map(|p| path_arg(Some(p)))
                .collect::<io::Result<Vec<_>>>()?;
            let w = workspace::Workspace::personal(&home, &journal, &extras)?;
            browse(home, false, Some(w)).map(|_| ())
        }
        #[cfg(feature = "native-ops")]
        Some("--workspace") => {
            let root = path_arg(args.next())?;
            let journal = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("journal path required"))?,
            ))?;
            let data = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("data path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected argument"));
            }
            let w = workspace::Workspace::new(&root, &journal, &data)?;
            browse(w.root().to_path_buf(), false, Some(w)).map(|_| ())
        }
        #[cfg(feature = "native-ops")]
        Some("--result") => {
            let result = path_arg(Some(
                args.next()
                    .ok_or_else(|| io::Error::other("result path required"))?,
            ))?;
            if args.next().is_some() {
                return Err(io::Error::other("unexpected argument"));
            }
            result_channel(result)
        }
        #[cfg(unix)]
        Some("--scan-paths0") => {
            let p = path_arg(args.next())?;
            let order = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("name") => Order::Name,
                Some("size") => Order::Size,
                Some("date") => Order::Date,
                Some("type") => Order::Type,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "invalid sort order",
                    ))
                }
            };
            let hidden = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("0") => false,
                Some("1") => true,
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "hidden must be 0 or 1",
                    ))
                }
            };
            let query =
                args.next().unwrap_or_default().into_string().map_err(|_| {
                    io::Error::new(io::ErrorKind::InvalidInput, "invalid UTF-8 query")
                })?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for entry in scan(&p, hidden, order, &query)? {
                out.write_all(entry.path.as_os_str().as_bytes())?;
                out.write_all(&[0])?;
            }
            out.flush()
        }
        Some("--scan-count") => {
            let p = path_arg(args.next())?;
            let n: usize = args
                .next()
                .and_then(|x| x.to_str()?.parse().ok())
                .unwrap_or(1);
            if n == 0 || n > 1000 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "repeat count must be 1..1000",
                ));
            }
            let mut count = 0;
            for _ in 0..n {
                count += std::hint::black_box(scan(&p, false, Order::Name, "")?).len();
            }
            println!("{count}");
            Ok(())
        }
        _ => {
            eprintln!("Experimental read-only Rust port\nUsage: terminal-tree --list [-a] [PATH] | --browse [PATH] | --scan-count PATH [REPEATS] | --version | --edition");
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "unsupported or missing command",
            ))
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!(
            "terminal-tree: {}",
            display_name(std::ffi::OsStr::new(&e.to_string()))
        );
        std::process::exit(1);
    }
}
