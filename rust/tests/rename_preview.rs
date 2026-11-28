#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "tt-rename-preview-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn run(dir: &Path, names: &[OsString]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
        .arg("--rename-plan")
        .arg(dir)
        .args(["old", "new"])
        .args(names)
        .output()
        .unwrap()
}

#[test]
fn preview_preserves_native_names_order_and_contents() {
    let f = Fixture::new();
    let raw = OsString::from_vec(b"old-\xff".to_vec());
    fs::write(f.0.join(&raw), b"raw").unwrap();
    fs::write(f.0.join("old-a"), b"a").unwrap();
    let result = run(&f.0, &[raw.clone(), "old-a".into()]);
    assert!(result.status.success());
    assert_eq!(result.stdout, b"old-\\xff\tnew-\\xff\nold-a\tnew-a\n");
    assert_eq!(fs::read(f.0.join(raw)).unwrap(), b"raw");
    assert_eq!(fs::read(f.0.join("old-a")).unwrap(), b"a");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 2);
}

#[test]
fn conflict_or_invalid_selection_produces_no_partial_plan() {
    let f = Fixture::new();
    fs::write(f.0.join("old-a"), b"a").unwrap();
    fs::write(f.0.join(".old-b"), b"b").unwrap();
    fs::write(f.0.join(".new-b"), b"occupied").unwrap();
    for names in [
        vec!["old-a".into(), ".old-b".into()],
        vec!["old-a".into(), "missing-old".into()],
        vec!["../old-a".into()],
        vec!["old-a".into(), "old-a".into()],
        vec![],
    ] {
        let result = run(&f.0, &names);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    assert_eq!(fs::read(f.0.join(".new-b")).unwrap(), b"occupied");
    assert_eq!(fs::read_dir(&f.0).unwrap().count(), 3);
}

#[test]
fn directory_alias_and_scan_budget_are_refused() {
    let f = Fixture::new();
    let dir = f.0.join("real");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("old-a"), b"a").unwrap();
    let alias = f.0.join("alias");
    symlink(&dir, &alias).unwrap();
    let names = ["old-a".into()];
    assert!(!run(&alias, &names).status.success());
    for i in 0..16_384 {
        fs::write(dir.join(format!("other-{i}")), b"").unwrap();
    }
    let result = run(&dir, &names);
    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert_eq!(fs::read(dir.join("old-a")).unwrap(), b"a");
    assert_eq!(fs::read_dir(&dir).unwrap().count(), 16_385);
}
