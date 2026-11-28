//! Locate only the two explicitly supported first-party optional helpers.
//! Never search PATH or the current browsing directory.
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

fn regular_executable(path: &Path, uid: u32) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| {
        m.is_file()
            && m.uid() == uid
            && m.mode() & 0o111 != 0
            && m.mode() & 0o022 == 0
            && m.mode() & 0o6000 == 0
    })
}
fn owned_directory(path: &Path, uid: u32) -> bool {
    fs::symlink_metadata(path).is_ok_and(|m| m.is_dir() && m.uid() == uid && m.mode() & 0o022 == 0)
}
fn local(home: &Path, name: &str, uid: u32) -> Option<PathBuf> {
    if uid == 0 || !home.is_absolute() || !owned_directory(home, uid) {
        return None;
    }
    let prefix = home.join(".local");
    let dir = prefix.join("bin");
    if !owned_directory(&prefix, uid) || !owned_directory(&dir, uid) {
        return None;
    }
    let path = dir.join(name);
    regular_executable(&path, uid).then_some(path)
}
pub fn find(name: &str) -> Option<PathBuf> {
    if !matches!(name, "tt-visual-preview" | "tt-x11-overlay") {
        return None;
    }
    let uid = rustix::process::geteuid().as_raw();
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        if let Some(path) = local(&home, name, uid) {
            return Some(path);
        }
    }
    let path = Path::new("/usr/local/bin").join(name);
    regular_executable(&path, 0).then_some(path)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{symlink, PermissionsExt};
    #[test]
    fn local_helper_requires_owned_nonwritable_plain_path() {
        let uid = rustix::process::geteuid().as_raw();
        if uid == 0 {
            return;
        }
        let dir = std::env::temp_dir().join(format!("tt-helper-path-{}", std::process::id()));
        fs::create_dir(&dir).unwrap();
        let cleanup = dir.clone();
        struct Cleanup(PathBuf);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let _cleanup = Cleanup(cleanup);
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let bin = dir.join(".local/bin");
        fs::create_dir_all(&bin).unwrap();
        let p = bin.join("tt-visual-preview");
        fs::write(&p, b"fixture").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(local(&dir, "tt-visual-preview", uid), Some(p.clone()));
        fs::set_permissions(&p, fs::Permissions::from_mode(0o722)).unwrap();
        assert!(local(&dir, "tt-visual-preview", uid).is_none());
        fs::remove_file(&p).unwrap();
        symlink("/bin/sh", &p).unwrap();
        assert!(local(&dir, "tt-visual-preview", uid).is_none());
        assert!(local(Path::new("."), "tt-visual-preview", uid).is_none());
        assert!(find("../sh").is_none());
    }
}
