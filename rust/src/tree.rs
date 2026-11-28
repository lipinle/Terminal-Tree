//! Bounded, read-only tree navigation. Never use rows as write authorization.
use crate::{scan_bounded, Entry, Order};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const MAX_SCAN: usize = 4096;
const MAX_ROWS: usize = 16384;
const MAX_EXPANDED: usize = 64;
pub const MAX_DEPTH: usize = 16;

#[derive(Debug)]
pub struct TreeRow {
    pub entry: Entry,
    pub depth: usize,
    pub expanded: bool,
}

#[derive(Debug)]
pub struct TreeModel {
    root: PathBuf,
    rows: Vec<TreeRow>,
    selected: usize,
    top: usize,
    depth_limit: usize,
    hidden: bool,
    order: Order,
}

fn plain_directory(path: &Path) -> io::Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(io::Error::other("tree expansion requires a plain directory"));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const REPARSE_POINT: u32 = 0x400;
        if metadata.file_attributes() & REPARSE_POINT != 0 {
            return Err(io::Error::other("tree expansion refuses reparse points"));
        }
    }
    Ok(metadata)
}

impl TreeModel {
    pub fn new(root: &Path, hidden: bool, order: Order) -> io::Result<Self> {
        let root = root.canonicalize()?;
        plain_directory(&root)?;
        let rows = scan_bounded(&root, hidden, order, "", MAX_SCAN)?
            .into_iter()
            .map(|entry| TreeRow {
                entry,
                depth: 1,
                expanded: false,
            })
            .collect();
        Ok(Self {
            root,
            rows,
            selected: 0,
            top: 0,
            depth_limit: 4,
            hidden,
            order,
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn rows(&self) -> &[TreeRow] {
        &self.rows
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn depth_limit(&self) -> usize {
        self.depth_limit
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index.min(self.rows.len().saturating_sub(1));
    }

    pub fn shift(&mut self, delta: isize) {
        self.select(self.selected.saturating_add_signed(delta));
    }

    pub fn viewport(&mut self, height: usize) -> (usize, usize) {
        let height = height.max(1);
        self.top = self.top.min(self.rows.len().saturating_sub(height));
        if self.selected < self.top {
            self.top = self.selected;
        } else if self.selected.saturating_sub(self.top) >= height {
            self.top = self.selected + 1 - height;
        }
        (self.top, self.top.saturating_add(height).min(self.rows.len()))
    }

    // Recheck each observed descendant component, not only the final component.
    // This is best-effort for a read-only view, not race-free authorization.
    fn check_directory(&self, path: &Path) -> io::Result<()> {
        let root_metadata = plain_directory(&self.root)?;
        if self.root.canonicalize()? != self.root {
            return Err(io::Error::other("tree root changed"));
        }
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| io::Error::other("tree path outside root"))?;
        let mut current = self.root.clone();
        for part in relative.components() {
            if !matches!(part, std::path::Component::Normal(_)) {
                return Err(io::Error::other("invalid tree component"));
            }
            current.push(part);
            let metadata = plain_directory(&current)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.dev() != root_metadata.dev() {
                    return Err(io::Error::other("tree expansion refuses device changes"));
                }
            }
            #[cfg(not(unix))]
            let _ = (&root_metadata, &metadata);
        }
        Ok(())
    }

    pub fn expand(&mut self) -> io::Result<()> {
        let Some(row) = self.rows.get(self.selected) else {
            return Ok(());
        };
        if row.expanded || !row.entry.directory {
            return Ok(());
        }
        if row.entry.symlink {
            return Err(io::Error::other("tree expansion refuses symlinks"));
        }
        if row.depth >= self.depth_limit {
            return Err(io::Error::other("tree depth limit reached"));
        }
        if self.rows.iter().filter(|row| row.expanded).count() >= MAX_EXPANDED {
            return Err(io::Error::other("expanded directory limit reached"));
        }
        let path = row.entry.path.clone();
        let depth = row.depth + 1;
        let budget = MAX_SCAN.min(MAX_ROWS.saturating_sub(self.rows.len()));
        self.check_directory(&path)?;
        let children = scan_bounded(&path, self.hidden, self.order, "", budget)?;
        self.check_directory(&path)?;
        // Commit only after the entire bounded scan succeeds.
        let children = children.into_iter().map(|entry| TreeRow {
            entry,
            depth,
            expanded: false,
        });
        drop(self.rows.splice(self.selected + 1..self.selected + 1, children));
        self.rows[self.selected].expanded = true;
        Ok(())
    }

    pub fn collapse_or_parent(&mut self) {
        let Some(row) = self.rows.get(self.selected) else {
            return;
        };
        let depth = row.depth;
        if row.expanded {
            let start = self.selected + 1;
            let end = start
                + self.rows[start..]
                    .iter()
                    .take_while(|child| child.depth > depth)
                    .count();
            drop(self.rows.drain(start..end));
            self.rows[self.selected].expanded = false;
        } else if depth > 1 {
            if let Some(parent) = self.rows[..self.selected]
                .iter()
                .rposition(|candidate| candidate.depth == depth - 1)
            {
                self.selected = parent;
            }
        }
        self.top = self.top.min(self.selected);
    }

    pub fn set_depth_limit(&mut self, limit: usize) -> io::Result<()> {
        if !(1..=MAX_DEPTH).contains(&limit) {
            return Err(io::Error::other("invalid tree depth limit"));
        }
        while self
            .rows
            .get(self.selected)
            .is_some_and(|row| row.depth > limit)
        {
            // Move to an ancestor even when the selected row is expanded.
            let depth = self.rows[self.selected].depth;
            let parent = self.rows[..self.selected]
                .iter()
                .rposition(|row| row.depth == depth - 1)
                .ok_or_else(|| io::Error::other("invalid tree structure"))?;
            self.selected = parent;
        }
        let selected_path = self.rows.get(self.selected).map(|row| row.entry.path.clone());
        self.rows.retain(|row| row.depth <= limit);
        for row in &mut self.rows {
            if row.depth == limit {
                row.expanded = false;
            }
        }
        self.selected = selected_path
            .and_then(|path| self.rows.iter().position(|row| row.entry.path == path))
            .unwrap_or(0);
        self.top = self.top.min(self.selected);
        self.depth_limit = limit;
        Ok(())
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        // Discard cached descendants only after a fresh root scan succeeds.
        // Keep the original canonical root, rather than accepting a new link target.
        self.check_directory(&self.root)?;
        let entries = scan_bounded(&self.root, self.hidden, self.order, "", MAX_SCAN)?;
        self.check_directory(&self.root)?;
        self.rows = entries
            .into_iter()
            .map(|entry| TreeRow {
                entry,
                depth: 1,
                expanded: false,
            })
            .collect();
        self.selected = 0;
        self.top = 0;
        Ok(())
    }
}
