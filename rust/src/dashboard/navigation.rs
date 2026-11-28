//! Tree navigation, location editing and dual-pane state transitions.
use super::geometry::Layout;
use super::view::{Cells, View};
use super::State;
use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use std::io;
use terminal_tree::{tree::TreeModel, KindFilter, Model};

impl State {
    pub(super) fn choose_view(&mut self, view: View) {
        self.view = view;
        self.dual = false;
        self.expanded_tree = None;
        self.side_focus = false;
    }

    // Restore a usable, lightweight view without touching files or closing shells.
    pub(super) fn reset_layout(&mut self) {
        self.choose_view(View::Tree);
        self.zoom = 1;
        self.location = None;
        self.dragging = None;
        self.prefs.sidebar = true;
        self.prefs.toolbar = true;
        self.prefs.sidebar_width = 19;
        self.prefs.preview_percent = 33;
        self.prefs.dual_percent = 50;
        self.prefs.terminal_percent = 28;
        self.prefs.terminal_visible = false;
        self.preview_enabled = false;
        self.preview_key = None;
        self.preview_scroll = 0;
        self.frame_cache.invalidate();
        #[cfg(feature = "native-ops")]
        {
            self.terminal_panel.focused = false;
            self.suspend_visual_preview();
        }
    }

    pub(super) fn start_location(&mut self, model: &Model) {
        self.expanded_tree = None;
        self.side_focus = false;
        self.location = Some(model.directory.to_str().unwrap_or("").to_owned());
    }

    fn open_expanded(&mut self, model: &mut Model) -> io::Result<()> {
        let Some(tree) = &self.expanded_tree else {
            return Ok(());
        };
        let Some(row) = tree.rows().get(tree.selected()) else {
            return Ok(());
        };
        let path = row.entry.path.clone();
        let directory = row.entry.directory && !row.entry.symlink;
        let target = if directory {
            path.clone()
        } else {
            path.parent()
                .ok_or_else(|| io::Error::other("missing parent"))?
                .to_path_buf()
        };
        // Descendant rows are read-only. Navigate into their containing folder,
        // reload ordinary rows and select the exact native path before any write.
        model.go(target)?;
        if !directory {
            if let Some(index) = model.entries.iter().position(|e| e.path == path) {
                model.selected = index;
            }
        }
        self.expanded_tree = None;
        self.preview_key = None;
        self.pane_switched = true;
        Ok(())
    }

    pub(super) fn expand_current(&mut self, model: &Model) -> io::Result<()> {
        if !model.query.is_empty() || model.kind_filter != KindFilter::All {
            return Err(io::Error::other("clear filters before expanding tree"));
        }
        let Some(entry) = model.entries.get(model.selected) else {
            return Ok(());
        };
        if !entry.directory || entry.symlink {
            return Ok(());
        }
        let mut tree = TreeModel::new(&model.directory, model.hidden, model.order)?;
        let index = tree
            .rows()
            .iter()
            .position(|r| r.entry.path.file_name() == entry.path.file_name())
            .ok_or_else(|| io::Error::other("tree selection changed; refresh first"))?;
        tree.select(index);
        tree.expand()?;
        self.expanded_tree = Some(tree);
        Ok(())
    }

    pub(super) fn expanded_input(
        &mut self,
        event: &Event,
        model: &mut Model,
        l: Layout,
    ) -> io::Result<bool> {
        let Some(tree) = self.expanded_tree.as_mut() else {
            return Ok(false);
        };
        let cells = Cells::new(l.end() - l.first(), l.capacity(), View::Tree, self.zoom);
        match event {
            Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                if k.modifiers == KeyModifiers::NONE {
                    match k.code {
                        KeyCode::Up => tree.shift(-1),
                        KeyCode::Down => tree.shift(1),
                        KeyCode::Home => tree.select(0),
                        KeyCode::End => tree.select(usize::MAX),
                        KeyCode::PageUp => tree.shift(-(cells.capacity() as isize)),
                        KeyCode::PageDown => tree.shift(cells.capacity() as isize),
                        KeyCode::Right => tree.expand()?,
                        KeyCode::Left => tree.collapse_or_parent(),
                        KeyCode::Enter => self.open_expanded(model)?,
                        KeyCode::Esc => { self.expanded_tree = None; },
                        KeyCode::F(5) => { tree.refresh()?; },
                        KeyCode::F(8) | KeyCode::F(9) | KeyCode::F(10) | KeyCode::Char('q') => {
                            self.expanded_tree = None;
                            return Ok(false);
                        }
                        _ => return Err(io::Error::other("Expanded tree: Enter opens folder or selects file in its parent; Esc returns")),
                    }
                } else if k.modifiers == KeyModifiers::CONTROL
                    && matches!(k.code, KeyCode::Char('l' | 'f' | 'q'))
                {
                    self.expanded_tree = None;
                    return Ok(false);
                }
            }
            Event::Mouse(mouse) if l.file_row(mouse.column, mouse.row).is_some() => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => tree.shift(-1),
                    MouseEventKind::ScrollDown => tree.shift(1),
                    MouseEventKind::Down(MouseButton::Left) => {
                        let (start, end) = tree.viewport(cells.capacity());
                        if let Some(offset) =
                            cells.hit(mouse.column - l.first(), usize::from(mouse.row - 4))
                        {
                            let index = start + offset;
                            if index < end {
                                tree.select(index);
                                if mouse.column < l.first() + 4 {
                                    if tree.rows()[index].expanded {
                                        tree.collapse_or_parent();
                                    } else {
                                        tree.expand()?;
                                    }
                                }
                            }
                        }
                    }
                    _ => (),
                }
            }
            Event::Mouse(mouse) if mouse.kind == MouseEventKind::Down(MouseButton::Left) => {
                self.expanded_tree = None;
                return Ok(false);
            }
            _ => (),
        }
        Ok(true)
    }

    pub(super) fn toggle_dual(&mut self, model: &Model) -> io::Result<()> {
        if !self.dual && self.other.is_none() {
            let mut other = Model::new(model.directory.clone())?;
            other.set_view(model.hidden, model.order, model.kind_filter, String::new())?;
            self.other = Some(other);
        }
        self.expanded_tree = None;
        self.dual = !self.dual;
        self.side_focus = false;
        self.preview_key = None;
        Ok(())
    }

    pub(super) fn switch_pane(&mut self, model: &mut Model) -> io::Result<()> {
        let other = self
            .other
            .as_mut()
            .ok_or_else(|| io::Error::other("second pane unavailable"))?;
        // Refresh the incoming pane before making its rows actionable.
        // A failed refresh leaves the active model and selection unchanged.
        other.reload()?;
        std::mem::swap(model, other);
        self.active_right = !self.active_right;
        self.pane_switched = true;
        self.preview_key = None;
        Ok(())
    }

    pub fn take_pane_switch(&mut self) -> bool {
        std::mem::take(&mut self.pane_switched)
    }
}
