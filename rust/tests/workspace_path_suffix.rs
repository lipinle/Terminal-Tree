#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::fs;
use std::os::unix::fs::DirBuilderExt;
use terminal_tree::operations::Engine;

#[test]
fn workspace_refuses_directory_only_suffixes_without_mutation() {
    let root = std::env::temp_dir().join(format!("tt-workspace-suffix-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let workspace = root.join("workspace");
    let journal = root.join("journal");
    fs::DirBuilder::new().mode(0o700).create(&workspace).unwrap();
    fs::DirBuilder::new().mode(0o700).create(&journal).unwrap();
    let file = workspace.join("file");
    fs::write(&file, b"keep").unwrap();
    let engine = Engine::new(&workspace, &journal).unwrap();
    assert!(engine.review(&file).is_ok());
    assert!(engine.destination(&workspace.join("new")).is_ok());
    for suffix in ["file/", "file/.", "new/", "new/."] {
        let path = workspace.join(suffix);
        assert!(engine.review(&path).is_err());
        assert!(engine.destination(&path).is_err());
    }
    assert_eq!(fs::read(&file).unwrap(), b"keep");
    assert!(!workspace.join("new").exists());
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
}
