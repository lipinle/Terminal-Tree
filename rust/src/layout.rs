//! Independent tab and pane state. Never grants filesystem write authorization.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pane {
    Left,
    Right,
}

impl Pane {
    fn index(self) -> usize {
        match self {
            Self::Left => 0,
            Self::Right => 1,
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Debug)]
struct Tab<T> {
    panes: [T; 2],
    focus: Pane,
}

impl<T> Tab<T> {
    fn new(left: T, right: T) -> Self {
        Self {
            panes: [left, right],
            focus: Pane::Left,
        }
    }
}

/// Keeps independent pane contents and focus for every tab.
#[derive(Debug)]
pub struct Layout<T> {
    tabs: Vec<Tab<T>>,
    active: usize,
    limit: usize,
}

impl<T> Layout<T> {
    pub fn new(left: T, right: T, limit: usize) -> Option<Self> {
        (limit > 0).then(|| Self {
            tabs: vec![Tab::new(left, right)],
            active: 0,
            limit,
        })
    }

    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    pub fn active(&self) -> usize {
        self.active
    }

    pub fn focused_pane(&self) -> Pane {
        self.tabs[self.active].focus
    }

    pub fn pane(&self, pane: Pane) -> &T {
        &self.tabs[self.active].panes[pane.index()]
    }

    pub fn pane_mut(&mut self, pane: Pane) -> &mut T {
        &mut self.tabs[self.active].panes[pane.index()]
    }

    pub fn focused(&self) -> &T {
        self.pane(self.focused_pane())
    }

    pub fn focused_mut(&mut self) -> &mut T {
        let pane = self.focused_pane();
        self.pane_mut(pane)
    }

    pub fn focus(&mut self, pane: Pane) {
        self.tabs[self.active].focus = pane;
    }

    pub fn toggle_focus(&mut self) {
        self.focus(self.focused_pane().other());
    }

    pub fn add(&mut self, left: T, right: T) -> bool {
        if self.tabs.len() >= self.limit {
            return false;
        }
        self.tabs.push(Tab::new(left, right));
        self.active = self.tabs.len() - 1;
        true
    }

    pub fn switch(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }
        self.active = index;
        true
    }

    /// Keep the same active tab when another tab closes, or select a neighbor.
    /// The last tab cannot be closed.
    pub fn close(&mut self, index: usize) -> bool {
        if self.tabs.len() == 1 || index >= self.tabs.len() {
            return false;
        }
        self.tabs.remove(index);
        if index < self.active {
            self.active -= 1;
        } else {
            self.active = self.active.min(self.tabs.len() - 1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_keep_independent_pane_contents_and_focus() {
        let mut layout = Layout::new(1, 2, 3).unwrap();
        layout.focus(Pane::Right);
        *layout.focused_mut() = 20;
        assert!(layout.add(3, 4));
        assert_eq!(layout.focused_pane(), Pane::Left);
        *layout.pane_mut(Pane::Right) = 40;
        layout.toggle_focus();
        assert_eq!(*layout.focused(), 40);
        assert!(layout.switch(0));
        assert_eq!(layout.focused_pane(), Pane::Right);
        assert_eq!(*layout.focused(), 20);
        assert_eq!(*layout.pane(Pane::Left), 1);
        assert!(layout.switch(1));
        assert_eq!(*layout.focused(), 40);
    }

    #[test]
    fn closing_tabs_keeps_active_identity_and_valid_neighbor() {
        let mut layout = Layout::new(1, 2, 4).unwrap();
        assert!(layout.add(3, 4));
        assert!(layout.add(5, 6));
        layout.focus(Pane::Right);
        assert!(layout.close(0));
        assert_eq!(layout.active(), 1);
        assert_eq!(*layout.focused(), 6);
        assert!(layout.close(1));
        assert_eq!(layout.active(), 0);
        assert_eq!(*layout.focused(), 3);
        assert!(!layout.close(0));
        assert_eq!(layout.tab_count(), 1);
    }

    #[test]
    fn limits_and_invalid_indices_preserve_state() {
        assert!(Layout::new(1, 2, 0).is_none());
        let mut layout = Layout::new(1, 2, 1).unwrap();
        assert!(!layout.add(3, 4));
        assert!(!layout.switch(usize::MAX));
        assert!(!layout.close(usize::MAX));
        assert_eq!(layout.active(), 0);
        assert_eq!(layout.tab_count(), 1);
        assert_eq!(*layout.focused(), 1);
    }
}
