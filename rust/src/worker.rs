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
    Mutex, MutexGuard,
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
    fn poison_error() -> io::Error {
        io::Error::other(
            "worker progress state poisoned; operation outcome may be uncertain; inspect journal and paths before retrying",
        )
    }
    fn lock_state(&self) -> io::Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(|_| {
            // Never recover or continue with a partially updated commit state.
            self.cancel.store(true, Ordering::Release);
            Self::poison_error()
        })
    }
    pub fn cancellation_flag(&self) -> &AtomicBool {
        &self.cancel
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
        let mut s = self.lock_state()?;
        s.label = label;
        s.done = done;
        s.phase = "Checking reviewed item";
        s.committing = false;
        s.copied = 0;
        Ok(())
    }
    pub fn checkpoint(&self, point: &str) -> io::Result<()> {
        let mut s = self.lock_state()?;
        // Serialize cancel and commit decisions under the same mutex. Once
        // commit begins, never return Interrupted from a post-publish hook.
        // Poison is different: commit state is untrustworthy. Fail closed
        // with an uncertain-outcome error, not an ordinary cancellation.
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
            "copy-before-publish" | "extract-before-publish" => "Checking verified payload",
            "extract-directory" | "extract-data" => "Extracting into private staging",
            "extract-before-verify" => "Verifying staged archive data",
            "hash-chunk" | "manifest-entry" => return Ok(()),
            _ if s.committing => "Committing / syncing; cancellation deferred",
            _ => "Preparing / verifying",
        };
        s.phase = phase;
        Ok(())
    }
    pub fn finished(&self, n: usize) -> io::Result<()> {
        let mut s = self.lock_state()?;
        s.done = n;
        s.committing = false;
        s.phase = "Item committed";
        Ok(())
    }
    fn request_cancel(&self) -> io::Result<()> {
        // Keep ordinary cancellation serialized with the commit decision.
        // lock_state also sets cancellation if the mutex is poisoned.
        let _state = self.lock_state()?;
        self.cancel.store(true, Ordering::Release);
        Ok(())
    }
    fn check_integrity(&self) -> io::Result<()> {
        if self.state.is_poisoned() {
            self.cancel.store(true, Ordering::Release);
            Err(Self::poison_error())
        } else {
            Ok(())
        }
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
    run_modal(total, "File operation in progress", false, job)
}

pub fn run_search<T: Send>(job: impl FnOnce(&Progress) -> io::Result<T> + Send) -> io::Result<T> {
    run_modal(1, "Search in progress", true, job)
}

fn run_modal<T: Send>(
    total: usize,
    title: &str,
    read_only: bool,
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
                        let s = p.lock_state()?;
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
                        Print(super::clip(title, width))
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
                    if bytes && !read_only {
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
                                    if read_only {
                                        "Esc / Ctrl+C: cancel search"
                                    } else {
                                        "Esc / Ctrl+C: request cancellation; commit cannot be interrupted"
                                    }
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
                            p.request_cancel()?;
                        }
                    }
                }
                Ok(())
            })();
            if let Err(e) = draw {
                // Cancellation is still requested on poison; the final
                // integrity check will surface the error after joining.
                let _ = p.request_cancel();
                ui_error = Some(e);
                break;
            }
        }
        // Even terminal failure cannot leave a detached mutation thread.
        let joined = handle.join();
        // Detect poison even when the job swallowed a progress error or
        // completed before the UI could observe the poisoned mutex.
        p.check_integrity()?;
        let result = joined
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
    fn poison(p: &Progress) {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = p.state.lock().unwrap();
            panic!("injected progress panic");
        }));
        assert!(result.is_err());
        assert!(p.state.is_poisoned());
    }

    #[test]
    fn poisoned_progress_refuses_new_work_and_cancellation_does_not_panic() {
        let p = Progress::new(1);
        poison(&p);
        assert!(p.request_cancel().is_err());
        assert!(p.cancellation_flag().load(Ordering::Acquire));
        assert!(p.item("new".into(), 0).is_err());
        let error = p.checkpoint("transfer-commit").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("inspect journal"));
        assert!(p.finished(1).is_err());
        assert!(p.check_integrity().is_err());
    }

    #[test]
    fn poison_after_commit_is_uncertain_not_ordinary_cancellation() {
        let p = Progress::new(1);
        p.item("one".into(), 0).unwrap();
        p.checkpoint("transfer-commit").unwrap();
        poison(&p);
        let error = p.checkpoint("copy-before-sync").unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("outcome may be uncertain"));
        assert!(p.finished(1).is_err());
        assert!(p.check_integrity().is_err());
    }

    #[test]
    fn modal_joins_worker_and_rejects_success_after_poison() {
        let completed = AtomicBool::new(false);
        let result = run(1, |p| {
            poison(p);
            completed.store(true, Ordering::Release);
            Ok(())
        });
        assert!(completed.load(Ordering::Acquire));
        let error = result.unwrap_err();
        assert!(error.to_string().contains("progress state poisoned"));
    }

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
        p.request_cancel().unwrap();
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
        p.request_cancel().unwrap();
        p.checkpoint("copy-before-sync").unwrap();
        p.checkpoint("copy-done").unwrap();
        p.finished(1).unwrap();
        assert!(p.item("two".into(), 1).is_err());
    }
}
