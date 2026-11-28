#![cfg(all(feature = "native-ops", target_os = "linux"))]
use std::fs;
use std::os::unix::fs::symlink;
use std::process::Command;
use terminal_tree::archive::portable_layout_preview;

#[test]
fn archive_build_cli_returns_complete_portable_bytes_or_nothing() {
    let root = std::env::temp_dir().join(format!("tt-archive-build-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let source = root.join("source file");
    fs::write(&source, b"hello").unwrap();
    let alias = root.join("alias");
    symlink(&source, &alias).unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .args(args)
            .output()
            .unwrap()
    };
    use std::ffi::OsStr;
    let os = OsStr::new::<str>;
    let prefix = [os("--archive-build"), os("--stdout")];
    let good = [prefix[0], prefix[1], os("folder/file"), source.as_os_str()];
    let output = run(&good);
    assert!(output.status.success());
    let layout = portable_layout_preview(output.stdout.as_slice()).unwrap();
    assert_eq!(layout.directories, ["folder"]);
    assert_eq!(layout.payload_bytes, 5);
    assert_eq!(&output.stdout[512..517], b"hello");
    for (name, path) in [
        ("../escape", &source),
        ("NUL.txt", &source),
        ("alias", &alias),
        ("directory", &root),
    ] {
        let result = run(&[prefix[0], prefix[1], os(name), path.as_os_str()]);
        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
    }
    let collision = run(&[
        prefix[0],
        prefix[1],
        os("A/file"),
        source.as_os_str(),
        os("a/other"),
        source.as_os_str(),
    ]);
    assert!(!collision.status.success());
    assert!(collision.stdout.is_empty());
    assert_eq!(fs::read(&source).unwrap(), b"hello");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 2);
}
