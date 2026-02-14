// Editor actions and key mapping.

use tide_core::{Key, Modifiers};

/// Actions the editor can perform in response to input.
#[derive(Debug, Clone, PartialEq)]
pub enum EditorAction {
    InsertChar(char),
    Backspace,
    Delete,
    Enter,
    MoveUp,
    MoveDown,
    MoveLeft,
    MoveRight,
    Home,
    End,
    PageUp,
    PageDown,
    Save,
    Undo,
    Redo,
    ScrollUp(f32),
    ScrollDown(f32),
    ScrollLeft(f32),
    ScrollRight(f32),
    /// Set cursor to a specific buffer position (from mouse click).
    SetCursor { line: usize, col: usize },
}

/// Map a Key + Modifiers to an EditorAction.
pub fn key_to_editor_action(key: &Key, modifiers: &Modifiers) -> Option<EditorAction> {
    // Ctrl+S / Cmd+S -> Save
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Char('s') | Key::Char('S')) {
        return Some(EditorAction::Save);
    }

    // Cmd+Shift+Z / Ctrl+Shift+Z -> Redo
    if (modifiers.ctrl || modifiers.meta) && modifiers.shift && matches!(key, Key::Char('z') | Key::Char('Z')) {
        return Some(EditorAction::Redo);
    }

    // Cmd+Z / Ctrl+Z -> Undo
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Char('z') | Key::Char('Z')) {
        return Some(EditorAction::Undo);
    }

    // Don't process other ctrl/meta combos as editor input
    if modifiers.ctrl || modifiers.meta {
        return None;
    }

    match key {
        Key::Char(ch) => Some(EditorAction::InsertChar(*ch)),
        Key::Backspace => Some(EditorAction::Backspace),
        Key::Delete => Some(EditorAction::Delete),
        Key::Enter => Some(EditorAction::Enter),
        Key::Up => Some(EditorAction::MoveUp),
        Key::Down => Some(EditorAction::MoveDown),
        Key::Left => Some(EditorAction::MoveLeft),
        Key::Right => Some(EditorAction::MoveRight),
        Key::Home => Some(EditorAction::Home),
        Key::End => Some(EditorAction::End),
        Key::PageUp => Some(EditorAction::PageUp),
        Key::PageDown => Some(EditorAction::PageDown),
        Key::Tab => Some(EditorAction::InsertChar('\t')),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_mod() -> Modifiers {
        Modifiers::default()
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    #[test]
    fn char_maps_to_insert() {
        assert_eq!(
            key_to_editor_action(&Key::Char('a'), &no_mod()),
            Some(EditorAction::InsertChar('a'))
        );
    }

    #[test]
    fn ctrl_s_maps_to_save() {
        assert_eq!(
            key_to_editor_action(&Key::Char('s'), &ctrl()),
            Some(EditorAction::Save)
        );
    }

    #[test]
    fn ctrl_other_returns_none() {
        assert_eq!(key_to_editor_action(&Key::Char('a'), &ctrl()), None);
    }

    #[test]
    fn arrows_map_correctly() {
        assert_eq!(
            key_to_editor_action(&Key::Up, &no_mod()),
            Some(EditorAction::MoveUp)
        );
        assert_eq!(
            key_to_editor_action(&Key::Down, &no_mod()),
            Some(EditorAction::MoveDown)
        );
    }
}
