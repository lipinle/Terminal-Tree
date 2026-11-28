#![cfg(all(feature = "native-ops", target_os = "linux"))]
use std::{fs, process::Command};
use terminal_tree::external_tools;

#[test]
#[ignore = "requires distribution eza, 7-Zip and Git"]
fn installed_tools_inspect_disposable_files_without_mutation() {
    let root = std::env::temp_dir().join(format!("tt-companions-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    fs::write(root.join("sample.txt"), b"keep").unwrap();
    let lines = external_tools::eza(&root).unwrap().join("\n");
    assert!(lines.contains("sample.txt"));
    let seven = ["/usr/bin/7zz", "/usr/bin/7z"]
        .into_iter()
        .find(|path| std::path::Path::new(path).is_file())
        .unwrap();
    let archive = root.join("sample.7z");
    assert!(Command::new(seven)
        .current_dir(&root)
        .args(["a", "sample.7z", "sample.txt"])
        .status()
        .unwrap()
        .success());
    let original = fs::read(&archive).unwrap();
    assert!(external_tools::archive(&archive)
        .unwrap()
        .join("\n")
        .contains("sample.txt"));
    assert_eq!(fs::read(&archive).unwrap(), original);
    assert!(Command::new("/usr/bin/git")
        .current_dir(&root)
        .args(["init", "-q"])
        .status()
        .unwrap()
        .success());
    let status = external_tools::git_status(&root).unwrap().join("\n");
    assert!(status.contains("?? sample.txt"));
    assert!(!root.join(".git/index").exists());
    assert_eq!(fs::read(root.join("sample.txt")).unwrap(), b"keep");
}
