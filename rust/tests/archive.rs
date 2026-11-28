use terminal_tree::archive::{list, Kind};

fn header(name: &str, kind: u8, size: usize) -> [u8; 512] {
    let mut h = [0; 512];
    h[..name.len()].copy_from_slice(name.as_bytes());
    h[124..136].copy_from_slice(format!("{size:011o}\0").as_bytes());
    h[148..156].fill(b' ');
    h[156] = kind;
    h[257..265].copy_from_slice(b"ustar\x0000");
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    h
}

fn append(archive: &mut Vec<u8>, name: &str, kind: u8, data: &[u8]) {
    archive.extend_from_slice(&header(name, kind, data.len()));
    archive.extend_from_slice(data);
    let pad = (512 - data.len() % 512) % 512;
    archive.resize(archive.len() + pad, 0);
}

fn finish(archive: &mut Vec<u8>) {
    archive.resize(archive.len() + 1024, 0);
}

#[test]
fn lists_regular_members_in_order() {
    let mut archive = Vec::new();
    append(&mut archive, "folder/", b'5', b"");
    append(&mut archive, "folder/file.txt", b'0', b"hello");
    finish(&mut archive);
    let entries = list(archive.as_slice()).unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "folder");
    assert_eq!(entries[0].kind, Kind::Directory);
    assert_eq!(entries[1].name, "folder/file.txt");
    assert_eq!(entries[1].size, 5);
}

#[test]
fn rejects_unsafe_names_and_unsupported_types() {
    for (name, kind) in [
        ("../outside", b'0'),
        ("/absolute", b'0'),
        ("a//b", b'0'),
        ("C:/outside", b'0'),
        ("a\\b", b'0'),
        ("link", b'2'),
        ("hardlink", b'1'),
        ("pax", b'x'),
        ("device", b'3'),
    ] {
        let mut archive = Vec::new();
        append(&mut archive, name, kind, b"");
        finish(&mut archive);
        assert!(list(archive.as_slice()).is_err(), "{name}");
    }
}

#[test]
fn rejects_duplicate_and_file_ancestor_conflicts() {
    for names in [["a", "a"], ["a/b", "a"], ["a", "a/b"]] {
        let mut archive = Vec::new();
        for name in names {
            append(&mut archive, name, b'0', b"");
        }
        finish(&mut archive);
        assert!(list(archive.as_slice()).is_err());
    }
}

#[test]
fn rejects_damage_truncation_and_trailing_data() {
    let mut archive = Vec::new();
    append(&mut archive, "file", b'0', b"abc");
    finish(&mut archive);
    assert!(list(&archive[..archive.len() - 1]).is_err());
    let mut corrupt = archive.clone();
    corrupt[0] ^= 1;
    assert!(list(corrupt.as_slice()).is_err());
    corrupt = archive.clone();
    corrupt[515] = 1;
    assert!(list(corrupt.as_slice()).is_err());
    archive.extend_from_slice(&[1; 512]);
    assert!(list(archive.as_slice()).is_err());
}

#[test]
fn enforces_member_and_declared_byte_limits() {
    let mut archive = Vec::new();
    for i in 0..4097 {
        append(&mut archive, &format!("file-{i}"), b'0', b"");
    }
    finish(&mut archive);
    assert!(list(archive.as_slice()).is_err());
    let oversized = header("large", b'0', 64 * 1024 * 1024);
    assert!(list(oversized.as_slice()).is_err());
}

#[test]
fn rejects_controls_and_dot_components() {
    for name in [
        "a/./b",
        "./file",
        "file\nname",
        "file\x1bname",
        "file\rname",
    ] {
        let mut archive = Vec::new();
        append(&mut archive, name, b'0', b"");
        finish(&mut archive);
        assert!(list(archive.as_slice()).is_err(), "{name:?}");
    }
}

#[test]
fn empty_basename_with_prefix_is_refused() {
    let mut h = header("", b'5', 0);
    h[345..351].copy_from_slice(b"folder");
    h[148..156].fill(b' ');
    let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
    h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
    let mut archive = h.to_vec();
    finish(&mut archive);
    assert!(list(archive.as_slice()).is_err());
}

#[cfg(all(feature = "native-ops", target_os = "linux"))]
#[test]
fn native_listing_cli_rejects_aliases_and_malformed_data() {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;
    use terminal_tree::archive::native::list_file;

    let root = std::env::temp_dir().join(format!("tt-archive-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("test.tar");
    let mut archive = Vec::new();
    append(&mut archive, "file.txt", b'0', b"hello");
    finish(&mut archive);
    fs::write(&path, &archive).unwrap();
    assert_eq!(list_file(&path).unwrap().len(), 1);
    let run = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--archive-list")
            .arg(path)
            .output()
            .unwrap()
    };
    let output = run(&path);
    assert!(output.status.success());
    assert_eq!(output.stdout, b"file\t5\tfile.txt\n");
    symlink(&path, root.join("alias")).unwrap();
    symlink(&root, root.join("parent-alias")).unwrap();
    for bad in [
        root.join("alias"),
        root.join("parent-alias/test.tar"),
        root.clone(),
    ] {
        let output = run(&bad);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
    let damaged = root.join("damaged.tar");
    fs::write(&damaged, b"bad").unwrap();
    let output = run(&damaged);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert_eq!(fs::read(&path).unwrap(), archive);
}

#[cfg(all(feature = "native-ops", target_os = "linux"))]
#[test]
fn layout_cli_includes_implicit_parents_without_writes() {
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::process::Command;

    let root = std::env::temp_dir().join(format!("tt-archive-layout-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("test.tar");
    let mut archive = Vec::new();
    append(&mut archive, "a/b/one", b'0', b"123");
    append(&mut archive, "a/two", b'0', b"45");
    finish(&mut archive);
    fs::write(&path, &archive).unwrap();
    let run = |path: &std::path::Path| {
        Command::new(env!("CARGO_BIN_EXE_terminal-tree"))
            .arg("--archive-layout")
            .arg(path)
            .current_dir(&root)
            .output()
            .unwrap()
    };
    let output = run(&path);
    assert!(output.status.success());
    assert_eq!(
        output.stdout,
        b"Layout (read-only): 2 directories, 2 files, 5 payload bytes\ndirectory\t0\ta\ndirectory\t0\ta/b\nfile\t3\ta/b/one\nfile\t2\ta/two\n"
    );
    assert!(!root.join("a").exists());
    assert_eq!(fs::read(&path).unwrap(), archive);
    symlink(&path, root.join("alias")).unwrap();
    let damaged = root.join("damaged.tar");
    fs::write(&damaged, b"bad").unwrap();
    for bad in [root.join("alias"), damaged, root.clone()] {
        let output = run(&bad);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}
