//! Responsive modal worker. One operation at a time; never detach a writer.
use crossterm::{
    cursor::MoveTo,
    event::{self, Event, KeyCode, KeyModifiers},
    queue,
    style::Print,
    terminal::{self, Clear, ClearType},
};
use std::io::{self, Write};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};

pub struct Progress {
    cancel: AtomicBool,
    state: Mutex<State>,
}
struct State {
    label: String,
    phase: &'static str,
    done: usize,
    total: usize,
    committing: bool,
    copied: u64,
}
impl Progress {
    fn new(total: usize) -> Self {
        Self {
            cancel: AtomicBool::new(false),
            state: Mutex::new(State {
                label: String::new(),
                phase: "Preparing",
                done: 0,
                total,
                committing: false,
                copied: 0,
            }),
        }
    }
    pub fn cancelled(&self) -> io::Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "cancel requested; inspect pending journal/stage if present",
            ))
        } else {
            Ok(())
        }
    }
    pub fn item(&self, label: String, done: usize) -> io::Result<()> {
        self.cancelled()?;
        let mut s = self.state.lock().unwrap();
        s.label = label;
        s.done = done;
        s.phase = "Checking reviewed item";
        s.committing = false;
        s.copied = 0;
        Ok(())
    }
    pub fn checkpoint(&self, point: &str) -> io::Result<()> {
        let mut s = self.state.lock().unwrap();
        // Serialize cancel and commit decisions under the same mutex. Once
        // commit begins, never return Interrupted from a post-publish hook.
        if !s.committing {
            self.cancelled()?;
            if point.ends_with("-commit") {
                s.committing = true;
            }
        }
        if let Some(n) = point
            .strip_prefix("copy-bytes:")
            .and_then(|s| s.parse::<u64>().ok())
        {
            s.copied = s.copied.saturating_add(n);
            return Ok(());
        }
        let phase = match point {
            "copy-partial" | "copy-entry" => "Copying into private staging",
            "copy-reflink" => "Cloned; verification required",
            "copy-verify-stage" => "Verifying and syncing staged data",
            "copy-verify-source" | "move-verify-source" => "Rechecking original data",
            "move-verify-result" => "Verifying committed result",
            "copy-before-publish" => "Checking verified payload",
            "hash-chunk" | "manifest-entry" => return Ok(()),
            _ if s.committing => "Committing / syncing; cancellation deferred",
            _ => "Preparing / verifying",
        };
        s.phase = phase;
        Ok(())
    }
    pub fn finished(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        s.done = n;
        s.committing = false;
        s.phase = "Item committed";
    }
    fn request_cancel(&self) {
        let _state = self.state.lock().unwrap();
        self.cancel.store(true, Ordering::Release);
    }
}

// Always reserve the last row for cancellation guidance when it fits.
fn progress_rows(height: u16) -> (bool, bool, bool, Option<u16>) {
    (
        height >= 3,
        height >= 4,
        height >= 5,
        (height >= 2).then(|| height - 1),
    )
}

pub fn run<T: Send>(
    total: usize,
    job: impl FnOnce(&Progress) -> io::Result<T> + Send,
) -> io::Result<T> {
    let progress = Progress::new(total);
    std::thread::scope(|scope| {
        let p = &progress;
        let handle = scope.spawn(move || job(p));
        let mut ui_error = None;
        let mut last_draw: Option<Instant> = None;
        while !handle.is_finished() {
            let draw = (|| -> io::Result<()> {
                if last_draw.map_or(true, |t| t.elapsed() >= Duration::from_millis(80)) {
                    last_draw = Some(Instant::now());
                    let (w, h) = terminal::size()?;
                    let width = usize::from(w).saturating_sub(1);
                    let (label, phase, done, total, committing, copied) = {
                        let s = p.state.lock().unwrap();
                        (
                            s.label.clone(),
                            s.phase,
                            s.done,
                            s.total,
                            s.committing,
                            s.copied,
                        )
                    };
                    let mut out = io::stdout().lock();
                    queue!(
                        out,
                        MoveTo(0, 0),
                        Clear(ClearType::All),
                        Print(super::clip("File operation in progress", width))
                    )?;
                    let (summary, item, bytes, footer) = progress_rows(h);
                    if summary {
                        queue!(
                            out,
                            MoveTo(0, 1),
                            Print(super::clip(
                                &format!("{done}/{total} items processed | {phase}"),
                                width
                            ))
                        )?;
                    }
                    if item {
                        queue!(out, MoveTo(0, 2), Print(super::clip(&label, width)))?;
                    }
                    if bytes {
                        queue!(out,MoveTo(0,3),Print(super::clip(&format!("Current item: {copied} logical bytes copied (not yet committed)"),width)))?;
                    }
                    if let Some(row) = footer {
                        queue!(
                            out,
                            MoveTo(0, row),
                            Print(super::clip(
                                if p.cancel.load(Ordering::Acquire) {
                                    if committing {
                                        "Cancel requested; finishing committed item safely"
                                    } else {
                                        "Cancel requested; waiting for safe checkpoint"
                                    }
                                } else {
                                    "Esc / Ctrl+C: request cancellation; commit cannot be interrupted"
                                },
                                width
                            ))
                        )?;
                    }
                    out.flush()?;
                    drop(out);
                }
                // Check completion frequently without redrawing the whole terminal.
                if event::poll(Duration::from_millis(10))? {
                    if let Event::Key(k) = event::read()? {
                        if k.code == KeyCode::Esc
                            || (k.code == KeyCode::Char('c')
                                && k.modifiers.contains(KeyModifiers::CONTROL))
                        {
                            p.request_cancel();
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = draw {
                p.request_cancel();
                ui_error = Some(e);
                break;
            }
        }
        // Even terminal failure cannot leave a detached mutation thread.
        let result = handle
            .join()
            .map_err(|_| io::Error::other("worker panicked; inspect journal before retrying"))?;
        match (result, ui_error) {
            (Ok(_), Some(e)) => Err(io::Error::other(format!(
                "operation returned; terminal failed, inspect paths before retrying: {e}"
            ))),
            (result, _) => result,
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn small_terminal_keeps_cancellation_footer_separate() {
        assert_eq!(progress_rows(0), (false, false, false, None));
        assert_eq!(progress_rows(1), (false, false, false, None));
        assert_eq!(progress_rows(2), (false, false, false, Some(1)));
        assert_eq!(progress_rows(3), (true, false, false, Some(2)));
        assert_eq!(progress_rows(4), (true, true, false, Some(3)));
        assert_eq!(progress_rows(5), (true, true, true, Some(4)));
    }

    #[test]
    fn cancellation_before_commit_is_refused() {
        let p = Progress::new(1);
        p.request_cancel();
        assert_eq!(
            p.checkpoint("transfer-commit").unwrap_err().kind(),
            io::ErrorKind::Interrupted
        );
    }
    #[test]
    fn cancellation_after_commit_defers_until_next_item() {
        let p = Progress::new(2);
        p.item("one".into(), 0).unwrap();
        p.checkpoint("transfer-commit").unwrap();
        p.request_cancel();
        p.checkpoint("copy-before-sync").unwrap();
        p.checkpoint("copy-done").unwrap();
        p.finished(1);
        assert!(p.item("two".into(), 1).is_err());
    }
}
