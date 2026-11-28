#![forbid(unsafe_code)]
// VM driver only. All actual transactions use the shared native engine.
use rustix::fs::{self as rx, Mode, OFlags};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::time::Duration;
use terminal_tree::operations::{Engine, Transfer, Trash};

const ROOT: &str = "/mnt/data";
const CASES: &[&str] = &[
    "copy-partial",
    "copy-before-publish",
    "copy-before-sync",
    "copy-after-sync",
    "copy-done",
    "move-before-sync",
    "move-after-sync",
    "move-done",
    "trash-journal",
    "trash-info-synced",
    "trash-before-sync",
    "trash-after-sync",
    "trash-done",
    "restore-journal",
    "restore-before-sync",
    "restore-after-sync",
    "restore-info-before-sync",
    "restore-info-after-sync",
    "restore-done",
    "purge-journal",
    "purge-entry-before-sync",
    "purge-entry-after-sync",
    "purge-data-synced",
    "purge-info-before-sync",
    "purge-info-after-sync",
    "purge-done",
];
fn fail(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(ok: bool, message: &str) -> io::Result<()> {
    if ok {
        Ok(())
    } else {
        Err(fail(message))
    }
}
fn new_directory(parent: &File, name: &str) -> io::Result<File> {
    rx::mkdirat(parent, name, Mode::from_bits_truncate(0o700))?;
    let dir: File = rx::openat(
        parent,
        name,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    dir.sync_all()?;
    parent.sync_all()?;
    Ok(dir)
}
fn run() -> io::Result<()> {
    let case = std::env::args()
        .nth(1)
        .ok_or_else(|| fail("missing case"))?;
    let fixture = std::env::args()
        .nth(2)
        .ok_or_else(|| fail("missing fixture"))?;
    require(
        CASES.contains(&case.as_str()) && matches!(fixture.as_str(), "file" | "tree"),
        "unknown crash case",
    )?;
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    require(
        cmdline.split_whitespace().any(|x| x == "tt_engine=rust")
            && cmdline.split_whitespace().any(|x| x == "tt_phase=execute")
            && cmdline
                .split_whitespace()
                .any(|x| x == format!("tt_case={case}"))
            && cmdline
                .split_whitespace()
                .any(|x| x == format!("tt_fixture={fixture}")),
        "refusing to run outside an opted-in crash-test guest",
    )?;
    require(
        fs::read_to_string("/proc/self/mountinfo")?
            .lines()
            .any(|line| {
                let Some((left, right)) = line.split_once(" - ") else {
                    return false;
                };
                left.split_whitespace().nth(4) == Some(ROOT)
                    && right.split_whitespace().take(2).eq(["ext4", "/dev/vda"])
            }),
        "disposable ext4 guest mount required",
    )?;
    let personal =
        case.starts_with("trash-") || case.starts_with("restore-") || case.starts_with("purge-");
    if personal {
        require(
            rustix::process::getuid().as_raw() == 1000
                && rustix::process::geteuid().as_raw() == 1000,
            "unprivileged disposable guest required",
        )?;
        let home = Path::new(ROOT).join("work");
        let journal = Path::new(ROOT).join("state/terminal-tree/operations");
        let data = home.join(".local/share");
        require(
            std::env::var_os("XDG_DATA_HOME").as_deref() == Some(data.as_os_str()),
            "fixture data home mismatch",
        )?;
        let e = Engine::new_personal(&home, &journal, &[], &[])?;
        let trash = Trash::new(&e, &data)?;
        let mut checkpoint = |point: &str| -> io::Result<()> {
            if point == case {
                let mut out = io::stdout().lock();
                writeln!(out, "TT_CUT_READY {case}")?;
                out.flush()?;
                drop(out);
                loop {
                    std::thread::sleep(Duration::from_secs(60));
                }
            }
            Ok(())
        };
        let item = data.join("Trash/files/source.bin");
        if case.starts_with("trash-") {
            trash.send_with(&e.review(&home.join("source.bin"))?, &mut checkpoint)?;
        } else if case.starts_with("restore-") {
            trash.restore_with(&trash.review_restore(&item)?, &mut checkpoint)?;
        } else {
            trash.purge_with(&trash.review_purge(&item)?, "DELETE", &mut checkpoint)?;
        }
        return Err(fail("expected personal cut point was not reached"));
    }
    let root: File = rx::open(
        Path::new(ROOT),
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?
    .into();
    let state = new_directory(&root, "state")?;
    let app = new_directory(&state, "terminal-tree")?;
    new_directory(&app, "operations")?;
    let work = Path::new(ROOT).join("work");
    let journal = Path::new(ROOT).join("state/terminal-tree/operations");
    let engine = Engine::new(&work, &journal)?;
    let review = engine.review(&work.join("source.bin"))?;
    let kind = if case.starts_with("copy-") {
        Transfer::Copy
    } else {
        Transfer::Move
    };
    engine.transfer_with(&review, &work.join("destination.bin"), kind, &mut |point| {
        if point == case {
            let mut out = io::stdout().lock();
            writeln!(out, "TT_CUT_READY {case}")?;
            out.flush()?;
            drop(out);
            loop {
                std::thread::sleep(Duration::from_secs(60));
            }
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
