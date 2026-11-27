#![forbid(unsafe_code)]
// Test-only native regular-file transaction engine. Never installed or exposed
// to the navigator. All mutations are descriptor-relative in a disposable VM.
use rustix::fs::{self as rx, AtFlags, Mode, OFlags, RenameFlags};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, FileTimes, Metadata};
use std::io::{self, Read, Write};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::time::Duration;
use xattr::FileExt;
#[path = "crash_probe/tree_probe.rs"]
mod tree_probe;

const ROOT: &str = "/mnt/data";
const STAGE: &str = ".terminal-tree-copy-rust-probe";
const SRC: &str = "source.bin";
const DST: &str = "destination.bin";
const CASES: &[&str] = &[
    "copy-partial", "copy-before-publish", "copy-before-sync",
    "copy-after-sync", "copy-done", "move-before-sync", "move-after-sync", "move-done",
];

fn fail(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn require(condition: bool, message: &str) -> io::Result<()> {
    if condition { Ok(()) } else { Err(fail(message)) }
}
fn cut(case: &str, point: &str) -> io::Result<()> {
    if case == point {
        let mut out = io::stdout().lock();
        writeln!(out, "TT_CUT_READY {case}")?;
        out.flush()?;
        drop(out);
        loop { std::thread::sleep(Duration::from_secs(60)); }
    }
    Ok(())
}
fn directory_at(parent: &File, name: &str) -> io::Result<File> {
    Ok(rx::openat(parent, name, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into())
}
fn new_directory(parent: &File, name: &str) -> io::Result<File> {
    rx::mkdirat(parent, name, Mode::from_bits_truncate(0o700))?;
    let dir = directory_at(parent, name)?;
    dir.sync_all()?;
    parent.sync_all()?;
    Ok(dir)
}
fn read_file(parent: &File, name: &str) -> io::Result<File> {
    let f: File = rx::openat(parent, name, OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC, Mode::empty())?.into();
    require(f.metadata()?.is_file(), "only regular files supported")?;
    Ok(f)
}
fn new_file(parent: &File, name: &str) -> io::Result<File> {
    Ok(rx::openat(parent, name, OFlags::RDWR | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::from_bits_truncate(0o600))?.into())
}
fn identity(m: &Metadata) -> (u64, u64, u32, u64, i64, i64, i64, i64) {
    (m.dev(), m.ino(), m.mode(), m.size(), m.mtime(), m.mtime_nsec(), m.ctime(), m.ctime_nsec())
}
fn same_source(parent: &File, original: &Metadata) -> io::Result<()> {
    require(identity(&read_file(parent, SRC)?.metadata()?) == identity(original), "source identity changed")
}
fn attributes(f: &File) -> io::Result<BTreeMap<OsString, Vec<u8>>> {
    let mut result = BTreeMap::new();
    for key in f.list_xattr()? {
        let value = f.get_xattr(&key)?.ok_or_else(|| fail("xattr disappeared"))?;
        result.insert(key, value);
    }
    Ok(result)
}
fn digest(mut f: &File) -> io::Result<[u8; 32]> {
    let before = identity(&f.metadata()?);
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 { break; }
        h.update(&buf[..n]);
    }
    require(identity(&f.metadata()?) == before, "file changed while hashing")?;
    Ok(h.finalize().into())
}

fn journal(root: &File, case: &str) -> io::Result<File> {
    // First-use creation is synced all the way to the already durable mount.
    let state = new_directory(root, "state")?;
    let app = new_directory(&state, "terminal-tree")?;
    let dir = new_directory(&app, "operations")?;
    let is_copy = case.starts_with("copy-");
    let record = serde_json::json!({
        "id": "rust-probe",
        "operation": if is_copy { "copy" } else { "move" },
        "source": "/mnt/data/work/source.bin",
        "destination": "/mnt/data/work/destination.bin",
        "stage": if is_copy { Some("/mnt/data/work/.terminal-tree-copy-rust-probe") } else { None },
        "started": "test-only"
    });
    let mut file = new_file(&dir, "rust-probe.pending.json")?;
    serde_json::to_writer(&mut file, &record).map_err(io::Error::other)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    dir.sync_all()?;
    Ok(dir)
}

fn complete(dir: &File) -> io::Result<()> {
    rx::renameat_with(dir, "rust-probe.pending.json", dir, "rust-probe.done.json", RenameFlags::NOREPLACE)?;
    dir.sync_all()
}
fn publish(from: &File, name: &str, to: &File, case: &str, kind: &str) -> io::Result<()> {
    from.sync_all()?;
    to.sync_all()?;
    rx::renameat_with(from, name, to, DST, RenameFlags::NOREPLACE)?;
    cut(case, &format!("{kind}-before-sync"))?;
    to.sync_all()?;
    from.sync_all()?;
    cut(case, &format!("{kind}-after-sync"))
}

fn copy(work: &File, original: &Metadata, source: &File, case: &str) -> io::Result<()> {
    let expected_hash = digest(source)?;
    let attrs = attributes(source)?;
    same_source(work, original)?;
    let stage = new_directory(work, STAGE)?;
    let mut payload = new_file(&stage, "payload")?;
    // Reopen at offset zero, and compare the opened descriptor to the reviewed file.
    let reopened = read_file(work, SRC)?;
    require(identity(&reopened.metadata()?) == identity(original), "source changed before copy")?;
    let mut source = &reopened;
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        let n = source.read(&mut buf)?;
        if n == 0 { break; }
        payload.write_all(&buf[..n])?;
        // Actual production-candidate streaming write, not a fake partial payload.
        cut(case, "copy-partial")?;
    }
    for (name, value) in &attrs { payload.set_xattr(name, value)?; }
    payload.set_permissions(original.permissions())?;
    payload.set_times(FileTimes::new().set_modified(original.modified()?))?;
    payload.sync_all()?;
    let verified = read_file(&stage, "payload")?;
    let meta = verified.metadata()?;
    require(meta.uid() == original.uid() && meta.gid() == original.gid()
        && meta.mode() == original.mode() && meta.size() == original.size()
        && meta.mtime() == original.mtime() && meta.mtime_nsec() == original.mtime_nsec()
        && attributes(&verified)? == attrs && digest(&verified)? == expected_hash,
        "copy content or metadata mismatch")?;
    require(identity(&source.metadata()?) == identity(original), "source changed during copy")?;
    same_source(work, original)?;
    cut(case, "copy-before-publish")?;
    publish(&stage, "payload", work, case, "copy")?;
    // No recursive deletion: only the now-empty pinned staging directory.
    rx::unlinkat(work, STAGE, AtFlags::REMOVEDIR)?;
    work.sync_all()
}

fn run() -> io::Result<()> {
    let case = std::env::args().nth(1).ok_or_else(|| fail("missing case"))?;
    require(CASES.contains(&case.as_str()), "unknown crash case")?;
    let cmdline = fs::read_to_string("/proc/cmdline")?;
    require(cmdline.split_whitespace().any(|x| x == "tt_engine=rust")
        && cmdline.split_whitespace().any(|x| x == "tt_phase=execute")
        && cmdline.split_whitespace().any(|x| x == format!("tt_case={case}")),
        "refusing to run outside an opted-in crash-test guest")?;
    let mountinfo = fs::read_to_string("/proc/self/mountinfo")?;
    require(mountinfo.lines().any(|line| {
        let Some((left, right)) = line.split_once(" - ") else { return false };
        left.split_whitespace().nth(4) == Some(ROOT)
            && right.split_whitespace().take(2).eq(["ext4", "/dev/vda"])
    }), "disposable ext4 guest mount required")?;
    let root: File = rx::open(Path::new(ROOT), OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC, Mode::empty())?.into();
    let work = directory_at(&root, "work")?;
    let fixture = std::env::args().nth(2).unwrap_or_else(|| "file".into());
    require(matches!(fixture.as_str(), "file" | "tree")
        && cmdline.split_whitespace().any(|x| x == format!("tt_fixture={fixture}")),
        "fixture must match guest command line")?;
    let journal = journal(&root, &case)?;
    if fixture == "tree" {
        tree_probe::transact(&work, &case)?;
    } else {
        let source = read_file(&work, SRC)?;
        let original = source.metadata()?;
        require(original.nlink() == 1, "test engine accepts only single-link regular files")?;
        if case.starts_with("copy-") {
            copy(&work, &original, &source, &case)?;
        } else {
            source.sync_all()?;
            same_source(&work, &original)?;
            publish(&work, SRC, &work, &case, "move")?;
        }
    }
    complete(&journal)?;
    cut(&case, if case.starts_with("copy-") { "copy-done" } else { "move-done" })?;
    Err(fail("expected cut point was not reached"))
}
fn main() {
    if let Err(e) = run() {
        eprintln!("TT_FAIL native crash probe: {e}");
        std::process::exit(1);
    }
}
