//! Menu selection and file context actions.
use super::State;
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::io;

pub(super) const MENUS: [(&str, u16); 7] = [
    ("File", 15),
    ("Edit", 21),
    ("View", 27),
    ("Go", 33),
    ("Tools", 37),
    ("Settings", 44),
    ("Add-ons", 54),
];

impl State {
    pub(super) fn menu(
        &mut self,
        category: Option<usize>,
        writable: bool,
    ) -> io::Result<Option<Event>> {
        let category = match category {
            Some(i) => i,
            None => {
                let labels = MENUS.map(|(name, _)| name.to_owned());
                match crate::choose("Menu", &labels)? {
                    Some(i) => i,
                    None => return Ok(None),
                }
            }
        };
        let commands = self.commands(category, writable);
        // Settings and Add-ons remain direct one-click destinations.
        if category == 5 || category == 6 {
            let (_, key, mods) = commands[0];
            return Ok(Some(Event::Key(KeyEvent::new(key, mods))));
        }
        let labels: Vec<_> = commands
            .iter()
            .map(|(label, _, _)| label.to_string())
            .collect();
        Ok(crate::choose(MENUS[category].0, &labels)?.map(|i| {
            let (_, key, modifiers) = commands[i];
            Event::Key(KeyEvent::new(key, modifiers))
        }))
    }

    pub(super) fn context_menu(&self, writable: bool) -> io::Result<Option<Event>> {
        let mut actions = vec![("Open", KeyCode::Enter), ("Properties", KeyCode::Char('i'))];
        if cfg!(feature = "native-ops") {
            actions.push(("Preview", KeyCode::F(3)));
        }
        if writable {
            actions.extend([
                ("Copy", KeyCode::Char('c')),
                ("Cut", KeyCode::Char('x')),
                ("Paste", KeyCode::Char('v')),
                ("Rename", KeyCode::F(2)),
                ("Select / deselect", KeyCode::Char(' ')),
                ("Move to Trash", KeyCode::Delete),
                ("More actions", KeyCode::F(6)),
            ]);
        }
        let labels: Vec<_> = actions.iter().map(|(label, _)| label.to_string()).collect();
        Ok(crate::choose("File context menu", &labels)?
            .map(|i| Event::Key(KeyEvent::new(actions[i].1, KeyModifiers::NONE))))
    }
}
