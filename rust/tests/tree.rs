use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use terminal_tree::{tree::TreeModel, Order};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "tt-tree-test-{}-{}",
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

#[test]
fn expand_collapse_and_depth_controls() {
    let f = Fixture::new();
    fs::create_dir_all(f.0.join("a/b")).unwrap();
    fs::write(f.0.join("a/b/file"), b"x").unwrap();
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    tree.expand().unwrap();
    assert_eq!(tree.rows().len(), 2);
    tree.shift(1);
    tree.expand().unwrap();
    assert_eq!(tree.rows()[2].depth, 3);
    tree.shift(1);
    tree.set_depth_limit(1).unwrap();
    assert_eq!(tree.rows().len(), 1);
    assert_eq!(tree.selected(), 0);
    assert!(!tree.rows()[0].expanded);
    assert!(tree.expand().is_err());
    tree.set_depth_limit(3).unwrap();
    tree.expand().unwrap();
    tree.collapse_or_parent();
    assert_eq!(tree.rows().len(), 1);
}

#[test]
fn failed_expansion_keeps_state_and_cursor_never_rescans() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("a")).unwrap();
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    fs::remove_dir(f.0.join("a")).unwrap();
    fs::write(f.0.join("new"), b"x").unwrap();
    assert!(tree.expand().is_err());
    tree.shift(100);
    assert_eq!(tree.rows().len(), 1);
    assert_eq!(tree.selected(), 0);
    assert!(!tree.rows()[0].expanded);
    assert_eq!(tree.viewport(5), (0, 1));
    tree.refresh().unwrap();
    assert_eq!(tree.rows()[0].entry.name, "new");
}

#[cfg(unix)]
#[test]
fn directory_link_cycle_is_not_expanded() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    symlink(&f.0, f.0.join("loop")).unwrap();
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    assert!(tree.expand().is_err());
    assert_eq!(tree.rows().len(), 1);
    assert!(!tree.rows()[0].expanded);
}

#[test]
fn expanded_directory_limit_preserves_existing_rows() {
    let f = Fixture::new();
    for i in 0..65 {
        fs::create_dir(f.0.join(format!("dir-{i:02}"))).unwrap();
    }
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    for i in 0..64 {
        tree.select(i);
        tree.expand().unwrap();
    }
    tree.select(64);
    assert!(tree.expand().is_err());
    assert_eq!(tree.rows().len(), 65);
    assert_eq!(tree.selected(), 64);
    assert!(!tree.rows()[64].expanded);
    tree.select(0);
    tree.collapse_or_parent();
    tree.select(64);
    tree.expand().unwrap();
    assert!(tree.rows()[64].expanded);
}

#[test]
fn invalid_depth_does_not_change_state() {
    let f = Fixture::new();
    fs::create_dir(f.0.join("a")).unwrap();
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    let depth = tree.depth_limit();
    assert!(tree.set_depth_limit(0).is_err());
    assert!(tree.set_depth_limit(17).is_err());
    assert_eq!(tree.depth_limit(), depth);
    assert_eq!(tree.rows().len(), 1);
}

#[cfg(unix)]
#[test]
fn replaced_directory_link_is_refused() {
    use std::os::unix::fs::symlink;
    let f = Fixture::new();
    let outside = Fixture::new();
    fs::create_dir(f.0.join("a")).unwrap();
    fs::write(outside.0.join("outside"), b"x").unwrap();
    let mut tree = TreeModel::new(&f.0, false, Order::Name).unwrap();
    fs::remove_dir(f.0.join("a")).unwrap();
    symlink(&outside.0, f.0.join("a")).unwrap();
    assert!(tree.expand().is_err());
    assert_eq!(tree.rows().len(), 1);
    assert!(!tree.rows()[0].expanded);
}

#[cfg(unix)]
#[test]
fn replaced_root_directory_does_not_change_existing_tree() {
    let f = Fixture::new();
    let root = f.0.join("root");
    fs::create_dir_all(root.join("a")).unwrap();
    let mut tree = TreeModel::new(&root, false, Order::Name).unwrap();
    fs::rename(&root, f.0.join("original")).unwrap();
    fs::create_dir_all(root.join("a")).unwrap();
    fs::write(root.join("a/new"), b"x").unwrap();
    assert!(tree.expand().is_err());
    assert!(tree.refresh().is_err());
    assert_eq!(tree.rows().len(), 1);
    assert_eq!(tree.rows()[0].entry.name, "a");
    assert!(!tree.rows()[0].expanded);
    assert_eq!(tree.selected(), 0);
}
