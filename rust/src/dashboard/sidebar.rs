//! Sidebar places and bookmark navigation.
use super::State;
use std::{io, path::PathBuf};
use terminal_tree::{display_name, Model};

impl State {
    pub(super) fn manage_bookmarks(&mut self) -> io::Result<()> {
        loop {
            let labels: Vec<_> = self
                .bookmarks
                .iter()
                .map(|p| display_name(p.as_os_str()))
                .collect();
            if labels.is_empty() {
                return crate::page(
                    "Manage bookmarks",
                    &["No bookmarks. Press b in a directory to add it.".into()],
                );
            }
            let Some(index) = crate::choose("Manage bookmarks", &labels)? else {
                return Ok(());
            };
            let actions = [
                "Move up".into(),
                "Move down".into(),
                "Remove bookmark".into(),
                "Cancel".into(),
            ];
            match crate::choose("Bookmark actions", &actions)? {
                Some(0) if index > 0 => self.bookmarks.swap(index, index - 1),
                Some(1) if index + 1 < self.bookmarks.len() => {
                    self.bookmarks.swap(index, index + 1)
                }
                Some(2) => {
                    self.bookmarks.remove(index);
                }
                _ => (),
            }
            self.refresh_places();
        }
    }

    pub(super) fn refresh_places(&mut self) {
        let mut places = vec![("PLACES".into(), None)];
        if let Some(home) = crate::platform::home_dir() {
            places.push(("Home".into(), Some(home.clone())));
            for name in [
                "Desktop",
                "Documents",
                "Downloads",
                "Pictures",
                "Music",
                "Videos",
            ] {
                let p = home.join(name);
                if p.is_dir() {
                    places.push((name.into(), Some(p)));
                }
            }
            let trash = self.trash.clone().unwrap_or_else(|| {
                std::env::var_os("XDG_DATA_HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .unwrap_or_else(|| home.join(".local/share"))
                    .join("Trash/files")
            });
            if trash.is_dir() {
                places.push(("Trash".into(), Some(trash)));
            }
        }
        places.push(("DEVICES".into(), None));
        if let Ok(root) = crate::platform::filesystem_root() {
            places.push(("File System".into(), Some(root)));
        }
        #[cfg(target_os = "linux")]
        if let Ok(paths) = std::fs::read("/proc/self/mountinfo")
            .and_then(|bytes| crate::places::media_mounts(&bytes))
        {
            for p in paths.into_iter().take(128) {
                let label = p
                    .file_name()
                    .map(display_name)
                    .unwrap_or_else(|| display_name(p.as_os_str()));
                places.push((label, Some(p)));
            }
        }
        places.push(("BOOKMARKS".into(), None));
        for p in &self.bookmarks {
            places.push((
                p.file_name()
                    .map(display_name)
                    .unwrap_or_else(|| display_name(p.as_os_str())),
                Some(p.clone()),
            ));
        }
        self.places = places;
        self.side_cursor = self.side_cursor.min(self.places.len().saturating_sub(1));
        if self.places[self.side_cursor].1.is_none() {
            self.side_cursor = self.places.iter().position(|p| p.1.is_some()).unwrap_or(0);
        }
    }

    pub(super) fn move_side(&mut self, forward: bool) {
        let next = if forward {
            (self.side_cursor + 1..self.places.len()).find(|&i| self.places[i].1.is_some())
        } else {
            (0..self.side_cursor)
                .rev()
                .find(|&i| self.places[i].1.is_some())
        };
        if let Some(i) = next {
            self.side_cursor = i;
        }
    }

    pub(super) fn open_side(&mut self, model: &mut Model) -> io::Result<()> {
        if let Some(path) = self.places.get(self.side_cursor).and_then(|p| p.1.clone()) {
            model.go(path)?;
            self.side_focus = false;
            self.preview_key = None;
        }
        Ok(())
    }
}
