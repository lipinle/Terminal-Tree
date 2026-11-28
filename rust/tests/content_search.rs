#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use terminal_tree::search::{
    content::{text, ContentLimits},
    SearchLimits,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "tt-content-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn limits() -> ContentLimits {
    ContentLimits {
        traversal: SearchLimits {
            entries: 32,
            depth: 4,
            matches: 8,
        },
        file_bytes: 128,
        total_bytes: 1024,
    }
}

#[test]
fn searches_nested_utf8_and_reports_omissions() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("folder")).unwrap();
    fs::write(f.0.join("folder/hit"), "ไทย needle").unwrap();
    fs::write(f.0.join("miss"), "NEEDLE").unwrap();
    fs::write(f.0.join("binary"), b"needle\0").unwrap();
    fs::write(f.0.join("invalid"), b"needle\xff").unwrap();
    fs::write(f.0.join("large"), vec![b'a'; 129]).unwrap();
    fs::write(f.0.join(".hidden"), b"needle").unwrap();
    symlink(f.0.join("folder/hit"), f.0.join("link")).unwrap();
    let cancel = AtomicBool::new(false);
    let report = text(&f.0, "needle", false, limits(), &cancel).unwrap();
    assert_eq!(report.matches, vec![f.0.join("folder/hit").canonicalize().unwrap()]);
    assert_eq!(report.files_read, 4);
    assert_eq!(report.skipped_binary, 2);
    assert_eq!(report.skipped_large, 1);
    assert_eq!(report.skipped_nonregular, 1);
    assert_eq!(text(&f.0, "needle", true, limits(), &cancel).unwrap().matches.len(), 2);
    assert_eq!(fs::read(f.0.join("folder/hit")).unwrap(), "ไทย needle".as_bytes());
}

#[test]
fn aggregate_and_match_limits_fail_without_partial_results() {
    let f = Fixture::new();
    fs::write(f.0.join("a"), b"needle").unwrap();
    fs::write(f.0.join("b"), b"needle").unwrap();
    let cancel = AtomicBool::new(false);
    let mut budget = limits();
    budget.total_bytes = 10;
    assert!(text(&f.0, "needle", false, budget, &cancel).is_err());
    budget = limits();
    budget.traversal.matches = 1;
    assert!(text(&f.0, "needle", false, budget, &cancel).is_err());
}

#[test]
fn cancellation_and_invalid_queries_fail() {
    let f = Fixture::new();
    let cancel = AtomicBool::new(true);
    assert_eq!(
        text(&f.0, "needle", false, limits(), &cancel).unwrap_err().kind(),
        std::io::ErrorKind::Interrupted
    );
    cancel.store(false, Ordering::Relaxed);
    assert!(text(&f.0, "", false, limits(), &cancel).is_err());
    assert!(text(&f.0, &"x".repeat(4097), false, limits(), &cancel).is_err());
    let mut budget = limits();
    budget.file_bytes = 0;
    assert!(text(&f.0, "needle", false, budget, &cancel).is_err());
}

#[test]
fn cli_content_results_and_omissions_are_separate() {
    use std::process::Command;
    use terminal_tree::display_name;

    let f = Fixture::new();
    fs::create_dir(f.0.join("sub")).unwrap();
    fs::write(f.0.join("sub/hit"), b"needle").unwrap();
    fs::write(f.0.join("needle"), b"not a match").unwrap();
    fs::write(f.0.join("binary"), b"needle\0").unwrap();
    let run = |options: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--find")
            .arg(&f.0)
            .arg("needle")
            .args(options)
            .output()
            .unwrap()
    };
    let output = run(&["--content", "--depth", "2"]);
    assert!(output.status.success());
    let expected = display_name(f.0.join("sub/hit").canonicalize().unwrap().as_os_str());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim_end(),
        expected
    );
    let summary = String::from_utf8(output.stderr).unwrap();
    assert!(summary.contains("1 matches"));
    assert!(summary.contains("1 binary/non-UTF-8"));
    let duplicate = run(&["--content", "--content"]);
    assert!(!duplicate.status.success());
    assert!(duplicate.stdout.is_empty());
    assert_eq!(fs::read(f.0.join("sub/hit")).unwrap(), b"needle");
}
