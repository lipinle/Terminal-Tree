use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use terminal_tree::search::{filenames, SearchLimits};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tt-search-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn limits(entries: usize, depth: usize, matches: usize) -> SearchLimits {
    SearchLimits {
        entries,
        depth,
        matches,
    }
}

#[test]
fn recursive_matching_respects_depth_and_hidden_directories() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("folder/deep")).unwrap();
    fs::create_dir(f.0.join(".secret")).unwrap();
    fs::write(f.0.join("folder/Match.txt"), b"x").unwrap();
    fs::write(f.0.join("folder/deep/match.txt"), b"x").unwrap();
    fs::write(f.0.join(".secret/match.txt"), b"x").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(filenames(&f.0, "match", false, limits(16, 1, 8), &cancel)
        .unwrap()
        .is_empty());
    let shallow = filenames(&f.0, "MATCH", false, limits(16, 2, 8), &cancel).unwrap();
    assert_eq!(shallow.len(), 1);
    assert_eq!(shallow[0].name, "Match.txt");
    assert_eq!(
        filenames(&f.0, "match", true, limits(16, 3, 8), &cancel)
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn budgets_include_hidden_and_nonmatching_entries() {
    let f = Fixture::new();
    fs::write(f.0.join("other"), b"x").unwrap();
    fs::write(f.0.join(".hidden"), b"x").unwrap();
    fs::write(f.0.join("target"), b"x").unwrap();
    let cancel = AtomicBool::new(false);
    assert!(filenames(&f.0, "target", false, limits(2, 1, 1), &cancel).is_err());
    assert_eq!(
        filenames(&f.0, "target", false, limits(3, 1, 1), &cancel)
            .unwrap()
            .len(),
        1
    );
    assert!(filenames(&f.0, "", true, limits(3, 1, 1), &cancel).is_err());
}

#[test]
fn cancellation_and_invalid_limits_return_errors() {
    let f = Fixture::new();
    let cancel = AtomicBool::new(true);
    let error = filenames(&f.0, "", false, limits(8, 2, 8), &cancel).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::Interrupted);
    cancel.store(false, Ordering::Relaxed);
    assert!(filenames(&f.0, "", false, limits(8, 0, 8), &cancel).is_err());
    assert!(filenames(&f.0, "", false, limits(8, 17, 8), &cancel).is_err());
    assert!(filenames(&f.0, "", false, limits(8, 2, 9), &cancel).is_err());
}

#[cfg(unix)]
#[test]
fn links_are_not_recursively_followed_and_raw_paths_survive() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = Fixture::new();
    fs::write(outside.0.join("match-outside"), b"x").unwrap();
    symlink(&outside.0, f.0.join("external")).unwrap();
    symlink(&f.0, f.0.join("loop")).unwrap();
    let raw = OsString::from_vec(b"match-\xff".to_vec());
    let path = f.0.join(raw);
    fs::write(&path, b"x").unwrap();
    let cancel = AtomicBool::new(false);
    let matches = filenames(&f.0, "match", false, limits(8, 4, 8), &cancel).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].path, path.canonicalize().unwrap());
}

#[test]
fn oversized_query_is_refused() {
    let f = Fixture::new();
    let error = filenames(
        &f.0,
        &"x".repeat(4097),
        false,
        limits(8, 2, 8),
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn cli_search_respects_hidden_and_depth_options() {
    use std::process::Command;
    use terminal_tree::display_name;

    let f = Fixture::new();
    fs::create_dir(f.0.join("folder")).unwrap();
    fs::create_dir(f.0.join(".secret")).unwrap();
    fs::write(f.0.join("folder/Match.txt"), b"keep").unwrap();
    fs::write(f.0.join(".secret/match.txt"), b"hidden").unwrap();
    let run = |options: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--find")
            .arg(&f.0)
            .arg("MATCH")
            .args(options)
            .output()
            .unwrap()
    };
    let shallow = run(&["--depth", "1"]);
    assert!(shallow.status.success());
    assert!(shallow.stdout.is_empty());

    let visible = run(&["--depth", "2"]);
    assert!(visible.status.success());
    let expected = display_name(
        f.0.join("folder/Match.txt")
            .canonicalize()
            .unwrap()
            .as_os_str(),
    );
    assert_eq!(
        String::from_utf8(visible.stdout).unwrap().trim_end(),
        expected
    );

    let hidden = run(&["--hidden", "--depth", "2"]);
    assert!(hidden.status.success());
    let output = String::from_utf8(hidden.stdout).unwrap();
    assert_eq!(output.lines().count(), 2);
    assert!(output.contains(&expected));
    let secret = display_name(
        f.0.join(".secret/match.txt")
            .canonicalize()
            .unwrap()
            .as_os_str(),
    );
    assert!(output.contains(&secret));
    assert_eq!(fs::read(f.0.join("folder/Match.txt")).unwrap(), b"keep");
}

#[test]
fn cli_invalid_options_fail_without_results() {
    use std::process::Command;

    let f = Fixture::new();
    fs::write(f.0.join("match"), b"x").unwrap();
    for options in [
        vec!["--depth", "0"],
        vec!["--depth", "17"],
        vec!["--depth"],
        vec!["--hidden", "--hidden"],
        vec!["--depth", "1", "--depth", "2"],
        vec!["--unknown"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--find")
            .arg(&f.0)
            .arg("match")
            .args(options)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
