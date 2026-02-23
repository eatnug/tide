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
    MoveWordLeft,
    MoveWordRight,
    MoveDocStart,
    MoveDocEnd,
    Home,
    End,
    PageUp,
    PageDown,
    SelectAll,
    Save,
    Undo,
    Redo,
    DeleteWordLeft,
    DeleteWordRight,
    DeleteToLineStart,
    DeleteToLineEnd,
    DeleteLine,
    MoveLineUp,
    MoveLineDown,
    Unindent,
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

    // Cmd+A / Ctrl+A -> SelectAll
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Char('a') | Key::Char('A')) {
        return Some(EditorAction::SelectAll);
    }

    // Cmd+Shift+K -> Delete line
    if (modifiers.ctrl || modifiers.meta) && modifiers.shift && matches!(key, Key::Char('k') | Key::Char('K')) {
        return Some(EditorAction::DeleteLine);
    }

    // Cmd+Left -> Home (line start)
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Left) {
        return Some(EditorAction::Home);
    }

    // Cmd+Right -> End (line end)
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Right) {
        return Some(EditorAction::End);
    }

    // Cmd+Up -> Document start
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Up) {
        return Some(EditorAction::MoveDocStart);
    }

    // Cmd+Down -> Document end
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Down) {
        return Some(EditorAction::MoveDocEnd);
    }

    // Cmd+Backspace -> Delete to line start
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Backspace) {
        return Some(EditorAction::DeleteToLineStart);
    }

    // Cmd+Delete -> Delete to line end
    if (modifiers.ctrl || modifiers.meta) && matches!(key, Key::Delete) {
        return Some(EditorAction::DeleteToLineEnd);
    }

    // Don't process other ctrl/meta combos as editor input
    if modifiers.ctrl || modifiers.meta {
        return None;
    }

    // Option+Left -> Move word left
    if modifiers.alt && matches!(key, Key::Left) {
        return Some(EditorAction::MoveWordLeft);
    }

    // Option+Right -> Move word right
    if modifiers.alt && matches!(key, Key::Right) {
        return Some(EditorAction::MoveWordRight);
    }

    // Option+Up -> Move line up
    if modifiers.alt && matches!(key, Key::Up) {
        return Some(EditorAction::MoveLineUp);
    }

    // Option+Down -> Move line down
    if modifiers.alt && matches!(key, Key::Down) {
        return Some(EditorAction::MoveLineDown);
    }

    // Option+Backspace -> Delete word left
    if modifiers.alt && matches!(key, Key::Backspace) {
        return Some(EditorAction::DeleteWordLeft);
    }

    // Option+Delete -> Delete word right
    if modifiers.alt && matches!(key, Key::Delete) {
        return Some(EditorAction::DeleteWordRight);
    }

    // Shift+Tab -> Unindent
    if modifiers.shift && matches!(key, Key::Tab) {
        return Some(EditorAction::Unindent);
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
    fn ctrl_a_maps_to_select_all() {
        assert_eq!(
            key_to_editor_action(&Key::Char('a'), &ctrl()),
            Some(EditorAction::SelectAll)
        );
    }

    #[test]
    fn ctrl_other_returns_none() {
        assert_eq!(key_to_editor_action(&Key::Char('b'), &ctrl()), None);
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

    fn alt() -> Modifiers {
        Modifiers {
            alt: true,
            ..Default::default()
        }
    }

    fn meta() -> Modifiers {
        Modifiers {
            meta: true,
            ..Default::default()
        }
    }

    fn meta_shift() -> Modifiers {
        Modifiers {
            meta: true,
            shift: true,
            ..Default::default()
        }
    }

    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Default::default()
        }
    }

    #[test]
    fn alt_arrows_map_to_word_nav() {
        assert_eq!(
            key_to_editor_action(&Key::Left, &alt()),
            Some(EditorAction::MoveWordLeft)
        );
        assert_eq!(
            key_to_editor_action(&Key::Right, &alt()),
            Some(EditorAction::MoveWordRight)
        );
    }

    #[test]
    fn alt_up_down_map_to_move_line() {
        assert_eq!(
            key_to_editor_action(&Key::Up, &alt()),
            Some(EditorAction::MoveLineUp)
        );
        assert_eq!(
            key_to_editor_action(&Key::Down, &alt()),
            Some(EditorAction::MoveLineDown)
        );
    }

    #[test]
    fn alt_backspace_delete_maps_to_word_delete() {
        assert_eq!(
            key_to_editor_action(&Key::Backspace, &alt()),
            Some(EditorAction::DeleteWordLeft)
        );
        assert_eq!(
            key_to_editor_action(&Key::Delete, &alt()),
            Some(EditorAction::DeleteWordRight)
        );
    }

    #[test]
    fn meta_arrows_map_to_home_end_doc() {
        assert_eq!(
            key_to_editor_action(&Key::Left, &meta()),
            Some(EditorAction::Home)
        );
        assert_eq!(
            key_to_editor_action(&Key::Right, &meta()),
            Some(EditorAction::End)
        );
        assert_eq!(
            key_to_editor_action(&Key::Up, &meta()),
            Some(EditorAction::MoveDocStart)
        );
        assert_eq!(
            key_to_editor_action(&Key::Down, &meta()),
            Some(EditorAction::MoveDocEnd)
        );
    }

    #[test]
    fn meta_backspace_delete_maps_to_line_delete() {
        assert_eq!(
            key_to_editor_action(&Key::Backspace, &meta()),
            Some(EditorAction::DeleteToLineStart)
        );
        assert_eq!(
            key_to_editor_action(&Key::Delete, &meta()),
            Some(EditorAction::DeleteToLineEnd)
        );
    }

    #[test]
    fn meta_shift_k_maps_to_delete_line() {
        assert_eq!(
            key_to_editor_action(&Key::Char('k'), &meta_shift()),
            Some(EditorAction::DeleteLine)
        );
    }

    #[test]
    fn shift_tab_maps_to_unindent() {
        assert_eq!(
            key_to_editor_action(&Key::Tab, &shift()),
            Some(EditorAction::Unindent)
        );
    }
}
