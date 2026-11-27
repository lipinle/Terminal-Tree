#![forbid(unsafe_code)]
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind,
        KeyModifiers, MouseButton, MouseEventKind},
    execute, queue,
    style::{Color, Print, ResetColor, SetBackgroundColor, SetForegroundColor},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::env;
use std::ffi::OsString;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::os::unix::ffi::OsStrExt;
use terminal_tree::{display_name, scan, Model, Order};
use unicode_width::UnicodeWidthChar;

fn clip(text: &str, limit: usize) -> String {
    let mut width = 0;
    text.chars().take_while(|c| {
        let n = c.width().unwrap_or(0);
        if width + n > limit { return false; }
        width += n;
        true
    }).collect()
}

fn icon(directory: bool, symlink: bool) -> (&'static str, Color) {
    if symlink { ("↗", Color::Cyan) }
    else if directory { ("▸", Color::Blue) }
    else { ("·", Color::White) }
}

fn list(path: &Path, hidden: bool) -> io::Result<()> {
    let entries = scan(path, hidden, Order::Name, "")?;
    let mut out = io::BufWriter::new(io::stdout().lock());
    writeln!(out, "Location: {}", display_name(path.as_os_str()))?;
    writeln!(out, "My Computer")?;
    let limit = terminal::size().map_or(20, |(_, h)| usize::from(h).saturating_sub(4).max(10));
    for (i, e) in entries.iter().take(limit).enumerate() {
        let branch = if i + 1 == entries.len() { "└──" } else { "├──" };
        writeln!(out, "{branch} {} {}", icon(e.directory, e.symlink).0, e.name)?;
    }
    if entries.len() > limit { writeln!(out, "└── … {} more", entries.len() - limit)?; }
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
        let _ = execute!(io::stdout(), ResetColor, DisableMouseCapture, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn draw(model: &mut Model, message: &str, searching: bool) -> io::Result<()> {
    let (w, h) = terminal::size()?;
    let width = usize::from(w).saturating_sub(1);
    let mut out = io::stdout().lock();
    queue!(out, MoveTo(0, 0), Clear(ClearType::All), SetForegroundColor(Color::Cyan),
        Print(clip(&format!("Terminal Tree [Rust / read-only] | {}",
            display_name(model.directory.as_os_str())), width)), ResetColor)?;
    if h < 5 { return out.flush(); }
    let capacity = usize::from(h - 4);
    let (start, end) = model.viewport(capacity);
    for (row, index) in (start..end).enumerate() {
        let e = &model.entries[index];
        let (glyph, color) = icon(e.directory, e.symlink);
        queue!(out, MoveTo(0, (row + 1) as u16))?;
        if index == model.selected {
            queue!(out, SetBackgroundColor(Color::DarkGrey))?;
        }
        queue!(out, SetForegroundColor(color), Print(clip(
            &format!("{} {} {}", if index + 1 == model.entries.len() { "└──" } else { "├──" }, glyph, e.name),
            width)), ResetColor)?;
    }
    queue!(out, MoveTo(0, h - 3), Print(clip(&format!(
        "{}/{} | sort {:?} | hidden {} | filter: {}", if model.entries.is_empty() { 0 } else { model.selected + 1 },
        model.entries.len(), model.order, model.hidden,
        display_name(std::ffi::OsStr::new(&model.query))), width)),
        MoveTo(0, h - 2), Print(clip(message, width)),
        MoveTo(0, h - 1), Print(clip(if searching {
            "Filter: type text | Enter accept | Esc cancel"
        } else {
            "↑↓ select | ← parent | →/Enter folder | / filter | h hidden | s sort | F5 refresh | q exit"
        }, width)))?;
    out.flush()
}

fn browse(path: PathBuf) -> io::Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Interactive terminal required"));
    }
    let mut model = Model::new(path)?;
    let _terminal = Terminal::enter()?;
    let mut message = String::from("Experimental read-only build. File mutations disabled.");
    let mut searching = false;
    let mut previous_query = String::new();
    loop {
        draw(&mut model, &message, searching)?;
        let event = event::read()?;
        let result = match event {
            Event::Key(key) if matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    break;
                }
                if searching {
                    match key.code {
                        KeyCode::Esc => { searching = false; model.query = previous_query.clone(); model.reload() }
                        KeyCode::Enter => { searching = false; Ok(()) }
                        KeyCode::Backspace => { model.query.pop(); model.reload() }
                        KeyCode::Char(c) if !c.is_control() => { model.query.push(c); model.reload() }
                        _ => Ok(()),
                    }
                } else {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('q') => break,
                        KeyCode::Up => { model.shift(-1); Ok(()) }
                        KeyCode::Down => { model.shift(1); Ok(()) }
                        KeyCode::PageUp => { model.shift(-(terminal::size()?.1.saturating_sub(4) as isize)); Ok(()) }
                        KeyCode::PageDown => { model.shift(terminal::size()?.1.saturating_sub(4) as isize); Ok(()) }
                        KeyCode::Home => { model.selected = 0; Ok(()) }
                        KeyCode::End => { model.selected = model.entries.len().saturating_sub(1); Ok(()) }
                        KeyCode::Right | KeyCode::Enter => {
                            if model.entries.get(model.selected).is_some_and(|e| !e.directory) {
                                message = "File opening is not implemented in this read-only port.".into();
                                Ok(())
                            } else { model.enter() }
                        }
                        KeyCode::Left => model.parent(),
                        KeyCode::Char('[') => model.history(false),
                        KeyCode::Char(']') => model.history(true),
                        KeyCode::Char('h') => { model.hidden = !model.hidden; model.reload() }
                        KeyCode::Char('s') => { model.order = model.order.next(); model.reload() }
                        KeyCode::F(5) | KeyCode::Char('f') => model.reload(),
                        KeyCode::Char('/') => { previous_query = model.query.clone(); searching = true; Ok(()) }
                        KeyCode::F(1) | KeyCode::Char('?') => {
                            message = "Mouse wheel: move | Left click: folder | Right click: select | [ ] history".into(); Ok(())
                        }
                        KeyCode::Char('x' | 'v' | 'c' | 'd' | 'D' | 'r' | 'n' | 't' | 'R' | 'u')
                        | KeyCode::Delete | KeyCode::F(2) => {
                            message = "Disabled: the Rust safety engine has not been validated.".into(); Ok(())
                        }
                        _ => Ok(()),
                    }
                }
            }
            Event::Mouse(mouse) if !searching => {
                let (w, h) = terminal::size()?;
                match mouse.kind {
                    MouseEventKind::ScrollUp => { model.shift(-1); Ok(()) }
                    MouseEventKind::ScrollDown => { model.shift(1); Ok(()) }
                    MouseEventKind::Down(MouseButton::Left | MouseButton::Right)
                        if h >= 5 && mouse.row > 0 && mouse.row <= h - 4 && mouse.column < w => {
                            let i = model.top + usize::from(mouse.row - 1);
                            if i < model.entries.len() {
                                model.selected = i;
                                if mouse.kind == MouseEventKind::Down(MouseButton::Left) { model.enter() }
                                else { Ok(()) }
                            } else { Ok(()) }
                        }
                    _ => Ok(()),
                }
            }
            _ => Ok(()),
        };
        if let Err(e) = result { message = display_name(std::ffi::OsStr::new(&e.to_string())); }
    }
    Ok(())
}

fn path_arg(value: Option<OsString>) -> io::Result<PathBuf> {
    match value {
        Some(p) => {
            let p = PathBuf::from(p);
            if p.is_absolute() { Ok(p) } else { Ok(env::current_dir()?.join(p)) }
        }
        None => env::current_dir(),
    }
}

fn run() -> io::Result<()> {
    let mut args = env::args_os().skip(1);
    match args.next().as_deref().and_then(|x| x.to_str()) {
        Some("--version") => { println!("terminal-tree Rust experimental {}", env!("CARGO_PKG_VERSION")); Ok(()) }
        Some("--list") => {
            let first = args.next();
            let hidden = first.as_deref() == Some(std::ffi::OsStr::new("-a"));
            let path = path_arg(if hidden { args.next() } else { first })?;
            list(&path, hidden)
        }
        Some("--browse") => browse(path_arg(args.next())?),
        Some("--scan-paths0") => {
            let p = path_arg(args.next())?;
            let order = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("name") => Order::Name,
                Some("size") => Order::Size,
                Some("date") => Order::Date,
                Some("type") => Order::Type,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid sort order")),
            };
            let hidden = match args.next().as_deref().and_then(|s| s.to_str()) {
                Some("0") => false,
                Some("1") => true,
                _ => return Err(io::Error::new(io::ErrorKind::InvalidInput, "hidden must be 0 or 1")),
            };
            let query = args.next().unwrap_or_default().into_string()
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid UTF-8 query"))?;
            let mut out = io::BufWriter::new(io::stdout().lock());
            for entry in scan(&p, hidden, order, &query)? {
                out.write_all(entry.path.as_os_str().as_bytes())?;
                out.write_all(&[0])?;
            }
            out.flush()
        }
        Some("--scan-count") => {
            let p = path_arg(args.next())?;
            let n: usize = args.next().and_then(|x| x.to_str()?.parse().ok()).unwrap_or(1);
            if n == 0 || n > 1000 { return Err(io::Error::new(io::ErrorKind::InvalidInput, "repeat count must be 1..1000")); }
            let mut count = 0;
            for _ in 0..n { count += std::hint::black_box(scan(&p, false, Order::Name, "")?).len(); }
            println!("{count}");
            Ok(())
        }
        _ => {
            eprintln!("Experimental read-only Rust port\nUsage: terminal-tree --list [-a] [PATH] | --browse [PATH] | --scan-count PATH [REPEATS] | --version");
            Err(io::Error::new(io::ErrorKind::InvalidInput, "unsupported or missing command"))
        }
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("terminal-tree: {}", display_name(std::ffi::OsStr::new(&e.to_string())));
        std::process::exit(1);
    }
}
