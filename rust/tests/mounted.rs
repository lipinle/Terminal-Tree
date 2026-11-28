#![cfg(feature = "native-ops")]
use std::os::unix::fs::PermissionsExt;
use std::{fs, io::Write, path::PathBuf};
use terminal_tree::operations::{Engine, Transfer};

#[test]
#[ignore = "requires an isolated, size-limited mount created by CI"]
fn mount_policy_and_real_enospc() {
    assert!(!rustix::process::geteuid().is_root());
    let home = PathBuf::from(std::env::var_os("TT_TEST_HOME").expect("fixture home required"));
    let external =
        PathBuf::from(std::env::var_os("TT_TEST_EXTERNAL_ROOT").expect("fixture mount required"));
    let journal = home.join("journal");
    fs::create_dir(&journal).unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).unwrap();
    let e = Engine::new_personal(&home, &journal, std::slice::from_ref(&external), &[]).unwrap();
    let source = home.join("source");
    fs::write(&source, b"preserved data").unwrap();
    assert!(e.destination(&external).is_err());
    assert!(e.destination(&external.join("nested/forbidden")).is_err());
    assert!(e.review(&external.join("nested")).is_err());
    let dest = external.join("small");
    assert!(e
        .transfer(&e.review(&source).unwrap(), &dest, Transfer::Move)
        .is_err());
    assert!(!dest.exists());
    e.transfer(&e.review(&source).unwrap(), &dest, Transfer::Copy)
        .unwrap();
    assert_eq!(fs::read(&dest).unwrap(), b"preserved data");
    let denied = home.join("denied");
    fs::write(&denied, b"private").unwrap();
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o0)).unwrap();
    assert!(e.review(&denied).is_err());
    fs::set_permissions(&denied, fs::Permissions::from_mode(0o600)).unwrap();
    let big = home.join("large");
    let mut file = fs::File::create(&big).unwrap();
    let block = vec![0x5a; 1024 * 1024];
    for _ in 0..16 {
        file.write_all(&block).unwrap();
    }
    file.sync_all().unwrap();
    let target = external.join("too-large");
    let err = e
        .transfer(&e.review(&big).unwrap(), &target, Transfer::Copy)
        .unwrap_err();
    assert_eq!(err.raw_os_error(), Some(28), "expected ENOSPC, got {err:?}");
    assert!(!target.exists());
    assert_eq!(fs::metadata(&big).unwrap().len(), 16 * 1024 * 1024);
    assert_eq!(fs::read(&source).unwrap(), b"preserved data");
    assert_eq!(fs::read(&dest).unwrap(), b"preserved data");
    assert_eq!(e.pending().unwrap().len(), 1);
    println!("TT_MOUNT_POLICY external copy, cross-device move refusal, nested bind mount refusal, permissions and real ENOSPC passed");
}
