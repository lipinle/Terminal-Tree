#![cfg(feature = "native-ops")]
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::{fs, path::PathBuf};
use terminal_tree::operations::{Engine, Transfer};

#[test]
#[ignore = "requires disposable real filesystem mount created by CI"]
fn filesystem_compatibility() {
    assert!(!rustix::process::geteuid().is_root());
    let home = PathBuf::from(std::env::var_os("TT_TEST_HOME").unwrap());
    let external = PathBuf::from(std::env::var_os("TT_TEST_EXTERNAL_ROOT").unwrap());
    let fs_type = std::env::var("TT_TEST_FS").unwrap();
    let journal = home.join("journal");
    fs::create_dir(&journal).unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).unwrap();
    let e = Engine::new_personal(&home, &journal, &[external.clone()], &[]).unwrap();
    let src = home.join("source");
    let dest = external.join("copy");
    fs::write(&src, b"verified data").unwrap();
    fs::set_permissions(&src, fs::Permissions::from_mode(0o600)).unwrap();
    xattr::set(&src, "user.tt.compat", b"metadata").unwrap();
    let review = e.review(&src).unwrap();
    assert!(e.transfer(&review, &dest, Transfer::Move).is_err());
    assert!(!dest.exists());
    if fs_type == "exfat" {
        // This source has an xattr exFAT cannot preserve. No downgrade to
        // a data-only copy, no publication, and no damage to the original.
        assert!(e.transfer(&review, &dest, Transfer::Copy).is_err());
        assert!(!dest.exists());
        assert_eq!(fs::read(&src).unwrap(), b"verified data");
        assert_eq!(
            xattr::get(&src, "user.tt.compat").unwrap().unwrap(),
            b"metadata"
        );
        println!("TT_FS_COMPAT exfat: incompatible metadata refused, source preserved");
        return;
    }
    e.transfer(&review, &dest, Transfer::Copy).unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"verified data");
    assert_eq!(
        xattr::get(&dest, "user.tt.compat").unwrap().unwrap(),
        b"metadata"
    );
    assert_eq!(
        fs::metadata(&src).unwrap().mtime_nsec(),
        fs::metadata(&dest).unwrap().mtime_nsec()
    );
    assert_eq!(fs::metadata(&dest).unwrap().mode() & 0o777, 0o600);
    let tree = home.join("tree");
    fs::create_dir(&tree).unwrap();
    fs::write(tree.join("data"), b"tree data").unwrap();
    fs::hard_link(tree.join("data"), tree.join("hard")).unwrap();
    symlink("missing", tree.join("link")).unwrap();
    xattr::set(tree.join("data"), "user.tt.compat", b"attr").unwrap();
    let copy = external.join("tree");
    e.transfer(&e.review(&tree).unwrap(), &copy, Transfer::Copy)
        .unwrap();
    assert_eq!(
        fs::metadata(copy.join("data")).unwrap().ino(),
        fs::metadata(copy.join("hard")).unwrap().ino()
    );
    assert_eq!(
        fs::read_link(copy.join("link")).unwrap(),
        PathBuf::from("missing")
    );
    let moved = external.join("moved");
    let undo = e
        .transfer(&e.review(&copy).unwrap(), &moved, Transfer::Move)
        .unwrap()
        .expect("verified move must grant undo");
    assert!(!copy.exists());
    assert_eq!(fs::read(moved.join("data")).unwrap(), b"tree data");
    e.undo(&undo).unwrap();
    assert!(!moved.exists());
    let back = home.join("returned");
    e.transfer(&e.review(&copy).unwrap(), &back, Transfer::Copy)
        .unwrap();
    assert_eq!(fs::read(back.join("hard")).unwrap(), b"tree data");
    assert_eq!(
        xattr::get(back.join("data"), "user.tt.compat")
            .unwrap()
            .unwrap(),
        b"attr"
    );
    assert!(e.destination(&dest).is_err());
    assert!(e.pending().unwrap().is_empty());
    println!("TT_FS_COMPAT {fs_type}: verified cross-filesystem copy, same-filesystem move, undo, links, attributes, no-overwrite passed");
}
