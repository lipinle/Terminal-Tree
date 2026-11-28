#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::{symlink, DirBuilderExt, PermissionsExt};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use terminal_tree::bookmarks::{snapshot, Bookmarks};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tt-bookmark-snapshot-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new().mode(0o700).create(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn marks() -> Bookmarks {
    let mut marks = Bookmarks::default();
    marks.add(PathBuf::from("/not-required-to-exist")).unwrap();
    marks
        .add(PathBuf::from("/").join(OsString::from_vec(b"raw-\xff".to_vec())))
        .unwrap();
    marks
}

#[test]
fn round_trip_preserves_raw_names_and_detects_damage() {
    let marks = marks();
    let bytes = snapshot::encode(&marks);
    assert_eq!(snapshot::decode(&bytes).unwrap().paths(), marks.paths());
    assert!(snapshot::decode(&bytes[..bytes.len() - 1]).is_err());
    let mut damaged = bytes;
    damaged[12] ^= 1;
    assert!(snapshot::decode(&damaged).is_err());
    assert!(snapshot::decode(&vec![0; 3 * 1024 * 1024]).is_err());
}

#[test]
fn save_and_load_never_replace_existing_destination() {
    let f = Fixture::new();
    let path = f.0.join("saved");
    let marks = marks();
    snapshot::save_new(&path, &marks).unwrap();
    let old = fs::read(&path).unwrap();
    assert_eq!(snapshot::load(&path).unwrap().paths(), marks.paths());
    assert!(snapshot::save_new(&path, &Bookmarks::default()).is_err());
    assert_eq!(fs::read(&path).unwrap(), old);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn unsafe_snapshot_files_and_directories_are_refused() {
    let f = Fixture::new();
    let path = f.0.join("saved");
    snapshot::save_new(&path, &marks()).unwrap();
    let link = f.0.join("link");
    symlink(&path, &link).unwrap();
    assert!(snapshot::load(&link).is_err());
    assert!(snapshot::save_new(&link, &marks()).is_err());
    let hardlink = f.0.join("hardlink");
    fs::hard_link(&path, &hardlink).unwrap();
    assert!(snapshot::load(&path).is_err());
    fs::remove_file(hardlink).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(snapshot::load(&path).is_err());
    let public = f.0.join("public");
    fs::DirBuilder::new().mode(0o755).create(&public).unwrap();
    fs::set_permissions(&public, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(snapshot::save_new(&public.join("new"), &marks()).is_err());
    let alias = f.0.join("alias");
    symlink(&public, &alias).unwrap();
    assert!(snapshot::save_new(&alias.join("new"), &marks()).is_err());
}
