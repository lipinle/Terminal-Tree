#![cfg(all(feature = "native-ops", target_os = "linux"))]

use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use terminal_tree::operations::Engine;
use terminal_tree::rename_plan::native::reviewed::Batch;

#[test]
fn reviewed_bulk_rename_preflights_all_and_stops_on_late_conflict() {
    let root = std::env::temp_dir().join(format!("tt-bulk-rename-{}", std::process::id()));
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
    let engine = Engine::new(&workspace, &journal).unwrap();
    let a = workspace.join("old-a");
    let b = workspace.join("old-b");
    let na = workspace.join("new-a");
    let nb = workspace.join("new-b");
    fs::write(&a, b"a").unwrap();
    fs::write(&b, b"b").unwrap();
    let sources = [engine.review(&a).unwrap(), engine.review(&b).unwrap()];
    let prepare = || {
        Batch::prepare(
            &engine,
            &workspace,
            &sources,
            OsStr::new("old"),
            OsStr::new("new"),
        )
    };
    fs::write(&nb, b"occupied").unwrap();
    assert!(prepare().is_err());
    assert!(!na.exists());
    assert_eq!(fs::read_dir(&journal).unwrap().count(), 0);
    fs::remove_file(&nb).unwrap();
    let batch = prepare().unwrap();
    assert_eq!(
        batch.paths().collect::<Vec<_>>(),
        [(a.as_path(), na.as_path()), (b.as_path(), nb.as_path())]
    );
    let outcome = batch
        .execute(&engine, &mut |index, _, point| {
            if index == 1 && point == "rename-item" {
                fs::write(&nb, b"late")?;
            }
            Ok(())
        })
        .unwrap();
    assert_eq!(outcome.completed, 1);
    assert_eq!(outcome.undos.len(), 1);
    assert!(outcome.failure.is_some());
    assert!(!a.exists());
    assert_eq!(fs::read(&na).unwrap(), b"a");
    assert_eq!(fs::read(&b).unwrap(), b"b");
    assert_eq!(fs::read(&nb).unwrap(), b"late");
    fs::remove_file(&nb).unwrap();
    let remaining = Batch::prepare(
        &engine,
        &workspace,
        &[engine.review(&b).unwrap()],
        OsStr::new("old"),
        OsStr::new("new"),
    )
    .unwrap();
    let done = remaining.execute(&engine, &mut |_, _, _| Ok(())).unwrap();
    assert_eq!(done.completed, 1);
    assert!(done.failure.is_none());
    assert_eq!(fs::read(&nb).unwrap(), b"b");
    assert!(!b.exists());
}
