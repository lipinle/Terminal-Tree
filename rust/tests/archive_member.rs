#![cfg(all(feature = "native-ops", target_os = "linux"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use terminal_tree::archive::Kind;
use terminal_tree::archive_build::{build, Entry};

#[test]
fn member_cli_requires_complete_valid_archive_and_explicit_stdout() {
    let root = std::env::temp_dir().join(format!("tt-archive-member-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let bytes = build(&[
        Entry {
            name: "folder/file",
            kind: Kind::File,
            data: b"a\0b\xff",
        },
        Entry {
            name: "empty",
            kind: Kind::File,
            data: b"",
        },
    ])
    .unwrap();
    let path = root.join("data.tar");
    fs::write(&path, &bytes).unwrap();
    let run = |path: &std::path::Path, name: &str, stdout: bool| {
        let mut command = Command::new(env!("CARGO_BIN_EXE_terminal-tree"));
        command.arg("--archive-member").arg(path).arg(name);
        if stdout {
            command.arg("--stdout");
        }
        command.output().unwrap()
    };
    let output = run(&path, "folder/file", true);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"a\0b\xff");
    let empty = run(&path, "empty", true);
    assert!(empty.status.success());
    assert!(empty.stdout.is_empty());
    for name in ["folder", "missing", "../escape"] {
        let output = run(&path, name, true);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let output = run(&path, "folder/file", false);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let damaged = root.join("damaged.tar");
    let mut bad = bytes.clone();
    *bad.last_mut().unwrap() = 1;
    fs::write(&damaged, bad).unwrap();
    let alias = root.join("alias");
    symlink(&path, &alias).unwrap();
    for bad in [&damaged, &alias, &root] {
        let output = run(bad, "folder/file", true);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert!(!root.join("folder").exists());
}
