//! Command registry and searchable command palette.
use super::{menus::MENUS, State};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;

impl State {
    pub(super) fn commands(
        &self,
        category: usize,
        writable: bool,
    ) -> Vec<(&'static str, KeyCode, KeyModifiers)> {
        let plain = KeyModifiers::NONE;
        let mut commands = match category {
            0 => vec![
                (
                    "New tab (Ctrl+T)",
                    KeyCode::Char('t'),
                    KeyModifiers::CONTROL,
                ),
                (
                    "Close tab (Ctrl+W)",
                    KeyCode::Char('w'),
                    KeyModifiers::CONTROL,
                ),
                ("Next tab (Ctrl+Tab)", KeyCode::Tab, KeyModifiers::CONTROL),
                ("Open", KeyCode::Enter, plain),
                ("Properties", KeyCode::Char('i'), plain),
                ("Exit", KeyCode::Char('q'), plain),
            ],
            1 if writable => vec![
                ("Copy (Ctrl+C)", KeyCode::Char('c'), plain),
                ("Cut (Ctrl+X)", KeyCode::Char('x'), plain),
                ("Paste (Ctrl+V)", KeyCode::Char('v'), plain),
                ("View clipboard", KeyCode::F(12), plain),
                ("Rename", KeyCode::F(2), plain),
                ("New folder", KeyCode::Char('n'), plain),
                ("New file", KeyCode::Char('t'), plain),
                ("Trash", KeyCode::Delete, plain),
                ("Undo", KeyCode::Char('u'), plain),
            ],
            1 => vec![(
                "Select a scoped workspace to enable file operations",
                KeyCode::Null,
                plain,
            )],
            2 => vec![
                (
                    "Cycle preview mode (Alt+P)",
                    KeyCode::Char('p'),
                    KeyModifiers::ALT,
                ),
                (
                    "Tree view (Ctrl+3)",
                    KeyCode::Char('3'),
                    KeyModifiers::CONTROL,
                ),
                (
                    "Grid view (Ctrl+1)",
                    KeyCode::Char('1'),
                    KeyModifiers::CONTROL,
                ),
                (
                    "List view (Ctrl+2)",
                    KeyCode::Char('2'),
                    KeyModifiers::CONTROL,
                ),
                ("Toggle dual pane (F8)", KeyCode::F(8), plain),
                ("Toggle live preview", KeyCode::F(9), plain),
                ("Open preview", KeyCode::F(3), plain),
                ("Hidden files", KeyCode::Char('h'), plain),
                ("Sort", KeyCode::Char('s'), plain),
                ("Refresh", KeyCode::F(5), plain),
                ("Filter", KeyCode::Char('/'), plain),
            ],
            3 => vec![
                ("Parent", KeyCode::Left, plain),
                ("Back", KeyCode::Char('['), plain),
                ("Forward", KeyCode::Char(']'), plain),
                ("Locations", KeyCode::Char('m'), plain),
                ("Bookmark current directory", KeyCode::Char('b'), plain),
                ("Bookmarks", KeyCode::Char('B'), plain),
            ],
            4 => vec![
                ("Companion tools", KeyCode::Char('p'), KeyModifiers::CONTROL),
                ("Git Manager (Alt+G)", KeyCode::Char('g'), KeyModifiers::ALT),
                ("Help", KeyCode::F(1), plain),
            ],
            5 => vec![("Settings Center", KeyCode::Char(','), KeyModifiers::CONTROL)],
            _ => vec![("Manage integrations", KeyCode::F(11), KeyModifiers::SHIFT)],
        };
        if category == 2 && cfg!(feature = "native-ops") {
            commands.push((
                "Toggle terminal panel (Alt+H)",
                KeyCode::Char('h'),
                KeyModifiers::ALT,
            ));
            commands.push((
                "Focus terminal (Alt+T)",
                KeyCode::Char('t'),
                KeyModifiers::ALT,
            ));
            commands.push((
                "Expand visual preview (Alt+V)",
                KeyCode::Char('v'),
                KeyModifiers::ALT,
            ));
            commands.push((
                "Start or close interactive shell (Alt+S)",
                KeyCode::Char('s'),
                KeyModifiers::ALT,
            ));
            commands.push(("Hex preview (Shift+F8)", KeyCode::F(8), KeyModifiers::SHIFT));
        }
        if category == 3 {
            commands.insert(
                0,
                (
                    "Go to path (Ctrl+L)",
                    KeyCode::Char('l'),
                    KeyModifiers::CONTROL,
                ),
            );
        }
        if category == 4 && writable {
            commands.push(("File actions", KeyCode::F(6), plain));
            commands.push(("Recovery", KeyCode::Char('j'), plain));
        }

        if !cfg!(feature = "native-ops") {
            commands.retain(|(_, key, mods)| {
                *key != KeyCode::F(3)
                    && !(*mods == KeyModifiers::CONTROL && *key == KeyCode::Char('p'))
                    && !(*mods == KeyModifiers::ALT && *key == KeyCode::Char('g'))
            });
        }
        commands.retain(|(_, key, _)| *key != KeyCode::Null);
        // Keep a readable explanation in the Edit menu when writes are disabled.
        if commands.is_empty() {
            commands.push((
                "Select a scoped workspace to enable file operations",
                KeyCode::Null,
                plain,
            ));
        }
        commands
    }

    pub(super) fn palette(&self, writable: bool) -> io::Result<Option<Event>> {
        let mut actions = Vec::new();
        let mut labels = Vec::new();
        for (category, (name, _)) in MENUS.iter().enumerate() {
            for (label, key, mods) in self.commands(category, writable) {
                if key == KeyCode::Null {
                    continue;
                }
                labels.push(format!("{name} / {label}"));
                actions.push(Event::Key(KeyEvent::new(key, mods)));
            }
        }
        Ok(crate::command_palette::choose(&labels)?.map(|i| actions.remove(i)))
    }
}
