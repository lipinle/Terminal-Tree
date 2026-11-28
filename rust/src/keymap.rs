//! Configurable read-only navigation keys. Mutation and confirmation keys are
//! deliberately outside this mapping.

use crossterm::event::{KeyCode, KeyModifiers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    PageUp,
    PageDown,
    First,
    Last,
    Expand,
    Collapse,
    Refresh,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingError {
    ReservedOrUnsupported,
    Conflict,
}

const ACTIONS: [Action; 9] = [
    Action::Up,
    Action::Down,
    Action::PageUp,
    Action::PageDown,
    Action::First,
    Action::Last,
    Action::Expand,
    Action::Collapse,
    Action::Refresh,
];

fn default_binding(action: Action) -> Binding {
    let code = match action {
        Action::Up => KeyCode::Up,
        Action::Down => KeyCode::Down,
        Action::PageUp => KeyCode::PageUp,
        Action::PageDown => KeyCode::PageDown,
        Action::First => KeyCode::Home,
        Action::Last => KeyCode::End,
        Action::Expand => KeyCode::Right,
        Action::Collapse => KeyCode::Left,
        Action::Refresh => KeyCode::F(5),
    };
    Binding {
        code,
        modifiers: KeyModifiers::NONE,
    }
}

fn permitted(binding: Binding) -> bool {
    // Restrict this first version to keys with no text/confirmation semantics.
    // Escape, Ctrl+C, Enter, Delete and existing workspace shortcuts cannot
    // be remapped by this navigation-only API.
    binding.modifiers == KeyModifiers::NONE
        && matches!(
            binding.code,
            KeyCode::Up
                | KeyCode::Down
                | KeyCode::Left
                | KeyCode::Right
                | KeyCode::PageUp
                | KeyCode::PageDown
                | KeyCode::Home
                | KeyCode::End
                | KeyCode::F(5 | 9 | 10 | 11 | 12)
        )
}

#[derive(Clone, Debug)]
pub struct Keymap {
    bindings: [Binding; ACTIONS.len()],
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            bindings: ACTIONS.map(default_binding),
        }
    }
}

impl Keymap {
    pub fn binding(&self, action: Action) -> Binding {
        self.bindings[Self::index(action)]
    }

    pub fn action(&self, binding: Binding) -> Option<Action> {
        self.bindings
            .iter()
            .position(|candidate| *candidate == binding)
            .map(|index| ACTIONS[index])
    }

    /// Validate the whole replacement before changing any mapping. This
    /// permits swaps without ever leaving an action without a binding.
    pub fn replace_all(&mut self, bindings: [Binding; ACTIONS.len()]) -> Result<(), BindingError> {
        for (index, binding) in bindings.iter().enumerate() {
            if !permitted(*binding) {
                return Err(BindingError::ReservedOrUnsupported);
            }
            if bindings[..index].contains(binding) {
                return Err(BindingError::Conflict);
            }
        }
        self.bindings = bindings;
        Ok(())
    }

    pub fn rebind(&mut self, action: Action, binding: Binding) -> Result<(), BindingError> {
        let mut next = self.bindings;
        next[Self::index(action)] = binding;
        self.replace_all(next)
    }

    fn index(action: Action) -> usize {
        match action {
            Action::Up => 0,
            Action::Down => 1,
            Action::PageUp => 2,
            Action::PageDown => 3,
            Action::First => 4,
            Action::Last => 5,
            Action::Expand => 6,
            Action::Collapse => 7,
            Action::Refresh => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> Binding {
        Binding {
            code,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn defaults_are_unique_and_resolvable() {
        let mut map = Keymap::default();
        assert_eq!(map.replace_all(map.bindings), Ok(()));
        for action in ACTIONS {
            assert_eq!(map.action(map.binding(action)), Some(action));
        }
    }

    #[test]
    fn conflicts_and_reserved_keys_preserve_mapping() {
        let mut map = Keymap::default();
        assert_eq!(
            map.rebind(Action::Up, key(KeyCode::Down)),
            Err(BindingError::Conflict)
        );
        for code in [
            KeyCode::Esc,
            KeyCode::Enter,
            KeyCode::Delete,
            KeyCode::Char('q'),
        ] {
            assert_eq!(
                map.rebind(Action::Up, key(code)),
                Err(BindingError::ReservedOrUnsupported)
            );
        }
        assert_eq!(
            map.rebind(
                Action::Up,
                Binding {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                },
            ),
            Err(BindingError::ReservedOrUnsupported)
        );
        assert_eq!(map.binding(Action::Up), key(KeyCode::Up));
    }

    #[test]
    fn swapping_and_rebinding_are_atomic() {
        let mut map = Keymap::default();
        let mut next = map.bindings;
        next.swap(0, 1);
        map.replace_all(next).unwrap();
        assert_eq!(map.action(key(KeyCode::Down)), Some(Action::Up));
        map.rebind(Action::Refresh, key(KeyCode::F(9))).unwrap();
        assert_eq!(map.action(key(KeyCode::F(5))), None);
        assert_eq!(map.action(key(KeyCode::F(9))), Some(Action::Refresh));
    }
}
