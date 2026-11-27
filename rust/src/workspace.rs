//! Explicitly scoped experimental write UI. The default navigator stays read-only.
use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use crossterm::{cursor::MoveTo, event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue, style::Print, terminal::{self, Clear, ClearType}};
use terminal_tree::{display_name, Model};
use terminal_tree::operations::{Engine, Review, Transfer, Trash, Undo};
use unicode_width::UnicodeWidthChar;

type Stamp = (u64,u64,u32,u64,i64,i64,i64,i64);
fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = fs::symlink_metadata(path)?;
    Ok((m.dev(),m.ino(),m.mode(),m.size(),m.mtime(),m.mtime_nsec(),m.ctime(),m.ctime_nsec()))
}
fn error(s: &str) -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, s) }
fn path_text(p: &Path) -> String { display_name(p.as_os_str()) }
fn keys() -> io::Result<crossterm::event::KeyEvent> {
    loop {
        if let Event::Key(k) = event::read()? {
            if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) { return Ok(k); }
        }
    }
}
fn cancelled(k: &crossterm::event::KeyEvent) -> bool {
    k.code == KeyCode::Esc || (k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c'))
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut used = 0;
    for c in s.chars() {
        let n = c.width().unwrap_or(0);
        if used + n > width && !lines.last().unwrap().is_empty() {
            lines.push(String::new()); used = 0;
        }
        lines.last_mut().unwrap().push(c); used += n;
    }
    lines
}
fn prompt(label: &str) -> io::Result<Option<String>> {
    let mut input = String::new();
    loop {
        let (w,h) = terminal::size()?;
        let mut out = io::stdout().lock();
        queue!(out, MoveTo(0,h.saturating_sub(1)), Clear(ClearType::CurrentLine),
            Print(super::clip(&format!("{label} {}", display_name(OsStr::new(&input))), usize::from(w).saturating_sub(1))))?;
        out.flush()?; drop(out);
        let k = keys()?;
        if cancelled(&k) { return Ok(None); }
        match k.code {
            KeyCode::Enter => return Ok(Some(input)),
            KeyCode::Backspace => { input.pop(); }
            KeyCode::Char(c) if !c.is_control() && input.len() < 4096 => input.push(c),
            _ => (),
        }
    }
}
fn confirm(title: &str, paths: &[&Path], word: &str) -> io::Result<bool> {
    let mut top = 0usize;
    loop {
        let (w,h) = terminal::size()?;
        if w < 24 || h < 6 { return Err(error("terminal too small for full review")); }
        let width = usize::from(w-1);
        let lines: Vec<_> = paths.iter().flat_map(|p| wrap(&path_text(p), width)).collect();
        let cap = usize::from(h-3);
        let end = lines.len().saturating_sub(cap);
        top = top.min(end);
        let mut out = io::stdout().lock();
        queue!(out, MoveTo(0,0), Clear(ClearType::All), Print(super::clip(title,width)))?;
        for (i,line) in lines.iter().skip(top).take(cap).enumerate() {
            queue!(out, MoveTo(0,(i+1) as u16), Print(line))?;
        }
        queue!(out, MoveTo(0,h-2), Print(super::clip("Review paths | Enter next/confirm | Esc cancel",width)))?;
        out.flush()?; drop(out);
        let k = keys()?;
        if cancelled(&k) { return Ok(false); }
        match k.code {
            KeyCode::Enter if top == end => return Ok(prompt(&format!("Type {word}:"))?.as_deref() == Some(word)),
            KeyCode::Enter | KeyCode::PageDown => top = top.saturating_add(cap).min(end),
            KeyCode::PageUp => top = top.saturating_sub(cap),
            KeyCode::Down => top = top.saturating_add(1).min(end),
            KeyCode::Up => top = top.saturating_sub(1),
            _ => (),
        }
    }
}
fn filename() -> io::Result<Option<String>> {
    let Some(name) = prompt("Name:")? else { return Ok(None) };
    if name.is_empty() { return Ok(None); }
    if name == "." || name == ".." || name.contains('/') || name.starts_with(".terminal-tree-copy-") {
        return Err(error("single non-managed filename required"));
    }
    Ok(Some(name))
}
pub struct Workspace {
    engine: Engine,
    root: PathBuf,
    data: PathBuf,
    observed: BTreeMap<PathBuf, Stamp>,
    clipboard: Option<(Review,Transfer)>,
    undo: Vec<Undo>,
}
impl Workspace {
    pub fn new(root: &Path, journal: &Path, data: &Path) -> io::Result<Self> {
        if rustix::process::geteuid().is_root() { return Err(error("do not run as root")); }
        let root = root.canonicalize()?;
        let meta = fs::metadata(&root)?;
        if meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & 0o077 != 0 {
            return Err(error("private owned workspace (0700) required"));
        }
        let engine = Engine::new(&root, journal)?;
        let data = data.canonicalize()?;
        if data == root || !data.starts_with(&root) { return Err(error("private data directory inside workspace required")); }
        let m = fs::metadata(&data)?;
        if !m.is_dir() || m.uid() != meta.uid() || m.mode() & 0o077 != 0 { return Err(error("private data directory required")); }
        Trash::new(&engine,&data)?;
        Ok(Self { engine, root, data, observed:BTreeMap::new(), clipboard:None, undo:Vec::new() })
    }
    pub fn root(&self) -> &Path { &self.root }
    pub fn observe(&mut self, model: &Model) {
        self.observed = model.entries.iter().filter_map(|e| stamp(&e.path).ok().map(|s|(e.path.clone(),s))).collect();
    }
    fn ordinary(&self,p:&Path) -> io::Result<()> {
        let parent = p.parent().ok_or_else(||error("missing parent"))?.canonicalize()?;
        let p = parent.join(p.file_name().ok_or_else(||error("missing name"))?);
        if !p.starts_with(&self.root) || p == self.root || p.starts_with(&self.data) || self.data.starts_with(&p)
            || p.components().any(|c| c.as_os_str().to_string_lossy().starts_with(".terminal-tree-copy-")) {
            return Err(error("outside ordinary workspace or managed location"));
        }
        Ok(())
    }
    fn selected(&self,model:&Model) -> io::Result<PathBuf> {
        let p = &model.entries.get(model.selected).ok_or_else(||error("select an item"))?.path;
        if self.observed.get(p) != Some(&stamp(p)?) { return Err(error("selection changed; refresh first")); }
        Ok(p.clone())
    }
    fn review(&self,model:&Model) -> io::Result<Review> {
        let p = self.selected(model)?;
        self.ordinary(&p)?;
        let review = self.engine.review(&p)?;
        self.selected(model)?;
        Ok(review)
    }
    pub fn operate(&mut self, key: char, model: &mut Model) -> io::Result<String> {
        let result = match key {
            'c'|'x' => {
                let review = self.review(model)?;
                self.clipboard = Some((review, if key=='c' {Transfer::Copy} else {Transfer::Move}));
                return Ok("Clipboard ready; v to review and paste".into());
            }
            'v' => {
                let (review,kind) = self.clipboard.as_ref().ok_or_else(||error("clipboard empty"))?;
                let dst = model.directory.join(review.path().file_name().unwrap());
                self.ordinary(review.path())?; self.ordinary(&dst)?;
                let target = self.engine.destination(&dst)?;
                if !confirm(if matches!(kind,Transfer::Copy) {"Copy item"} else {"Move item"}, &[review.path(),target.path()], "yes")? { return Ok("Cancelled".into()); }
                if let Some(undo) = self.engine.transfer_reviewed(review,&target,*kind)? { self.undo.push(undo); }
                if matches!(kind,Transfer::Move) { self.clipboard=None; }
                "Transfer completed"
            }
            'r' => {
                let review=self.review(model)?;
                let Some(name)=filename()? else{return Ok("Cancelled".into())};
                let dst=model.directory.join(name);
                self.ordinary(&dst)?;
                let target=self.engine.destination(&dst)?;
                if !confirm("Rename item",&[review.path(),target.path()],"yes")? {return Ok("Cancelled".into());}
                if let Some(undo)=self.engine.transfer_reviewed(&review,&target,Transfer::Move)? {self.undo.push(undo);}
                "Renamed"
            }
            'n'|'t' => {
                let Some(name)=filename()? else{return Ok("Cancelled".into())};
                let dst=model.directory.join(name);
                self.ordinary(&dst)?;
                let target=self.engine.destination(&dst)?;
                if !confirm("Create item",&[target.path()],"yes")? {return Ok("Cancelled".into());}
                self.engine.create_reviewed(&target,key=='n')?;
                "Created"
            }
            'u' => {
                let undo=self.undo.last().ok_or_else(||error("nothing to undo"))?;
                self.ordinary(undo.source())?; self.ordinary(undo.destination())?;
                let dst=self.engine.destination(undo.destination())?;
                if !confirm("Undo move",&[undo.source(),dst.path()],"yes")? {return Ok("Cancelled".into());}
                self.engine.undo_reviewed(undo,&dst)?;
                self.undo.pop(); "Undone"
            }
            'd' => {
                let review=self.review(model)?;
                if !confirm("Move to Trash",&[review.path()],"yes")? {return Ok("Cancelled".into());}
                Trash::new(&self.engine,&self.data)?.send(&review)?;
                "Trashed"
            }
            'R' => {
                let p=self.selected(model)?;
                let trash=Trash::new(&self.engine,&self.data)?;
                let review=trash.review_restore(&p)?;
                self.ordinary(review.destination())?;
                self.selected(model)?;
                if !confirm("Restore item",&[review.source(),review.destination()],"yes")? {return Ok("Cancelled".into());}
                let outcome=trash.restore(&review)?;
                return self.refresh(model,format!("{outcome:?}"));
            }
            'D' => {
                let p=self.selected(model)?;
                let trash=Trash::new(&self.engine,&self.data)?;
                let review=trash.review_purge(&p)?;
                self.selected(model)?;
                if !confirm("PERMANENT DELETE - cannot undo",&[review.source()],"DELETE")? {return Ok("Cancelled".into());}
                let outcome=trash.purge(&review,"DELETE")?;
                return self.refresh(model,format!("{outcome:?}"));
            }
            'j' => {
                let count=self.engine.pending()?.len();
                return Ok(format!("Pending records: {count}; inspect journal before retrying"));
            }
            _ => return Err(error("unsupported operation")),
        };
        self.refresh(model,result.into())
    }
    fn refresh(&mut self,model:&mut Model,msg:String)->io::Result<String> {
        model.reload().map_err(|e|io::Error::other(format!("operation completed; refresh failed: {e}")))?;
        self.observe(model);
        Ok(msg)
    }
}
