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
    InvalidSpecification,
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

impl Action {
    fn name(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::PageUp => "pageup",
            Self::PageDown => "pagedown",
            Self::First => "first",
            Self::Last => "last",
            Self::Expand => "expand",
            Self::Collapse => "collapse",
            Self::Refresh => "refresh",
        }
    }
}

fn named_key(name: &str) -> Option<KeyCode> {
    Some(match name {
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "f5" => KeyCode::F(5),
        "f9" => KeyCode::F(9),
        "f10" => KeyCode::F(10),
        "f11" => KeyCode::F(11),
        "f12" => KeyCode::F(12),
        _ => return None,
    })
}

fn key_name(code: KeyCode) -> Option<&'static str> {
    Some(match code {
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::F(5) => "f5",
        KeyCode::F(9) => "f9",
        KeyCode::F(10) => "f10",
        KeyCode::F(11) => "f11",
        KeyCode::F(12) => "f12",
        _ => return None,
    })
}

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
    /// Apply comma-separated action=key assignments to the default map.
    /// Validate the complete map atomically, including simultaneous swaps.
    pub fn from_spec(spec: &str) -> Result<Self, BindingError> {
        if spec.is_empty() || spec.len() > 256 {
            return Err(BindingError::InvalidSpecification);
        }
        let mut map = Self::default();
        let mut bindings = map.bindings;
        let mut seen = [false; ACTIONS.len()];
        for assignment in spec.split(',') {
            let (action, key) = assignment
                .split_once('=')
                .ok_or(BindingError::InvalidSpecification)?;
            let action = ACTIONS
                .iter()
                .copied()
                .find(|candidate| candidate.name() == action)
                .ok_or(BindingError::InvalidSpecification)?;
            let index = Self::index(action);
            if seen[index] {
                return Err(BindingError::InvalidSpecification);
            }
            seen[index] = true;
            bindings[index] = Binding {
                code: named_key(key).ok_or(BindingError::ReservedOrUnsupported)?,
                modifiers: KeyModifiers::NONE,
            };
        }
        map.replace_all(bindings)?;
        Ok(map)
    }

    /// Export the complete map in a deterministic form accepted by from_spec.
    /// This is a string only; no configuration file is created or changed.
    pub fn to_spec(&self) -> Result<String, BindingError> {
        ACTIONS
            .iter()
            .map(|&action| {
                let binding = self.binding(action);
                if !permitted(binding) {
                    return Err(BindingError::ReservedOrUnsupported);
                }
                let key = key_name(binding.code).ok_or(BindingError::ReservedOrUnsupported)?;
                Ok(format!("{}={key}", action.name()))
            })
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(","))
    }

    pub fn summary(&self) -> String {
        ACTIONS
            .iter()
            .map(|&action| format!("{}={:?}", action.name(), self.binding(action).code))
            .collect::<Vec<_>>()
            .join(" ")
    }

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
    fn exported_spec_round_trips_complete_mapping() {
        for map in [
            Keymap::default(),
            Keymap::from_spec("up=down,down=up,refresh=f12").unwrap(),
        ] {
            let spec = map.to_spec().unwrap();
            assert!(spec.len() <= 256);
            let decoded = Keymap::from_spec(&spec).unwrap();
            assert_eq!(map.bindings, decoded.bindings);
            assert_eq!(decoded.to_spec().unwrap(), spec);
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
    fn specifications_support_swaps_and_reject_conflicts() {
        let map = Keymap::from_spec("up=down,down=up,refresh=f9").unwrap();
        assert_eq!(map.action(key(KeyCode::Down)), Some(Action::Up));
        assert_eq!(map.action(key(KeyCode::Up)), Some(Action::Down));
        assert_eq!(map.action(key(KeyCode::F(5))), None);
        assert_eq!(map.action(key(KeyCode::F(9))), Some(Action::Refresh));
        assert!(map.summary().contains("refresh=F(9)"));
        assert_eq!(
            Keymap::from_spec("up=down").unwrap_err(),
            BindingError::Conflict
        );
        for invalid in ["", "up", "up=up,up=up", "unknown=up", "up=up,"] {
            assert_eq!(
                Keymap::from_spec(invalid).unwrap_err(),
                BindingError::InvalidSpecification
            );
        }
        for reserved in ["up=esc", "up=enter", "up=ctrl+c", "up=q"] {
            assert_eq!(
                Keymap::from_spec(reserved).unwrap_err(),
                BindingError::ReservedOrUnsupported
            );
        }
        assert!(Keymap::from_spec(&"x".repeat(257)).is_err());
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
