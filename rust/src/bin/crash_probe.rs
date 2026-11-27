#![forbid(unsafe_code)]
// VM driver only. All actual transactions use the shared native engine.
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;
use terminal_tree::operations::{Engine, Transfer};

const ROOT: &str = "/mnt/data";
const CASES: &[&str] = &[
    "copy-partial", "copy-before-publish", "copy-before-sync",
    "copy-after-sync", "copy-done", "move-before-sync", "move-after-sync", "move-done",
];
fn fail(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(ok: bool, message: &str) -> io::Result<()> { if ok { Ok(()) } else { Err(fail(message)) } }
fn new_directory(parent: &File, name: &str) -> io::Result<File> {
    rx::mkdirat(parent, name, Mode::from_bits_truncate(0o700))?;
    let dir: File = rx::openat(parent, name, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into();
    dir.sync_all()?;
    parent.sync_all()?;
    Ok(dir)
}
fn run() -> io::Result<()> {
    let case = std::env::args().nth(1).ok_or_else(|| fail("missing case"))?;
    let fixture = std::env::args().nth(2).ok_or_else(|| fail("missing fixture"))?;
    require(CASES.contains(&case.as_str()) && matches!(fixture.as_str(), "file" | "tree"), "unknown crash case")?;
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    require(cmdline.split_whitespace().any(|x| x == "tt_engine=rust")
        && cmdline.split_whitespace().any(|x| x == "tt_phase=execute")
        && cmdline.split_whitespace().any(|x| x == format!("tt_case={case}"))
        && cmdline.split_whitespace().any(|x| x == format!("tt_fixture={fixture}")),
        "refusing to run outside an opted-in crash-test guest")?;
    require(fs::read_to_string("/proc/self/mountinfo")?.lines().any(|line| {
        let Some((left, right)) = line.split_once(" - ") else { return false };
        left.split_whitespace().nth(4) == Some(ROOT)
            && right.split_whitespace().take(2).eq(["ext4", "/dev/vda"])
    }), "disposable ext4 guest mount required")?;
    let root: File = rx::open(Path::new(ROOT), OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into();
    let state = new_directory(&root, "state")?;
    let app = new_directory(&state, "terminal-tree")?;
    new_directory(&app, "operations")?;
    let work = Path::new(ROOT).join("work");
    let journal = Path::new(ROOT).join("state/terminal-tree/operations");
    let engine = Engine::new(&work, &journal)?;
    let review = engine.review(&work.join("source.bin"))?;
    let kind = if case.starts_with("copy-") { Transfer::Copy } else { Transfer::Move };
    engine.transfer_with(&review, &work.join("destination.bin"), kind, &mut |point| {
        if point == case {
            let mut out = io::stdout().lock();
            writeln!(out, "TT_CUT_READY {case}")?;
            out.flush()?;
            drop(out);
            loop { std::thread::sleep(Duration::from_secs(60)); }
        }
        Ok(())
    })?;
    Err(fail("expected cut point was not reached"))
}
fn main() {
    if let Err(e) = run() {
        eprintln!("TT_FAIL native crash probe: {e}");
        std::process::exit(1);
    }
}
