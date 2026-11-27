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

#[test]
fn reviewed_destination_directory_swap_is_refused() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("source"); fs::write(&src, b"data").unwrap();
    let parent = f.root.join("parent"); fs::create_dir(&parent).unwrap();
    let destination = e.destination(&parent.join("dest")).unwrap();
    fs::rename(&parent, f.root.join("old-parent")).unwrap();
    fs::create_dir(&parent).unwrap();
    assert!(e.transfer_reviewed(&e.review(&src).unwrap(), &destination, Transfer::Copy).is_err());
    assert!(!parent.join("dest").exists()); assert!(src.exists());
}
#[test]
fn journal_symlink_is_refused() {
    let f = Fixture::new();
    let symlink_path = f.base.join("journal-link");
    symlink(&f.journal, &symlink_path).unwrap();
    assert!(Engine::new(&f.root, &symlink_path).is_err());
}

fn trash_fixture(f: &Fixture) -> (PathBuf, PathBuf, PathBuf) {
    let data = f.root.join("data"); fs::create_dir(&data).unwrap();
    let trash = data.join("Trash"); fs::create_dir(&trash).unwrap();
    for p in [&trash, &trash.join("files"), &trash.join("info")] {
        if !p.exists() { fs::create_dir(p).unwrap(); }
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    (data, trash.join("files"), trash.join("info"))
}
#[test]
fn restore_checked_trash_metadata_and_reject_collision() {
    use terminal_tree::operations::{Trash, RestoreOutcome};
    let f = Fixture::new(); let e = f.engine();
    let (data, files, info) = trash_fixture(&f);
    let p = files.join("sample"); let destination = f.root.join("original");
    fs::write(&p, b"preserve").unwrap();
    let meta = info.join("sample.trashinfo");
    fs::write(&meta, format!("[Trash Info]\nPath={}\n", destination.display())).unwrap();
    let trash = Trash::new(&e, &data).unwrap();
    let review = trash.review_restore(&p).unwrap();
    fs::write(&destination, b"collision").unwrap();
    assert!(trash.restore(&review).is_err());
    assert_eq!(fs::read(&destination).unwrap(), b"collision"); assert!(p.exists());
    fs::remove_file(&destination).unwrap();
    let review = trash.review_restore(&p).unwrap();
    assert_eq!(trash.restore(&review).unwrap(), RestoreOutcome::Restored);
    assert_eq!(fs::read(destination).unwrap(), b"preserve"); assert!(!meta.exists());
}
#[test]
fn restore_refuses_changed_or_outside_metadata() {
    use terminal_tree::operations::Trash;
    let f = Fixture::new(); let e = f.engine();
    let (data, files, info) = trash_fixture(&f);
    let p = files.join("sample"); fs::write(&p, b"safe").unwrap();
    let meta = info.join("sample.trashinfo");
    let trash = Trash::new(&e, &data).unwrap();
    fs::write(&meta, format!("[Trash Info]\nPath={}\n", f.base.join("outside").display())).unwrap();
    assert!(trash.review_restore(&p).is_err());
    fs::write(&meta, format!("[Trash Info]\nPath={}\n", f.root.join("inside").display())).unwrap();
    let review = trash.review_restore(&p).unwrap();
    fs::write(&meta, format!("[Trash Info]\nPath={}\n", f.root.join("other").display())).unwrap();
    assert!(trash.restore(&review).is_err()); assert!(p.exists());
}
#[test]
fn gio_trash_round_trip_in_private_workspace() {
    use terminal_tree::operations::{Trash, RestoreOutcome};
    let f = Fixture::new(); let e = f.engine();
    let (data, files, _) = trash_fixture(&f);
    let p = f.root.join("original"); fs::write(&p, b"gio fixture").unwrap();
    let trash = Trash::new(&e, &data).unwrap();
    trash.send(&e.review(&p).unwrap()).unwrap();
    assert!(!p.exists());
    let entry = fs::read_dir(files).unwrap().next().unwrap().unwrap().path();
    let review = trash.review_restore(&entry).unwrap();
    assert_eq!(trash.restore(&review).unwrap(), RestoreOutcome::Restored);
    assert_eq!(fs::read(&p).unwrap(), b"gio fixture");
}

#[test]
fn purge_requires_review_and_confirmation_and_preserves_symlink_target() {
    use terminal_tree::operations::{Trash, PurgeOutcome};
    let f = Fixture::new(); let e = f.engine();
    let (data, files, info) = trash_fixture(&f);
    let outside = f.root.join("untouched"); fs::write(&outside, b"keep").unwrap();
    let p = files.join("folder"); fs::create_dir(&p).unwrap();
    fs::write(p.join("file"), b"delete").unwrap();
    symlink(&outside, p.join("link")).unwrap();
    fs::write(info.join("folder.trashinfo"), b"[Trash Info]\nPath=/tmp/unused\n").unwrap();
    let trash = Trash::new(&e, &data).unwrap();
    assert!(trash.review_purge(&outside).is_err());
    let review = trash.review_purge(&p).unwrap();
    assert!(trash.purge(&review, "yes").is_err()); assert!(p.exists());
    fs::write(p.join("added"), b"new").unwrap();
    assert!(trash.purge(&review, "DELETE").is_err()); assert!(p.join("added").exists());
    let review = trash.review_purge(&p).unwrap();
    assert_eq!(trash.purge(&review, "DELETE").unwrap(), PurgeOutcome::Deleted);
    assert!(!p.exists()); assert!(!info.join("folder.trashinfo").exists());
    assert_eq!(fs::read(outside).unwrap(), b"keep");
}
#[test]
fn hard_link_purge_refused_before_any_deletion() {
    use terminal_tree::operations::Trash;
    let f = Fixture::new(); let e = f.engine();
    let (data, files, _) = trash_fixture(&f);
    let p = files.join("folder"); fs::create_dir(&p).unwrap();
    fs::write(p.join("a"), b"keep").unwrap();
    fs::hard_link(p.join("a"), p.join("b")).unwrap();
    let trash = Trash::new(&e, &data).unwrap();
    assert!(trash.review_purge(&p).is_err());
    assert!(p.join("a").exists() && p.join("b").exists());
}
