#![cfg(all(feature = "native-ops", target_os = "linux"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;

#[test]
fn name_suggestion_cli_observes_conflicts_without_writing() {
    let root = std::env::temp_dir().join(format!("tt-suggest-cli-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    fs::write(root.join("report"), b"original").unwrap();
    symlink(root.join("absent"), root.join("report (1)")).unwrap();
    let run = |name: &str| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--suggest-name")
            .arg(&root)
            .arg(name)
            .output()
            .unwrap()
    };
    let output = run("report");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"report (2)\n");
    let free = run("free");
    assert!(free.status.success());
    assert_eq!(free.stdout, b"free\n");
    let invalid = run("../escape");
    assert!(!invalid.status.success());
    assert!(invalid.stdout.is_empty());
    assert_eq!(fs::read(root.join("report")).unwrap(), b"original");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
}
