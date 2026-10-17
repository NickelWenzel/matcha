//! Key bindings that take precedence over the editor's built-in chords.

use iced::keyboard::{Key, Modifiers};

use crate::Action;

/// A key pressed with an exact set of modifiers.
///
/// `key` is the logical key. A character key matches either case, so
/// `Chord::new(Key::Character("l".into()), Modifiers::CTRL | Modifiers::SHIFT)`
/// matches the `L` that Shift produces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chord {
    key: Key,
    modifiers: Modifiers,
}

impl Chord {
    /// `key` pressed with exactly `modifiers` held.
    #[must_use]
    pub fn new(key: Key, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }

    fn matches(&self, key: &Key, modifiers: Modifiers) -> bool {
        if self.modifiers != modifiers {
            return false;
        }
        match (&self.key, key) {
            (Key::Character(a), Key::Character(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        }
    }
}

/// Overrides for the editor's key bindings: rebind a chord to another
/// [`Action`], or unbind it so the editor ignores it and the press reaches the
/// app's own `keyboard::listen`.
///
/// Popup, signature-box and snippet keys keep priority while those are open,
/// and the find-bar chords (Ctrl+F, Ctrl+H, Escape) are not editor keys.
///
/// ```
/// use iced::keyboard::{key::Named, Key, Modifiers};
/// use matcha_iced::{keymap::{Chord, Keymap}, Action};
///
/// let keymap = Keymap::new()
///     .bind(
///         Chord::new(Key::Named(Named::ArrowUp), Modifiers::CTRL | Modifiers::ALT),
///         Action::AddCaretVertical { down: false },
///     )
///     .unbind(Chord::new(Key::Character("u".into()), Modifiers::CTRL));
/// ```
#[derive(Clone, Debug, Default)]
pub struct Keymap {
    bindings: Vec<(Chord, Option<Action>)>,
}

impl Keymap {
    /// No overrides: every chord keeps its built-in meaning.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Make `chord` publish `action`. A later binding of the same chord wins.
    #[must_use]
    pub fn bind(mut self, chord: Chord, action: Action) -> Self {
        self.bindings.push((chord, Some(action)));
        self
    }

    /// Make the editor ignore `chord`. A later binding of the same chord wins.
    #[must_use]
    pub fn unbind(mut self, chord: Chord) -> Self {
        self.bindings.push((chord, None));
        self
    }

    /// `None` when no override covers the press; `Some(None)` when it is unbound.
    pub(crate) fn lookup(&self, key: &Key, modifiers: Modifiers) -> Option<Option<&Action>> {
        self.bindings
            .iter()
            .rev()
            .find(|(chord, _)| chord.matches(key, modifiers))
            .map(|(_, action)| action.as_ref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::key::Named;

    #[test]
    fn the_latest_binding_of_a_chord_wins() {
        let up = || Chord::new(Key::Named(Named::ArrowUp), Modifiers::ALT);
        let keymap = Keymap::new().bind(up(), Action::Undo).unbind(up());
        assert_eq!(keymap.lookup(&Key::Named(Named::ArrowUp), Modifiers::ALT), Some(None));
        let keymap = keymap.bind(up(), Action::Redo);
        assert_eq!(keymap.lookup(&Key::Named(Named::ArrowUp), Modifiers::ALT), Some(Some(&Action::Redo)));
    }

    #[test]
    fn modifiers_match_exactly_and_characters_ignore_case() {
        let keymap = Keymap::new().bind(
            Chord::new(Key::Character("l".into()), Modifiers::CTRL | Modifiers::SHIFT),
            Action::SelectAll,
        );
        let l = Key::Character("L".into());
        assert_eq!(keymap.lookup(&l, Modifiers::CTRL | Modifiers::SHIFT), Some(Some(&Action::SelectAll)));
        assert_eq!(keymap.lookup(&l, Modifiers::CTRL), None);
        assert_eq!(keymap.lookup(&Key::Named(Named::ArrowUp), Modifiers::CTRL | Modifiers::SHIFT), None);
    }
}
