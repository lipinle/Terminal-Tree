#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::fs;
use std::process::Command;
use terminal_tree::archive::native::{list_file, read_member};
use terminal_tree::operations::Engine;
use std::os::unix::fs::PermissionsExt;

#[test]
fn gzip_archive_cli_round_trip_and_scoped_extraction() {
    let root = std::env::temp_dir().join(format!("tt-gzip-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let workspace = root.join("workspace");
    let journal = root.join("journal");
    fs::create_dir(&workspace).unwrap();
    fs::create_dir(&journal).unwrap();
    fs::set_permissions(&journal, fs::Permissions::from_mode(0o700)).unwrap();
    let source = workspace.join("source");
    fs::write(&source, b"hello gzip\0").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
        .args(["--archive-build", "--gzip", "--stdout", "folder/file"])
        .arg(&source)
        .output()
        .unwrap();
    assert!(result.status.success());
    assert!(result.stdout.starts_with(&[0x1f, 0x8b]));
    let archive = workspace.join("data.tar.gz");
    fs::write(&archive, &result.stdout).unwrap();
    assert_eq!(list_file(&archive).unwrap()[0].name, "folder/file");
    assert_eq!(read_member(&archive, "folder/file").unwrap(), b"hello gzip\0");
    let engine = Engine::new(&workspace, &journal).unwrap();
    let bytes = engine.read_archive_for_extraction(&archive).unwrap();
    let dest = workspace.join("extracted");
    engine
        .extract_archive_reviewed(&engine.destination(&dest).unwrap(), &bytes)
        .unwrap();
    assert_eq!(fs::read(dest.join("folder/file")).unwrap(), b"hello gzip\0");
    let broken = workspace.join("broken.tar.gz");
    fs::write(&broken, &result.stdout[..result.stdout.len() - 1]).unwrap();
    assert!(list_file(&broken).is_err());
    assert!(engine.read_archive_for_extraction(&broken).is_err());
    assert_eq!(fs::read(&source).unwrap(), b"hello gzip\0");
    assert_eq!(fs::read(&archive).unwrap(), result.stdout);
}
