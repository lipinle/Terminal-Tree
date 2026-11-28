//! Explicitly scoped experimental write UI. The default navigator stays read-only.
use super::worker;
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use terminal_tree::operations::{Engine, Review, Transfer, Trash, Undo};
use terminal_tree::{display_name, Model};
use unicode_width::UnicodeWidthChar;

type Stamp = (u64, u64, u32, u64, i64, i64, i64, i64);
fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = fs::symlink_metadata(path)?;
    Ok((
        m.dev(),
        m.ino(),
        m.mode(),
        m.size(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    ))
}
fn error(s: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, s)
}
fn path_text(p: &Path) -> String {
    display_name(p.as_os_str())
}
fn keys() -> io::Result<crossterm::event::KeyEvent> {
    loop {
        if let Event::Key(k) = event::read()? {
            if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                return Ok(k);
            }
        }
    }
}
fn cancelled(k: &crossterm::event::KeyEvent) -> bool {
    k.code == KeyCode::Esc
        || (k.modifiers.contains(KeyModifiers::CONTROL) && k.code == KeyCode::Char('c'))
}
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines = vec![String::new()];
    let mut used = 0;
    for c in s.chars() {
        let n = c.width().unwrap_or(0);
        if used + n > width && !lines.last().unwrap().is_empty() {
            lines.push(String::new());
            used = 0;
        }
        lines.last_mut().unwrap().push(c);
        used += n;
    }
    lines
}
pub(crate) fn prompt(label: &str) -> io::Result<Option<String>> {
    let mut input = String::new();
    loop {
        let (w, h) = terminal::size()?;
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, h.saturating_sub(1)),
            Clear(ClearType::CurrentLine),
            Print(super::clip(
                &format!("{label} {}", display_name(OsStr::new(&input))),
                usize::from(w).saturating_sub(1)
            ))
        )?;
        out.flush()?;
        drop(out);
        let k = keys()?;
        if cancelled(&k) {
            return Ok(None);
        }
        match k.code {
            KeyCode::Enter => return Ok(Some(input)),
            KeyCode::Backspace => {
                input.pop();
            }
            KeyCode::Char(c) if !c.is_control() && input.len() + c.len_utf8() <= 4096 => {
                input.push(c)
            }
            _ => (),
        }
    }
}
fn confirm(title: &str, paths: &[&Path], word: &str) -> io::Result<bool> {
    confirm_review(title, paths, word, false)
}

fn confirm_review(title: &str, paths: &[&Path], word: &str, quick: bool) -> io::Result<bool> {
    let mut top = 0usize;
    loop {
        let (w, h) = terminal::size()?;
        if w < 24 || h < 6 {
            return Err(error("terminal too small for full review"));
        }
        let width = usize::from(w - 1);
        let lines: Vec<_> = paths
            .iter()
            .flat_map(|p| wrap(&path_text(p), width))
            .collect();
        let cap = usize::from(h - 3);
        let end = lines.len().saturating_sub(cap);
        top = top.min(end);
        let mut out = io::stdout().lock();
        queue!(
            out,
            MoveTo(0, 0),
            Clear(ClearType::All),
            Print(super::clip(title, width))
        )?;
        for (i, line) in lines.iter().skip(top).take(cap).enumerate() {
            queue!(out, MoveTo(0, (i + 1) as u16), Print(line))?;
        }
        queue!(
            out,
            MoveTo(0, h - 2),
            Print(super::clip(
                if quick && top == end {
                    "Review paths | Enter confirm | Esc cancel"
                } else {
                    "Review paths | Enter next/confirm | Esc cancel"
                },
                width
            ))
        )?;
        out.flush()?;
        drop(out);
        let k = keys()?;
        if cancelled(&k) {
            return Ok(false);
        }
        match k.code {
            KeyCode::Enter if top == end && quick && k.kind == KeyEventKind::Press => {
                return Ok(true);
            }
            KeyCode::Enter if top == end && !quick => {
                return Ok(prompt(&format!("Type {word}:"))?.as_deref() == Some(word))
            }
            KeyCode::Enter | KeyCode::PageDown => top = top.saturating_add(cap).min(end),
            KeyCode::PageUp => top = top.saturating_sub(cap),
            KeyCode::Down => top = top.saturating_add(1).min(end),
            KeyCode::Up => top = top.saturating_sub(1),
            _ => (),
        }
    }
}
fn filename() -> io::Result<Option<String>> {
    let Some(name) = prompt("Name:")? else {
        return Ok(None);
    };
    if name.is_empty() {
        return Ok(None);
    }
    if name == "." || name == ".." || name.contains('/') || name.starts_with(".terminal-tree-copy-")
    {
        return Err(error("single non-managed filename required"));
    }
    Ok(Some(name))
}
pub struct Workspace {
    engine: Engine,
    root: PathBuf,
    data: PathBuf,
    settings: super::settings::Store,
    observed: BTreeMap<PathBuf, Stamp>,
    marks: BTreeMap<PathBuf, Stamp>,
    mark_directory: PathBuf,
    clipboard: Option<(Vec<Review>, Transfer)>,
    undo: Vec<Undo>,
    quick_review: bool,
}
impl Workspace {
    pub fn new(root: &Path, journal: &Path, data: &Path) -> io::Result<Self> {
        if rustix::process::geteuid().is_root() {
            return Err(error("do not run as root"));
        }
        let root = root.canonicalize()?;
        let meta = fs::metadata(&root)?;
        if meta.uid() != rustix::process::getuid().as_raw() || meta.mode() & 0o077 != 0 {
            return Err(error("private owned workspace (0700) required"));
        }
        let engine = Engine::new(&root, journal)?;
        let data = data.canonicalize()?;
        if data == root || !data.starts_with(&root) {
            return Err(error("private data directory inside workspace required"));
        }
        let m = fs::metadata(&data)?;
        if !m.is_dir() || m.uid() != meta.uid() || m.mode() & 0o077 != 0 {
            return Err(error("private data directory required"));
        }
        Trash::new(&engine, &data)?;
        let settings = super::settings::Store::new(journal)?;
        Ok(Self {
            engine,
            root,
            data,
            settings,
            observed: BTreeMap::new(),
            marks: BTreeMap::new(),
            mark_directory: PathBuf::new(),
            clipboard: None,
            undo: Vec::new(),
            quick_review: false,
        })
    }
    pub fn personal(home: &Path, journal: &Path, extra: &[PathBuf]) -> io::Result<Self> {
        let root = home.to_path_buf();
        let engine = Engine::new_personal(&root, journal, extra, &[])?;
        let data = engine
            .personal_data_home()
            .ok_or_else(|| error("personal data home unavailable"))?
            .to_path_buf();
        let settings = super::settings::Store::new(journal)?;
        Ok(Self {
            engine,
            root,
            data,
            settings,
            observed: BTreeMap::new(),
            marks: BTreeMap::new(),
            mark_directory: PathBuf::new(),
            clipboard: None,
            undo: Vec::new(),
            quick_review: false,
        })
    }
    pub fn load_settings(&self, model: &mut Model) -> io::Result<()> {
        self.settings.load(model)
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn is_personal(&self) -> bool {
        self.engine.is_personal()
    }
    pub fn marked(&self, p: &Path) -> bool {
        self.marks.contains_key(p)
    }
    pub fn enable_application_review(&mut self) {
        self.quick_review = true;
    }
    pub fn clipboard_lines(&self) -> Vec<String> {
        let Some((items, kind)) = &self.clipboard else {
            return vec!["Clipboard is empty".into()];
        };
        let mut lines = vec![format!("{} | {} items", if matches!(kind, Transfer::Copy) { "Copy" } else { "Cut" }, items.len())];
        lines.extend(items.iter().map(|item| path_text(item.path())));
        lines
    }
    pub fn clipboard_count(&self) -> usize {
        self.clipboard.as_ref().map_or(0, |(items, _)| items.len())
    }
    pub fn trash_path(&self) -> PathBuf {
        self.data.join("Trash/files")
    }
    pub fn marked_count(&self) -> usize {
        self.marks.len()
    }
    pub fn observe_pane(&mut self, model: &Model) {
        // Selection marks belong to the active pane, not the clipboard.
        self.clear_marks();
        self.observe(model);
    }
    pub fn observe(&mut self, model: &Model) {
        if self.mark_directory != model.directory {
            self.marks.clear();
            self.mark_directory = model.directory.clone();
        }
        self.observed = model
            .entries
            .iter()
            .filter_map(|e| stamp(&e.path).ok().map(|s| (e.path.clone(), s)))
            .collect();
    }
    fn ordinary(&self, p: &Path) -> io::Result<()> {
        if self.engine.is_personal() {
            return self.engine.validate_personal_path(p);
        }
        let parent = p
            .parent()
            .ok_or_else(|| error("missing parent"))?
            .canonicalize()?;
        let p = parent.join(p.file_name().ok_or_else(|| error("missing name"))?);
        if !p.starts_with(&self.root)
            || p == self.root
            || p.starts_with(&self.data)
            || self.data.starts_with(&p)
            || p.components().any(|c| {
                c.as_os_str()
                    .to_string_lossy()
                    .starts_with(".terminal-tree-copy-")
            })
        {
            return Err(error("outside ordinary workspace or managed location"));
        }
        Ok(())
    }
    fn selected(&self, model: &Model) -> io::Result<PathBuf> {
        let p = &model
            .entries
            .get(model.selected)
            .ok_or_else(|| error("select an item"))?
            .path;
        if self.observed.get(p) != Some(&stamp(p)?) {
            return Err(error("selection changed; refresh first"));
        }
        Ok(p.clone())
    }
    fn review(&self, model: &Model) -> io::Result<Review> {
        let p = self.selected(model)?;
        self.ordinary(&p)?;
        let review = worker::run(1, |progress| {
            progress.item(path_text(&p), 0)?;
            self.engine.review_with(&p, &mut |s| progress.checkpoint(s))
        })?;
        self.selected(model)?;
        Ok(review)
    }
    fn selected_paths(&self, model: &Model) -> io::Result<Vec<PathBuf>> {
        if self.marks.is_empty() {
            return Ok(vec![self.selected(model)?]);
        }
        let visible: BTreeSet<_> = model.entries.iter().map(|e| &e.path).collect();
        for (p, s) in &self.marks {
            if !visible.contains(p) || stamp(p)? != *s || self.observed.get(p) != Some(s) {
                return Err(error(
                    "marked item hidden, missing or changed; clear marks and select again",
                ));
            }
        }
        Ok(self.marks.keys().cloned().collect())
    }
    fn reviews(&self, model: &Model) -> io::Result<Vec<Review>> {
        let paths = self.selected_paths(model)?;
        let out = worker::run(paths.len(), |progress| {
            let mut out = Vec::with_capacity(paths.len());
            for (i, p) in paths.iter().enumerate() {
                progress.item(path_text(p), i)?;
                self.ordinary(p)?;
                out.push(
                    self.engine
                        .review_with(p, &mut |s| progress.checkpoint(s))?,
                );
            }
            Ok(out)
        })?;
        self.selected_paths(model)?;
        Ok(out)
    }
    fn clear_marks(&mut self) {
        self.marks.clear();
    }
    fn finish_batch(
        &mut self,
        model: &mut Model,
        done: usize,
        total: usize,
        operation: &str,
        failure: Option<(PathBuf, io::Error)>,
    ) -> io::Result<String> {
        self.clear_marks();
        let message = if let Some((path, e)) = failure {
            format!("{operation}: {done}/{total} confirmed complete; stopped at {}: {e}. Inspect journal and paths; do not blindly retry.",path_text(&path))
        } else {
            format!("{operation}: {done}/{total} complete")
        };
        // The complete failure path is shown in a scrollable modal, not just
        // the clipped status bar. No automatic rollback of durable items.
        if done != total {
            let (width, _) = terminal::size()?;
            super::page(
                "Batch stopped",
                &wrap(&message, usize::from(width).saturating_sub(1).max(1)),
            )?;
        }
        self.refresh(model, message)
    }
    pub fn operate(&mut self, key: char, model: &mut Model) -> io::Result<String> {
        if key == ' ' {
            let p = self.selected(model)?;
            if self.marks.remove(&p).is_none() {
                if self.marks.len() >= 1000 {
                    return Err(error("mark limit: 1000"));
                }
                self.marks.insert(p.clone(), stamp(&p)?);
            }
            return Ok(format!("{} marked", self.marks.len()));
        }
        if key == 'a' {
            if model.entries.len() > 1000 {
                return Err(error("mark limit: 1000; use a filter"));
            }
            let mut marks = BTreeMap::new();
            for entry in &model.entries {
                let s = stamp(&entry.path)?;
                if self.observed.get(&entry.path) != Some(&s) {
                    return Err(error("selection changed; refresh first"));
                }
                marks.insert(entry.path.clone(), s);
            }
            self.marks = marks;
            return Ok(format!("{} marked", self.marks.len()));
        }
        if key == 'A' {
            self.clear_marks();
            return Ok("Marks cleared".into());
        }
        if key == 'r' && !self.marks.is_empty() {
            return Err(error("clear marks before single-item rename"));
        }
        if matches!(key, 'd' | 'R' | 'D') && !self.marks.is_empty() {
            return self.batch_trash(key, model);
        }
        let result = match key {
            'c' | 'x' => {
                self.clipboard = None;
                let reviews = self.reviews(model)?;
                self.clipboard = Some((
                    reviews,
                    if key == 'c' {
                        Transfer::Copy
                    } else {
                        Transfer::Move
                    },
                ));
                self.clear_marks();
                return Ok("Clipboard ready; v to review and paste".into());
            }
            'v' | 'V' => {
                let (reviews, kind) = self
                    .clipboard
                    .as_ref()
                    .ok_or_else(|| error("clipboard empty"))?;
                let kind = *kind;
                let mut items = Vec::with_capacity(reviews.len());
                for review in reviews {
                    let name = review.path().file_name().unwrap();
                    let dst = if key == 'V' {
                        if reviews.len() != 1 {
                            return Err(error("keep both requires one clipboard item"));
                        }
                        let candidate =
                            terminal_tree::rename_plan::native::suggest(&model.directory, name)?;
                        model.directory.join(candidate)
                    } else {
                        model.directory.join(name)
                    };
                    self.ordinary(review.path())?;
                    self.ordinary(&dst)?;
                    items.push((review.clone(), self.engine.destination(&dst)?));
                }
                let total = items.len();
                if total > 1 {
                    self.engine.validate_batch(&items, kind)?;
                }
                let paths: Vec<_> = items
                    .iter()
                    .flat_map(|(r, d)| [r.path(), d.path()])
                    .collect();
                let title = if total == 1 {
                    if matches!(kind, Transfer::Copy) {
                        "Copy item".into()
                    } else {
                        "Move item".into()
                    }
                } else {
                    format!(
                        "{} {total} items - each committed separately",
                        if matches!(kind, Transfer::Copy) {
                            "Copy"
                        } else {
                            "Move"
                        }
                    )
                };
                let quick = self.quick_review && total == 1 && matches!(kind, Transfer::Copy);
                if !confirm_review(&title, &paths, "yes", quick)? {
                    return Ok("Cancelled".into());
                }
                if total > 1 {
                    self.engine.validate_batch(&items, kind)?;
                }
                let (done, failure, undos) = worker::run(total, |progress| {
                    let mut done = 0;
                    let mut failure = None;
                    let mut undos = Vec::new();
                    for (review, target) in &items {
                        let result = progress.item(path_text(review.path()), done).and_then(|_| {
                            self.engine
                                .transfer_reviewed_with(review, target, kind, &mut |point| {
                                    progress.checkpoint(point)
                                })
                        });
                        match result {
                            Ok(undo) => {
                                if let Some(u) = undo {
                                    undos.push(u);
                                }
                                done += 1;
                                progress.finished(done);
                            }
                            Err(e) => {
                                failure = Some((review.path().to_path_buf(), e));
                                break;
                            }
                        }
                    }
                    Ok((done, failure, undos))
                })?;
                self.undo.extend(undos);
                if matches!(kind, Transfer::Move) || failure.is_some() {
                    self.clipboard = None;
                }
                if total == 1 && failure.is_none() {
                    "Transfer completed"
                } else {
                    return self.finish_batch(model, done, total, "Transfer", failure);
                }
            }
            'M' => {
                let (reviews, kind) = self
                    .clipboard
                    .as_ref()
                    .ok_or_else(|| error("cut one regular file first"))?;
                if reviews.len() != 1 || !matches!(kind, Transfer::Move) {
                    return Err(error("cut one regular file first"));
                }
                let review = &reviews[0];
                let path = model.directory.join(review.path().file_name().unwrap());
                self.ordinary(review.path())?;
                self.ordinary(&path)?;
                let target = self.engine.destination(&path)?;
                let observed = self.engine.inspect_file_transfer(review, &target)?;
                if !observed.crosses_devices() {
                    return Err(error("use normal paste for same-device moves"));
                }
                let policies = vec![
                    "Strict metadata (default)".to_string(),
                    "Allow modification time rounding only (up to 2 seconds)".to_string(),
                ];
                let Some(policy) = super::choose("Cross-device metadata policy", &policies)? else {
                    return Ok("Cancelled".into());
                };
                let allow_time_rounding = policy == 1;
                if !confirm(
                    if allow_time_rounding {
                        "Retain original; allow mtime rounding"
                    } else {
                        "Cross-device move: keep original in private retention"
                    },
                    &[review.path(), target.path()],
                    if allow_time_rounding {
                        "round-time"
                    } else {
                        "yes"
                    },
                )? {
                    return Ok("Cancelled".into());
                }
                let retained = worker::run(1, |progress| {
                    progress.item(path_text(review.path()), 0)?;
                    let retained = self.engine.move_retaining_source_with_time_rounding(
                        review,
                        &target,
                        allow_time_rounding,
                        &mut |point| progress.checkpoint(point),
                    )?;
                    progress.finished(1);
                    Ok(retained)
                });
                self.clipboard = None;
                let retained = retained?;
                let (width, _) = terminal::size()?;
                super::page(
                    "Cross-device move: original retained",
                    &wrap(
                        &format!(
                            "Original preserved at {}. Do not delete until independently verified.",
                            path_text(&retained)
                        ),
                        usize::from(width).saturating_sub(1).max(1),
                    ),
                )?;
                "Moved; original retained"
            }
            'N' => {
                use terminal_tree::rename_plan::native::reviewed::Batch;

                if self.marks.is_empty() {
                    return Err(error("mark files before bulk rename"));
                }
                let sources = self.reviews(model)?;
                let Some(needle) = prompt("Replace text:")? else {
                    return Ok("Cancelled".into());
                };
                let Some(replacement) = prompt("With text (empty removes):")? else {
                    return Ok("Cancelled".into());
                };
                let batch = Batch::prepare(
                    &self.engine,
                    &model.directory,
                    &sources,
                    OsStr::new(&needle),
                    OsStr::new(&replacement),
                )?;
                let paths: Vec<_> = batch.paths().flat_map(|(a, b)| [a, b]).collect();
                for path in &paths {
                    self.ordinary(path)?;
                }
                let total = sources.len();
                if !confirm(
                    &format!("Rename {total} items - each committed separately"),
                    &paths,
                    "yes",
                )? {
                    return Ok("Cancelled".into());
                }
                let outcome = worker::run(total, |progress| {
                    let outcome = batch.execute(&self.engine, &mut |index, path, point| {
                        if point == "rename-item" {
                            progress.item(path_text(path), index)?;
                        }
                        progress.checkpoint(point)
                    })?;
                    progress.finished(outcome.completed);
                    Ok(outcome)
                })?;
                self.undo.extend(outcome.undos);
                return self.finish_batch(
                    model,
                    outcome.completed,
                    total,
                    "Rename",
                    outcome.failure,
                );
            }
            'r' => {
                let review = self.review(model)?;
                let Some(name) = filename()? else {
                    return Ok("Cancelled".into());
                };
                let dst = model.directory.join(name);
                self.ordinary(&dst)?;
                let target = self.engine.destination(&dst)?;
                if !confirm_review("Rename item", &[review.path(), target.path()], "yes", self.quick_review)? {
                    return Ok("Cancelled".into());
                }
                if let Some(undo) = worker::run(1, |p| {
                    p.item(path_text(review.path()), 0)?;
                    self.engine
                        .transfer_reviewed_with(&review, &target, Transfer::Move, &mut |s| {
                            p.checkpoint(s)
                        })
                })? {
                    self.undo.push(undo);
                }
                "Renamed"
            }
            'n' | 't' => {
                let Some(name) = filename()? else {
                    return Ok("Cancelled".into());
                };
                let dst = model.directory.join(name);
                self.ordinary(&dst)?;
                let target = self.engine.destination(&dst)?;
                if !confirm_review("Create item", &[target.path()], "yes", self.quick_review)? {
                    return Ok("Cancelled".into());
                }
                self.engine.create_reviewed(&target, key == 'n')?;
                "Created"
            }
            'u' => {
                let undo = self.undo.last().ok_or_else(|| error("nothing to undo"))?;
                self.ordinary(undo.source())?;
                self.ordinary(undo.destination())?;
                let dst = self.engine.destination(undo.destination())?;
                if !confirm("Undo move", &[undo.source(), dst.path()], "yes")? {
                    return Ok("Cancelled".into());
                }
                self.engine.undo_reviewed(undo, &dst)?;
                self.undo.pop();
                "Undone"
            }
            'd' => {
                let review = self.review(model)?;
                let trash = Trash::new(&self.engine, &self.data)?;
                if !confirm("Move to Trash", &[review.path()], "yes")? {
                    return Ok("Cancelled".into());
                }
                worker::run(1, |p| {
                    p.item(path_text(review.path()), 0)?;
                    trash.send_with(&review, &mut |s| p.checkpoint(s))
                })?;
                "Trashed"
            }
            'R' => {
                let p = self.selected(model)?;
                let trash = Trash::new(&self.engine, &self.data)?;
                let review = trash.review_restore(&p)?;
                self.ordinary(review.destination())?;
                self.selected(model)?;
                if !confirm(
                    "Restore item",
                    &[review.source(), review.destination()],
                    "yes",
                )? {
                    return Ok("Cancelled".into());
                }
                let outcome = worker::run(1, |p| {
                    p.item(path_text(review.source()), 0)?;
                    trash.restore_with(&review, &mut |s| p.checkpoint(s))
                })?;
                return self.refresh(model, format!("{outcome:?}"));
            }
            'D' => {
                let p = self.selected(model)?;
                let trash = Trash::new(&self.engine, &self.data)?;
                let review = trash.review_purge(&p)?;
                self.selected(model)?;
                if !confirm(
                    "PERMANENT DELETE - cannot undo",
                    &[review.source()],
                    "DELETE",
                )? {
                    return Ok("Cancelled".into());
                }
                let outcome = worker::run(1, |p| {
                    p.item(path_text(review.source()), 0)?;
                    trash.purge_with(&review, "DELETE", &mut |s| p.checkpoint(s))
                })?;
                return self.refresh(model, format!("{outcome:?}"));
            }
            'E' => {
                if !self.marks.is_empty() {
                    return Err(error("clear marks before extracting one archive"));
                }
                let source = self.selected(model)?;
                self.ordinary(&source)?;
                let bytes = worker::run(1, |progress| {
                    progress.item(path_text(&source), 0)?;
                    let bytes = self.engine.read_archive_for_extraction(&source)?;
                    progress.cancelled()?;
                    Ok(bytes)
                })?;
                self.selected(model)?;
                let layout = terminal_tree::archive::portable_layout_preview(bytes.as_slice())?;
                let mut rows = vec![format!(
                    "{} directories, {} files, {} bytes; new directory only",
                    layout.directories.len(),
                    layout.files.len(),
                    layout.payload_bytes
                )];
                rows.extend(layout.directories.iter().map(|d| format!("directory {d}")));
                rows.extend(
                    layout
                        .files
                        .iter()
                        .map(|f| format!("file {} bytes {}", f.size, f.name)),
                );
                super::page("Archive layout (review)", &rows)?;
                let Some(name) = filename()? else {
                    return Ok("Cancelled".into());
                };
                let path = model.directory.join(name);
                self.ordinary(&path)?;
                let destination = self.engine.destination(&path)?;
                if !confirm(
                    "Extract to new directory",
                    &[&source, destination.path()],
                    "yes",
                )? {
                    return Ok("Cancelled".into());
                }
                self.selected(model)?;
                self.ordinary(&source)?;
                self.ordinary(&path)?;
                worker::run(1, |progress| {
                    progress.item(path_text(&path), 0)?;
                    self.engine.extract_archive_reviewed_with(
                        &destination,
                        &bytes,
                        &mut |point| progress.checkpoint(point),
                    )?;
                    progress.finished(1);
                    Ok(())
                })?;
                "Archive extracted"
            }
            'S' => {
                self.settings.save(model)?;
                return Ok("Rust view settings saved".into());
            }
            'O' => {
                if !self.marks.is_empty() {
                    return Err(error("clear marks; select the existing published copy"));
                }
                let published = self.selected(model)?;
                self.ordinary(&published)?;
                let Some(original) = prompt("Original absolute path from inspected journal:")?
                else {
                    return Ok("Cancelled".into());
                };
                let original = PathBuf::from(original);
                if !original.is_absolute() {
                    return Err(error("absolute original path required"));
                }
                self.ordinary(&original)?;
                let Some(id) = prompt("Retained move transaction ID:")? else {
                    return Ok("Cancelled".into());
                };
                let Some(name) = filename()? else {
                    return Ok("Cancelled".into());
                };
                let output = model.directory.join(name);
                self.ordinary(&output)?;
                let target = self.engine.destination(&output)?;
                let review = worker::run(1, |progress| {
                    progress.item(path_text(&published), 0)?;
                    self.engine
                        .review_retained_recovery(&original, &published, &id, &mut |point| {
                            progress.checkpoint(point)
                        })
                })?;
                if !confirm(
                    "Recover verified retained copy to NEW path; preserve all evidence",
                    &[&original, review.retained_path(), &published, target.path()],
                    "recover",
                )? {
                    return Ok("Cancelled".into());
                }
                worker::run(1, |progress| {
                    progress.item(path_text(review.retained_path()), 0)?;
                    self.engine
                        .copy_retained_recovery(&review, &target, &mut |point| {
                            progress.checkpoint(point)
                        })?;
                    progress.finished(1);
                    Ok(())
                })?;
                "Recovery copy created; retained and published files preserved"
            }
            'j' => {
                let (records, truncated) = self.engine.recovery_report()?;
                super::recovery::show(&records, truncated)?;
                return Ok(format!(
                    "Pending records: {}{}; inspect journal before retrying",
                    records.len(),
                    if truncated { "+" } else { "" }
                ));
            }
            _ => return Err(error("unsupported operation")),
        };
        self.refresh(model, result.into())
    }
    fn batch_trash(&mut self, key: char, model: &mut Model) -> io::Result<String> {
        let paths = self.selected_paths(model)?;
        let total = paths.len();
        let mut done = 0usize;
        let mut failure = None;
        {
            let trash = Trash::new(&self.engine, &self.data)?;
            match key {
                'd' => {
                    let reviews = self.reviews(model)?;
                    for r in &reviews {
                        trash.validate_send(r)?;
                    }
                    let display: Vec<_> = reviews.iter().map(|r| r.path()).collect();
                    if !confirm(
                        &format!("Trash {total} items - each committed separately"),
                        &display,
                        "yes",
                    )? {
                        return Ok("Cancelled".into());
                    }
                    for r in &reviews {
                        trash.validate_send(r)?;
                    }
                    worker::run(total, |p| {
                        for r in &reviews {
                            match p
                                .item(path_text(r.path()), done)
                                .and_then(|_| trash.send_with(r, &mut |s| p.checkpoint(s)))
                            {
                                Ok(()) => {
                                    done += 1;
                                    p.finished(done);
                                }
                                Err(e) => {
                                    failure = Some((r.path().to_path_buf(), e));
                                    break;
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
                'R' => {
                    let mut reviews = Vec::new();
                    let mut targets = BTreeSet::new();
                    for p in &paths {
                        let r = trash.review_restore(p)?;
                        self.ordinary(r.destination())?;
                        if targets.iter().any(|d: &PathBuf| {
                            d.starts_with(r.destination()) || r.destination().starts_with(d)
                        }) {
                            return Err(error("overlapping restore destinations"));
                        }
                        targets.insert(r.destination().to_path_buf());
                        trash.validate_restore(&r)?;
                        reviews.push(r);
                    }
                    self.selected_paths(model)?;
                    let display: Vec<_> = reviews
                        .iter()
                        .flat_map(|r| [r.source(), r.destination()])
                        .collect();
                    if !confirm(
                        &format!("Restore {total} items - each committed separately"),
                        &display,
                        "yes",
                    )? {
                        return Ok("Cancelled".into());
                    }
                    for r in &reviews {
                        trash.validate_restore(r)?;
                    }
                    worker::run(total, |p| {
                        for r in &reviews {
                            match p
                                .item(path_text(r.source()), done)
                                .and_then(|_| trash.restore_with(r, &mut |s| p.checkpoint(s)))
                            {
                                Ok(terminal_tree::operations::RestoreOutcome::Restored) => {
                                    done += 1;
                                    p.finished(done);
                                }
                                Ok(_) => {
                                    failure = Some((
                                        r.source().to_path_buf(),
                                        error("data restored; metadata pending"),
                                    ));
                                    break;
                                }
                                Err(e) => {
                                    failure = Some((r.source().to_path_buf(), e));
                                    break;
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
                'D' => {
                    let mut reviews = Vec::new();
                    for p in &paths {
                        let r = trash.review_purge(p)?;
                        trash.validate_purge(&r)?;
                        reviews.push(r);
                    }
                    self.selected_paths(model)?;
                    let display: Vec<_> = reviews.iter().map(|r| r.source()).collect();
                    if !confirm(
                        &format!(
                            "PERMANENT DELETE {total} items - cannot undo; may partially complete"
                        ),
                        &display,
                        "DELETE",
                    )? {
                        return Ok("Cancelled".into());
                    }
                    for r in &reviews {
                        trash.validate_purge(r)?;
                    }
                    worker::run(total, |p| {
                        for r in &reviews {
                            match p.item(path_text(r.source()), done).and_then(|_| {
                                trash.purge_with(r, "DELETE", &mut |s| p.checkpoint(s))
                            }) {
                                Ok(terminal_tree::operations::PurgeOutcome::Deleted) => {
                                    done += 1;
                                    p.finished(done);
                                }
                                Ok(_) => {
                                    failure = Some((
                                        r.source().to_path_buf(),
                                        error("data deleted; metadata pending"),
                                    ));
                                    break;
                                }
                                Err(e) => {
                                    failure = Some((r.source().to_path_buf(), e));
                                    break;
                                }
                            }
                        }
                        Ok(())
                    })?;
                }
                _ => return Err(error("unsupported batch action")),
            }
        }
        self.finish_batch(
            model,
            done,
            total,
            match key {
                'd' => "Trash",
                'R' => "Restore",
                _ => "Purge",
            },
            failure,
        )
    }
    fn refresh(&mut self, model: &mut Model, msg: String) -> io::Result<String> {
        model
            .reload()
            .map_err(|e| io::Error::other(format!("operation completed; refresh failed: {e}")))?;
        self.observe(model);
        Ok(msg)
    }
}
