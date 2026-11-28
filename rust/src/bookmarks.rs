//! In-memory saved locations. Paths are data, never write authorization.
#[cfg(all(feature = "native-ops", target_os = "linux"))]
pub mod snapshot;

use std::path::{Component, Path, PathBuf};

pub const MAX_BOOKMARKS: usize = 128;
const MAX_PATH_BYTES: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookmarkError {
    InvalidPath,
    Duplicate,
    Limit,
    Index,
}

/// Preserves native path identity and insertion order without filesystem writes.
#[derive(Debug, Default)]
pub struct Bookmarks {
    paths: Vec<PathBuf>,
}

fn valid(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    path.is_absolute()
        && bytes.len() <= MAX_PATH_BYTES
        && !bytes.contains(&0)
        && !path.components().any(|part| part == Component::ParentDir)
}

impl Bookmarks {
    pub fn paths(&self) -> &[PathBuf] {
        &self.paths
    }

    pub fn add(&mut self, path: PathBuf) -> Result<(), BookmarkError> {
        if !valid(&path) {
            return Err(BookmarkError::InvalidPath);
        }
        if self.paths.contains(&path) {
            return Err(BookmarkError::Duplicate);
        }
        if self.paths.len() >= MAX_BOOKMARKS {
            return Err(BookmarkError::Limit);
        }
        self.paths.push(path);
        Ok(())
    }

    pub fn get(&self, index: usize) -> Option<&Path> {
        self.paths.get(index).map(PathBuf::as_path)
    }

    pub fn remove(&mut self, index: usize) -> Result<PathBuf, BookmarkError> {
        if index >= self.paths.len() {
            return Err(BookmarkError::Index);
        }
        Ok(self.paths.remove(index))
    }

    /// Reorder saved locations without changing the underlying path values.
    pub fn move_to(&mut self, from: usize, to: usize) -> Result<(), BookmarkError> {
        if from >= self.paths.len() || to >= self.paths.len() {
            return Err(BookmarkError::Index);
        }
        if from != to {
            let path = self.paths.remove(from);
            self.paths.insert(to, path);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn root() -> PathBuf {
        #[cfg(windows)]
        {
            PathBuf::from(r"C:\")
        }
        #[cfg(not(windows))]
        {
            PathBuf::from("/")
        }
    }

    #[test]
    fn add_remove_and_reorder_preserve_paths() {
        let a = root().join("a");
        let b = root().join("b");
        let mut marks = Bookmarks::default();
        marks.add(a.clone()).unwrap();
        marks.add(b.clone()).unwrap();
        assert_eq!(marks.add(a.clone()), Err(BookmarkError::Duplicate));
        marks.move_to(0, 1).unwrap();
        assert_eq!(marks.get(0), Some(b.as_path()));
        assert_eq!(marks.remove(1), Ok(a));
        assert_eq!(marks.paths(), &[b]);
    }

    #[test]
    fn invalid_paths_and_indices_do_not_mutate_state() {
        let mut marks = Bookmarks::default();
        assert_eq!(
            marks.add(PathBuf::from("relative")),
            Err(BookmarkError::InvalidPath)
        );
        assert_eq!(
            marks.add(root().join("a").join("..")),
            Err(BookmarkError::InvalidPath)
        );
        assert_eq!(
            marks.add(root().join("nul\0name")),
            Err(BookmarkError::InvalidPath)
        );
        assert_eq!(marks.remove(0), Err(BookmarkError::Index));
        assert_eq!(marks.move_to(0, 0), Err(BookmarkError::Index));
        assert!(marks.paths().is_empty());
    }

    #[test]
    fn count_limit_is_enforced() {
        let mut marks = Bookmarks::default();
        for index in 0..MAX_BOOKMARKS {
            marks.add(root().join(index.to_string())).unwrap();
        }
        assert_eq!(
            marks.add(root().join("overflow")),
            Err(BookmarkError::Limit)
        );
        assert_eq!(marks.paths().len(), MAX_BOOKMARKS);
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_paths_are_preserved() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;
        let raw = root().join(OsString::from_vec(b"raw-\xff".to_vec()));
        let mut marks = Bookmarks::default();
        marks.add(raw.clone()).unwrap();
        assert_eq!(marks.remove(0), Ok(raw));
    }
}
