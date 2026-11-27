#![cfg(feature = "native-ops")]
use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use terminal_tree::operations::{Engine, Transfer};
struct Fixture { base: PathBuf, root: PathBuf, journal: PathBuf }
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let base = std::env::temp_dir().join(format!("tt-ops-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        fs::create_dir(&base).unwrap();
        let root = base.join("workspace"); let journal = base.join("journal");
        fs::create_dir(&root).unwrap(); fs::create_dir(&journal).unwrap();
        fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).unwrap();
        Self { base, root, journal }
    }
    fn engine(&self) -> Engine { Engine::new(&self.root, &self.journal).unwrap() }
}
impl Drop for Fixture { fn drop(&mut self) { fs::remove_dir_all(&self.base).unwrap(); } }
#[test]
fn copy_preserves_tree_links_and_metadata() {
    use std::os::unix::fs::MetadataExt;
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("src"); let dst = f.root.join("dst");
    fs::create_dir(&src).unwrap(); fs::write(src.join("data"), b"content").unwrap();
    fs::hard_link(src.join("data"), src.join("hard")).unwrap();
    symlink("missing", src.join("link")).unwrap();
    xattr::set(src.join("data"), "user.tt", b"attr").unwrap();
    e.transfer(&e.review(&src).unwrap(), &dst, Transfer::Copy).unwrap();
    assert_eq!(fs::read(dst.join("data")).unwrap(), b"content");
    assert_eq!(fs::metadata(dst.join("data")).unwrap().ino(), fs::metadata(dst.join("hard")).unwrap().ino());
    assert_eq!(fs::read_link(dst.join("link")).unwrap(), PathBuf::from("missing"));
    assert_eq!(xattr::get(dst.join("data"), "user.tt").unwrap().unwrap(), b"attr");
    assert!(e.pending().unwrap().is_empty());
}
#[test]
fn collision_and_stale_review_refused() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("source"); let dst = f.root.join("dest");
    fs::write(&src, b"source").unwrap(); fs::write(&dst, b"untouched").unwrap();
    let review = e.review(&src).unwrap();
    assert!(e.transfer(&review, &dst, Transfer::Copy).is_err());
    fs::remove_file(&dst).unwrap(); fs::write(&src, b"new content").unwrap();
    assert!(e.transfer(&review, &dst, Transfer::Move).is_err());
    assert!(!dst.exists()); assert_eq!(fs::read(&src).unwrap(), b"new content");
}
#[test]
fn move_undo_and_modified_undo_refused() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("src"); let dst = f.root.join("dst");
    fs::create_dir(&src).unwrap(); fs::write(src.join("data"), b"one").unwrap();
    let undo = e.transfer(&e.review(&src).unwrap(), &dst, Transfer::Move).unwrap().unwrap();
    assert!(!src.exists()); e.undo(&undo).unwrap(); assert!(src.exists());
    let undo = e.transfer(&e.review(&src).unwrap(), &dst, Transfer::Move).unwrap().unwrap();
    fs::write(dst.join("data"), b"two").unwrap();
    assert!(e.undo(&undo).is_err()); assert!(!src.exists());
}
#[test]
fn create_is_exclusive_and_scope_is_enforced() {
    let f = Fixture::new(); let e = f.engine();
    let file = f.root.join("file");
    e.create(&file, false).unwrap(); assert!(file.is_file());
    assert!(e.create(&file, false).is_err());
    e.create(&f.root.join("dir"), true).unwrap();
    assert!(e.create(&f.base.join("outside"), false).is_err());
    symlink(&f.base, f.root.join("escape")).unwrap();
    assert!(e.create(&f.root.join("escape/outside"), false).is_err());
    assert!(!f.base.join("outside").exists());
}
#[test]
fn interrupted_copy_keeps_pending_record_and_source() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("src"); let dst = f.root.join("dst");
    fs::write(&src, b"content").unwrap();
    let result = e.transfer_with(&e.review(&src).unwrap(), &dst, Transfer::Copy,
        &mut |point| if point == "copy-partial" { Err(std::io::Error::other("injected failure")) } else { Ok(()) });
    assert!(result.is_err()); assert!(!dst.exists());
    assert_eq!(fs::read(&src).unwrap(), b"content");
    assert_eq!(e.pending().unwrap().len(), 1);
}
