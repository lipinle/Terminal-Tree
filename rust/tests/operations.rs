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

#[test]
fn corrupted_stage_is_not_published() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("source"); let dst = f.root.join("dest");
    fs::write(&src, b"original data").unwrap();
    let review = e.review(&src).unwrap();
    let result = e.transfer_with(&review, &dst, Transfer::Copy, &mut |point| {
        if point == "copy-before-publish" {
            let stage = fs::read_dir(&f.root)?.filter_map(Result::ok)
                .find(|p| p.file_name().to_string_lossy().starts_with(".terminal-tree-copy-")).unwrap();
            fs::write(stage.path().join("payload"), b"modified data")?;
        }
        Ok(())
    });
    assert!(result.is_err()); assert!(!dst.exists());
    assert_eq!(fs::read(&src).unwrap(), b"original data");
    assert_eq!(e.pending().unwrap().len(),1);
}
#[test]
fn source_mutation_during_copy_is_refused() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("source"); let dst = f.root.join("dest");
    fs::write(&src, vec![7u8; 2*1024*1024]).unwrap();
    let review = e.review(&src).unwrap();
    let mut changed = false;
    let result = e.transfer_with(&review, &dst, Transfer::Copy, &mut |point| {
        if point == "copy-partial" && !changed {
            fs::write(&src, vec![9u8; 2*1024*1024])?; changed = true;
        }
        Ok(())
    });
    assert!(changed && result.is_err() && !dst.exists());
    assert_eq!(fs::read(&src).unwrap()[0],9);
}
#[test]
fn moved_result_modified_before_undo_is_not_accepted() {
    let f = Fixture::new(); let e = f.engine();
    let src = f.root.join("source"); let dst = f.root.join("dest");
    fs::write(&src, b"original").unwrap();
    let review = e.review(&src).unwrap();
    let undo = e.transfer_with(&review, &dst, Transfer::Move, &mut |point| {
        if point == "move-after-sync" { fs::write(&dst, b"changed!")?; }
        Ok(())
    }).unwrap();
    assert!(undo.is_none()); assert!(!src.exists());
    assert_eq!(fs::read(dst).unwrap(),b"changed!");
}

#[test]
fn kernel_copy_preserves_sparse_bytes_and_empty_file() {
    use std::io::{Seek,SeekFrom,Write};
    let f=Fixture::new(); let e=f.engine();
    let src=f.root.join("source"); fs::create_dir(&src).unwrap();
    fs::write(src.join("empty"),b"").unwrap();
    let mut sparse=fs::File::create(src.join("sparse")).unwrap();
    sparse.seek(SeekFrom::Start(4*1024*1024)).unwrap(); sparse.write_all(b"end").unwrap();
    let dst=f.root.join("dest");
    e.transfer(&e.review(&src).unwrap(),&dst,Transfer::Copy).unwrap();
    assert_eq!(fs::read(src.join("sparse")).unwrap(),fs::read(dst.join("sparse")).unwrap());
    assert_eq!(fs::metadata(dst.join("empty")).unwrap().len(),0);
    fs::write(dst.join("sparse"),b"changed").unwrap();
    assert_eq!(fs::metadata(src.join("sparse")).unwrap().len(),4*1024*1024+3);
}

#[test]
fn journal_replacement_stops_before_publish() {
    let f=Fixture::new(); let e=f.engine();
    let source=f.root.join("source"); let dest=f.root.join("dest");
    fs::write(&source,b"original").unwrap();
    let reviewed=e.review(&source).unwrap();
    let moved=f.base.join("journal-old");
    let result=e.transfer_with(&reviewed,&dest,Transfer::Copy,&mut |p|{
        if p=="copy-before-publish"{
            fs::rename(&f.journal,&moved)?;
            fs::create_dir(&f.journal)?;
            fs::set_permissions(&f.journal,fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    });
    assert!(result.is_err() && !dest.exists());
    assert_eq!(fs::read(source).unwrap(),b"original");
    assert_eq!(fs::read_dir(&f.journal).unwrap().count(),0);
    assert_eq!(fs::read_dir(moved).unwrap().count(),1);
}
#[test]
fn personal_policy_guards_real_files_and_managed_state() {
    if rustix::process::geteuid().is_root(){return;}
    let f=Fixture::new();
    let home=&f.root;
    let managed=home.join(".local/state/terminal-tree");
    fs::create_dir_all(&managed).unwrap();
    let journal=managed.join("operations");
    fs::create_dir(&journal).unwrap();
    fs::set_permissions(&journal,fs::Permissions::from_mode(0o700)).unwrap();
    let ordinary=home.join("documents");fs::create_dir(&ordinary).unwrap();
    let src=ordinary.join("source");fs::write(&src,b"original").unwrap();
    let outside=f.base.join("outside");fs::create_dir(&outside).unwrap();
    let sentinel=outside.join("sentinel");fs::write(&sentinel,b"outside").unwrap();
    symlink(&outside,home.join("escape")).unwrap();
    symlink(&ordinary,home.join("alias")).unwrap();
    fs::write(home.join(".bashrc"),b"protected").unwrap();
    let e=Engine::new_personal(home,&journal,&[],&[]).unwrap();
    for p in [home.clone(),home.join(".bashrc"),home.join(".local"),managed.clone(),journal.clone(),outside.clone(),
        home.join("escape/sentinel"),home.join("alias/source"),home.join("documents/../.bashrc"),
        home.join(".terminal-tree-copy-test"),PathBuf::from("/etc/passwd")]{
        assert!(e.review(&p).is_err(),"unexpected review: {p:?}");
        assert!(e.destination(&p).is_err(),"unexpected destination: {p:?}");
    }
    let dest=ordinary.join("copy");
    e.transfer(&e.review(&src).unwrap(),&dest,Transfer::Copy).unwrap();
    assert_eq!(fs::read(&dest).unwrap(),b"original");
    let moved=ordinary.join("moved");
    let undo=e.transfer(&e.review(&dest).unwrap(),&moved,Transfer::Move).unwrap().unwrap();
    e.undo(&undo).unwrap();
    assert_eq!(fs::read(&dest).unwrap(),b"original");
    assert_eq!(fs::read(sentinel).unwrap(),b"outside");
    assert!(e.pending().unwrap().is_empty());
    assert!(terminal_tree::operations::Trash::new(&e,home).is_err());
}
#[test]
fn personal_policy_refuses_invalid_external_root() {
    if rustix::process::geteuid().is_root(){return;}
    let f=Fixture::new();
    assert!(Engine::new_personal(&f.root,&f.journal,&[f.base.clone()],&[]).is_err());
    assert!(Engine::new_personal(std::path::Path::new("/"),&f.journal,&[],&[]).is_err());
}

fn personal_trash_fixture(f:&Fixture)->(Engine,PathBuf,PathBuf){
    let engine=Engine::new_personal(&f.root,&f.journal,&[],&[]).unwrap();
    let data=engine.personal_data_home().unwrap().to_path_buf();
    assert!(data.starts_with(&f.root),"test must not touch real XDG data home");
    let root=data.join("Trash");
    for d in [&root,&root.join("files"),&root.join("info")]{
        fs::create_dir_all(d).unwrap();
        fs::set_permissions(d,fs::Permissions::from_mode(0o700)).unwrap();
    }
    (engine,data,root)
}
#[test]
fn personal_native_trash_roundtrip_and_purge(){
    if rustix::process::geteuid().is_root(){return;}
    use terminal_tree::operations::{Trash,RestoreOutcome,PurgeOutcome};
    use std::os::unix::ffi::OsStringExt;
    let f=Fixture::new();
    let (e,data,root)=personal_trash_fixture(&f);
    let trash=Trash::new(&e,&data).unwrap();
    let source=f.root.join(std::ffi::OsString::from_vec(b"a %\xff".to_vec()));
    fs::create_dir(&source).unwrap();
    fs::write(source.join("data"),b"original").unwrap();
    symlink("missing",source.join("link")).unwrap();
    xattr::set(source.join("data"),"user.tt",b"attr").unwrap();
    trash.send(&e.review(&source).unwrap()).unwrap();
    assert!(!source.exists());
    let item=fs::read_dir(root.join("files")).unwrap().next().unwrap().unwrap().path();
    assert_eq!(fs::read(item.join("data")).unwrap(),b"original");
    assert!(e.review(&item).is_err());
    assert!(e.destination(&root.join("files/ordinary")).is_err());
    assert!(trash.review_purge(&item.join("data")).is_err());
    let mut n=item.file_name().unwrap().to_os_string();n.push(".trashinfo");
    let meta=root.join("info").join(n);
    let original=fs::read(&meta).unwrap();
    assert!(String::from_utf8_lossy(&original).contains("a%20%25%FF"));
    assert_eq!(trash.review_restore(&item).unwrap().destination(),source);
    // Restore review invalidated by metadata change and by destination collision.
    let r=trash.review_restore(&item).unwrap();
    fs::write(&meta,b"[Trash Info]\nPath=/etc/passwd\n").unwrap();
    assert!(trash.restore(&r).is_err());
    assert!(trash.review_restore(&item).is_err());
    fs::write(&meta,&original).unwrap();
    let r=trash.review_restore(&item).unwrap();
    fs::write(&source,b"sentinel").unwrap();
    assert!(trash.restore(&r).is_err());
    assert_eq!(fs::read(&source).unwrap(),b"sentinel");
    fs::remove_file(&source).unwrap();
    assert_eq!(trash.restore(&trash.review_restore(&item).unwrap()).unwrap(),RestoreOutcome::Restored);
    assert_eq!(fs::read(source.join("data")).unwrap(),b"original");
    assert_eq!(xattr::get(source.join("data"),"user.tt").unwrap().unwrap(),b"attr");
    assert_eq!(fs::read_link(source.join("link")).unwrap(),PathBuf::from("missing"));
    assert!(!meta.exists());
    trash.send(&e.review(&source).unwrap()).unwrap();
    let item=fs::read_dir(root.join("files")).unwrap().next().unwrap().unwrap().path();
    let r=trash.review_purge(&item).unwrap();
    assert!(trash.purge(&r,"yes").is_err());
    assert!(item.exists());
    assert_eq!(trash.purge(&r,"DELETE").unwrap(),PurgeOutcome::Deleted);
    assert!(!item.exists());
    assert_eq!(fs::read_dir(root.join("info")).unwrap().count(),0);
}
#[test]
fn personal_trash_refuses_swapped_dirs_and_protected_state(){
    if rustix::process::geteuid().is_root(){return;}
    use terminal_tree::operations::Trash;
    let f=Fixture::new();
    let (e,data,root)=personal_trash_fixture(&f);
    let trash=Trash::new(&e,&data).unwrap();
    let src=f.root.join("source");fs::write(&src,b"keep").unwrap();
    let review=e.review(&src).unwrap();
    fs::rename(root.join("files"),root.join("old-files")).unwrap();
    fs::create_dir(root.join("files")).unwrap();
    fs::set_permissions(root.join("files"),fs::Permissions::from_mode(0o700)).unwrap();
    assert!(trash.send(&review).is_err());
    assert_eq!(fs::read(&src).unwrap(),b"keep");
    fs::remove_dir(root.join("files")).unwrap();
    symlink(&f.base,root.join("files")).unwrap();
    assert!(Trash::new(&e,&data).is_err());
}

#[test]
fn batch_preflight_refuses_late_conflicts_and_stale_source(){
    let f=Fixture::new();let e=f.engine();
    let src=f.root.join("source");let dst=f.root.join("destination");
    fs::create_dir(&src).unwrap();fs::create_dir(&dst).unwrap();
    for n in ["a","b"]{fs::write(src.join(n),n).unwrap();}
    let items:Vec<_>=["a","b"].iter().map(|n|(e.review(&src.join(n)).unwrap(),e.destination(&dst.join(n)).unwrap())).collect();
    e.validate_batch(&items,Transfer::Copy).unwrap();
    fs::write(dst.join("b"),b"sentinel").unwrap();
    assert!(e.validate_batch(&items,Transfer::Copy).is_err());
    assert!(!dst.join("a").exists());
    assert_eq!(fs::read(dst.join("b")).unwrap(),b"sentinel");
    fs::remove_file(dst.join("b")).unwrap();
    fs::write(src.join("b"),b"changed").unwrap();
    assert!(e.validate_batch(&items,Transfer::Move).is_err());
    assert!(!dst.join("a").exists());
    assert_eq!(fs::read(src.join("a")).unwrap(),b"a");
    assert!(e.pending().unwrap().is_empty());
}
#[test]
fn batch_refuses_overlapping_sources_and_targets(){
    let f=Fixture::new();let e=f.engine();
    let src=f.root.join("source");fs::create_dir(&src).unwrap();
    fs::write(src.join("child"),b"keep").unwrap();
    let a=e.review(&src).unwrap();
    let b=e.review(&src.join("child")).unwrap();
    let d1=e.destination(&f.root.join("a")).unwrap();
    let d2=e.destination(&f.root.join("b")).unwrap();
    assert!(e.validate_batch(&[(a.clone(),d1.clone()),(b.clone(),d2)],Transfer::Copy).is_err());
    assert!(e.validate_batch(&[(a,d1.clone()),(b,d1)],Transfer::Copy).is_err());
    assert!(!f.root.join("a").exists());
}

#[test]
fn cooperative_cancel_during_copy_preserves_source_and_pending_stage(){
    let f=Fixture::new();let e=f.engine();
    let src=f.root.join("large");let dst=f.root.join("destination");
    fs::write(&src,vec![0x5a;4*1024*1024]).unwrap();
    let r=e.review(&src).unwrap();let d=e.destination(&dst).unwrap();
    let err=e.transfer_reviewed_with(&r,&d,Transfer::Copy,&mut |point|{
        if point=="copy-partial"{Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"cancelled"))}else{Ok(())}
    }).unwrap_err();
    assert_eq!(err.kind(),std::io::ErrorKind::Interrupted);
    assert!(!dst.exists());
    assert_eq!(fs::read(&src).unwrap(),vec![0x5a;4*1024*1024]);
    assert_eq!(e.pending().unwrap().len(),1);
}
#[test]
fn cancel_hash_review_and_move_before_commit(){
    let f=Fixture::new();let e=f.engine();
    let src=f.root.join("source");let dst=f.root.join("destination");
    fs::write(&src,vec![0x33;2*1024*1024]).unwrap();
    assert!(e.review_with(&src,&mut |point|if point=="hash-chunk"{Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"cancelled"))}else{Ok(())}).is_err());
    assert!(e.pending().unwrap().is_empty());
    let r=e.review(&src).unwrap();let d=e.destination(&dst).unwrap();
    let err=e.transfer_reviewed_with(&r,&d,Transfer::Move,&mut |point|{
        if point=="transfer-commit"{Err(std::io::Error::new(std::io::ErrorKind::Interrupted,"cancelled"))}else{Ok(())}
    }).unwrap_err();
    assert_eq!(err.kind(),std::io::ErrorKind::Interrupted);
    assert!(!dst.exists());assert_eq!(fs::read(&src).unwrap(),vec![0x33;2*1024*1024]);
    assert_eq!(e.pending().unwrap().len(),1);
}

#[test]
fn batch_path_index_rejects_nonadjacent_ancestors_and_duplicate_targets(){
    let f=Fixture::new();let e=f.engine();
    let parent=f.root.join("parent");fs::create_dir(&parent).unwrap();
    let child=parent.join("child");fs::write(&child,b"child").unwrap();
    let sibling=f.root.join("parent-other");fs::write(&sibling,b"sibling").unwrap();
    let first=e.destination(&f.root.join("one")).unwrap();
    let second=e.destination(&f.root.join("two")).unwrap();
    let third=e.destination(&f.root.join("three")).unwrap();
    let p=e.review(&parent).unwrap();let c=e.review(&child).unwrap();let s=e.review(&sibling).unwrap();
    assert!(e.validate_batch(&[(p,first.clone()),(s.clone(),second.clone()),(c.clone(),third)],Transfer::Copy).is_err());
    assert!(e.validate_batch(&[(c,first.clone()),(s,first)],Transfer::Copy).is_err());
    assert!(e.pending().unwrap().is_empty());
}

#[test]
fn recovery_report_preserves_bad_records_and_shows_valid_ones(){
    let f=Fixture::new();let e=f.engine();
    let good=f.journal.join("good.pending.json");
    let bad=f.journal.join("bad.pending.json");
    let link=f.journal.join("link.pending.json");
    fs::write(&good,b"{\"operation\":\"copy\"}").unwrap();
    fs::write(&bad,b"{broken").unwrap();
    for p in [&good,&bad]{fs::set_permissions(p,fs::Permissions::from_mode(0o600)).unwrap();}
    symlink(&good,&link).unwrap();
    let (rows,truncated)=e.recovery_report().unwrap();
    assert!(!truncated);assert_eq!(rows.len(),3);
    assert_eq!(rows.iter().filter(|(_,r)|r.is_ok()).count(),1);
    assert_eq!(fs::read(&bad).unwrap(),b"{broken");
    assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_dir(&f.journal).unwrap().count(),3);
}
