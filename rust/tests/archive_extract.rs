#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use terminal_tree::archive::Kind;
use terminal_tree::archive_build::{build, Entry};
use terminal_tree::operations::Engine;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("tt-extract-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("workspace")).unwrap();
        fs::create_dir(root.join("journal")).unwrap();
        fs::set_permissions(root.join("journal"), fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn staged_extraction_never_merges_or_overwrites_existing_trees() {
    let f = Fixture::new();
    let workspace = f.0.join("workspace");
    let journal = f.0.join("journal");
    let engine = Engine::new(&workspace, &journal).unwrap();
    let archive = build(&[
        Entry {
            name: "folder/file",
            kind: Kind::File,
            data: b"hello\0",
        },
        Entry {
            name: "empty/",
            kind: Kind::Directory,
            data: b"",
        },
    ])
    .unwrap();
    let source = workspace.join("archive.tar");
    fs::write(&source, &archive).unwrap();
    assert_eq!(
        engine.read_archive_for_extraction(&source).unwrap(),
        archive
    );
    let path = workspace.join("result");
    let destination = engine.destination(&path).unwrap();
    let mut damaged = archive.clone();
    *damaged.last_mut().unwrap() = 1;
    assert!(engine
        .extract_archive_reviewed(&destination, &damaged)
        .is_err());
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
    engine
        .extract_archive_reviewed(&destination, &archive)
        .unwrap();
    assert_eq!(fs::read(path.join("folder/file")).unwrap(), b"hello\0");
    assert!(path.join("empty").is_dir());
    assert!(engine
        .extract_archive_reviewed(&destination, &archive)
        .is_err());
    assert_eq!(fs::read(path.join("folder/file")).unwrap(), b"hello\0");
    assert_eq!(fs::read_dir(&workspace).unwrap().count(), 2);
    assert_eq!(fs::read(&source).unwrap(), archive);
    let done = fs::read_dir(&journal).unwrap().next().unwrap().unwrap();
    assert!(done.file_name().to_string_lossy().ends_with(".done.json"));

    let interrupted = workspace.join("interrupted");
    let destination = engine.destination(&interrupted).unwrap();
    let result = engine.extract_archive_reviewed_with(&destination, &archive, &mut |point| {
        if point == "extract-before-publish" {
            Err(io::Error::other("injected interruption"))
        } else {
            Ok(())
        }
    });
    assert!(result.is_err());
    assert!(!interrupted.exists());
    let (pending, truncated) = engine.recovery_report().unwrap();
    assert!(!truncated);
    assert_eq!(pending.len(), 1);
    assert!(pending[0].1.is_ok());
    assert_eq!(fs::read(path.join("folder/file")).unwrap(), b"hello\0");

    let conflict = workspace.join("conflict");
    let destination = engine.destination(&conflict).unwrap();
    let result = engine.extract_archive_reviewed_with(&destination, &archive, &mut |point| {
        if point == "extract-before-publish" {
            fs::write(&conflict, b"other")?;
        }
        Ok(())
    });
    assert!(result.is_err());
    assert_eq!(fs::read(&conflict).unwrap(), b"other");

    for (index, point) in ["extract-before-sync", "extract-after-sync"]
        .into_iter()
        .enumerate()
    {
        let path = workspace.join(format!("published-{index}"));
        let destination = engine.destination(&path).unwrap();
        let result = engine.extract_archive_reviewed_with(&destination, &archive, &mut |seen| {
            if seen == point {
                Err(io::Error::other("injected interruption"))
            } else {
                Ok(())
            }
        });
        assert!(result.is_err());
        assert_eq!(fs::read(path.join("folder/file")).unwrap(), b"hello\0");
        assert!(path.join("empty").is_dir());
        assert_eq!(fs::read(&source).unwrap(), archive);
        assert_eq!(engine.pending().unwrap().len(), index + 3);
    }
}
