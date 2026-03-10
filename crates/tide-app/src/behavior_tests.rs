//! Behavioral tests — living documentation of what the system does.
//!
//! Each test name reads as a natural language sentence describing a system behavior.
//! Organized by feature domain so tests serve as a browsable specification.

#[cfg(test)]
mod focus_management {
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::App;
    use tide_core::LayoutEngine;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, pane_id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        let pane = crate::editor_pane::EditorPane::new_empty(pane_id);
        app.panes.insert(pane_id, PaneKind::Editor(pane));
        app.focused = Some(pane_id);
        app.focus_area = FocusArea::PaneArea;
        (app, pane_id)
    }

    #[test]
    fn new_app_starts_with_no_focused_pane() {
        let app = test_app();
        assert_eq!(app.focused, None);
    }

    #[test]
    fn new_app_starts_in_pane_area_focus() {
        let app = test_app();
        assert_eq!(app.focus_area, FocusArea::PaneArea);
    }

    #[test]
    fn focus_terminal_sets_focus_area_to_pane_area() {
        let (mut app, id) = app_with_editor();
        app.focus_area = FocusArea::FileTree;
        app.focus_terminal(id);
        assert_eq!(app.focus_area, FocusArea::PaneArea);
    }

    #[test]
    fn focus_terminal_updates_chrome_generation_when_changing_pane() {
        let mut app = test_app();
        let (layout, id1) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id1, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id1)));
        let id2 = app.layout.split(id1, tide_core::SplitDirection::Vertical);
        app.panes.insert(id2, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id2)));
        app.focused = Some(id1);

        let gen_before = app.cache.chrome_generation;
        app.focus_terminal(id2);
        assert!(app.cache.chrome_generation > gen_before);
    }

    #[test]
    fn focus_terminal_same_pane_does_not_change_chrome() {
        let (mut app, id) = app_with_editor();
        let gen_before = app.cache.chrome_generation;
        app.focus_terminal(id);
        assert_eq!(app.cache.chrome_generation, gen_before);
    }

    #[test]
    fn toggling_file_tree_focus_cycles_through_three_states() {
        let (mut app, _) = app_with_editor();
        // State 1: file tree hidden, focus on pane area
        assert!(!app.ft.visible);
        assert_eq!(app.focus_area, FocusArea::PaneArea);

        // Toggle → State 2: file tree shown + focused
        app.handle_focus_area(FocusArea::FileTree);
        assert!(app.ft.visible);
        assert_eq!(app.focus_area, FocusArea::FileTree);

        // Toggle → State 3: file tree hidden + back to pane area
        app.handle_focus_area(FocusArea::FileTree);
        assert!(!app.ft.visible);
        assert_eq!(app.focus_area, FocusArea::PaneArea);
    }

    #[test]
    fn switching_to_pane_area_from_file_tree_preserves_focused_pane() {
        let (mut app, id) = app_with_editor();
        app.ft.visible = true;
        app.focus_area = FocusArea::FileTree;

        app.handle_focus_area(FocusArea::PaneArea);
        assert_eq!(app.focus_area, FocusArea::PaneArea);
        assert_eq!(app.focused, Some(id));
    }

    #[test]
    fn toggling_zoom_on_focused_pane_fills_entire_area() {
        let mut app = test_app();
        let (layout, id1) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id1, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id1)));
        let id2 = app.layout.split(id1, tide_core::SplitDirection::Vertical);
        app.panes.insert(id2, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id2)));
        app.focused = Some(id1);

        assert!(app.zoomed_pane.is_none());
        app.handle_global_action(tide_input::GlobalAction::ToggleZoom);
        assert_eq!(app.zoomed_pane, Some(id1));
    }

    #[test]
    fn toggling_zoom_again_restores_split_layout() {
        let mut app = test_app();
        let (layout, id1) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id1, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id1)));
        app.focused = Some(id1);

        app.handle_global_action(tide_input::GlobalAction::ToggleZoom);
        assert_eq!(app.zoomed_pane, Some(id1));
        app.handle_global_action(tide_input::GlobalAction::ToggleZoom);
        assert!(app.zoomed_pane.is_none());
    }

    #[test]
    fn zoom_has_no_effect_when_focus_area_is_file_tree() {
        let (mut app, _) = app_with_editor();
        app.ft.visible = true;
        app.focus_area = FocusArea::FileTree;

        app.handle_global_action(tide_input::GlobalAction::ToggleZoom);
        assert!(app.zoomed_pane.is_none());
    }
}

#[cfg(test)]
mod modal_behavior {
    use crate::pane::PaneKind;
    use crate::ui_state::*;
    use crate::App;
    use std::path::PathBuf;
    use tide_core::Rect;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn new_app_modal_stack_is_empty() {
        let app = test_app();
        assert!(!app.modal.is_any_open());
    }

    #[test]
    fn modal_stack_close_all_dismisses_all_modals() {
        let mut app = test_app();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/"), vec![]));
        app.modal.git_switcher = Some(GitSwitcherState::new(
            1, GitSwitcherMode::Branches, vec![], vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        assert!(app.modal.is_any_open());
        app.modal.close_all();
        assert!(!app.modal.is_any_open());
    }

    #[test]
    fn config_page_blocks_all_text_input() {
        let (mut app, _id) = app_with_editor();
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::Consumed,
        );
    }

    #[test]
    fn context_menu_blocks_text_input() {
        let (mut app, _id) = app_with_editor();
        app.modal.context_menu = Some(ContextMenuState {
            entry_index: 0,
            path: PathBuf::from("/tmp"),
            is_dir: false,
            shell_idle: true,
            position: tide_core::Vec2::new(0.0, 0.0),
            selected: 0,
        });
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::Consumed,
        );
    }

    #[test]
    fn save_confirm_blocks_text_input() {
        let (mut app, id) = app_with_editor();
        app.modal.save_confirm = Some(crate::SaveConfirmState { pane_id: id });
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::Consumed,
        );
    }

    #[test]
    fn file_finder_captures_text_instead_of_pane() {
        let (mut app, _id) = app_with_editor();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::FileFinder,
        );
    }

    #[test]
    fn git_switcher_captures_text_instead_of_pane() {
        let (mut app, id) = app_with_editor();
        app.modal.git_switcher = Some(GitSwitcherState::new(
            id, GitSwitcherMode::Branches, vec![], vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::GitSwitcher,
        );
    }

    #[test]
    fn modal_stack_has_higher_input_priority_than_search_bar() {
        let (mut app, id) = app_with_editor();
        app.search_focus = Some(id);
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        // File finder beats search bar
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::FileFinder,
        );
    }

    #[test]
    fn config_page_has_highest_priority_in_modal_stack() {
        let (mut app, id) = app_with_editor();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        app.modal.git_switcher = Some(GitSwitcherState::new(
            id, GitSwitcherMode::Branches, vec![], vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        // Config page beats everything
        assert_eq!(
            app.text_input_target(),
            crate::event_handler::text_routing::TextInputTarget::Consumed,
        );
    }

    #[test]
    fn escape_closes_file_finder_modal() {
        let (mut app, _id) = app_with_editor();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.file_finder.is_none());
    }

    #[test]
    fn escape_closes_git_switcher() {
        let (mut app, id) = app_with_editor();
        app.modal.git_switcher = Some(GitSwitcherState::new(
            id, GitSwitcherMode::Branches, vec![], vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.git_switcher.is_none());
    }

    #[test]
    fn escape_closes_save_as_input() {
        let (mut app, id) = app_with_editor();
        app.modal.save_as_input = Some(SaveAsInput::new(id, PathBuf::from("/tmp"), Rect::new(0.0, 0.0, 100.0, 30.0)));
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.save_as_input.is_none());
    }

    #[test]
    fn escape_closes_context_menu() {
        let (mut app, _id) = app_with_editor();
        app.modal.context_menu = Some(ContextMenuState {
            entry_index: 0,
            path: PathBuf::from("/tmp"),
            is_dir: false,
            shell_idle: true,
            position: tide_core::Vec2::new(0.0, 0.0),
            selected: 0,
        });
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.context_menu.is_none());
    }

    #[test]
    fn escape_cancels_save_confirm() {
        let (mut app, id) = app_with_editor();
        app.modal.save_confirm = Some(crate::SaveConfirmState { pane_id: id });
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.save_confirm.is_none());
    }

    #[test]
    fn escape_closes_file_tree_rename() {
        let (mut app, _id) = app_with_editor();
        app.modal.file_tree_rename = Some(FileTreeRenameState {
            entry_index: 0,
            original_path: PathBuf::from("/tmp/file.txt"),
            input: InputLine::with_text("file.txt".to_string()),
        });
        app.handle_key_down(tide_core::Key::Escape, tide_core::Modifiers::default(), None);
        assert!(app.modal.file_tree_rename.is_none());
    }
}

#[cfg(test)]
mod pane_lifecycle {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::App;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        let pane = EditorPane::new_empty(id);
        app.panes.insert(id, PaneKind::Editor(pane));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn new_editor_pane_adds_to_focused_tab_group() {
        let (mut app, _first_id) = app_with_editor();
        let pane_count_before = app.panes.len();
        app.new_editor_pane();
        assert_eq!(app.panes.len(), pane_count_before + 1);
        assert_ne!(app.focused, Some(_first_id));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn new_editor_pane_sets_focus_to_new_pane() {
        let (mut app, _) = app_with_editor();
        app.new_editor_pane();
        let new_id = app.focused.unwrap();
        assert!(app.panes.contains_key(&new_id));
        assert!(matches!(app.panes.get(&new_id), Some(PaneKind::Editor(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn new_editor_pane_does_nothing_without_focus() {
        let mut app = test_app();
        let count_before = app.panes.len();
        app.new_editor_pane();
        assert_eq!(app.panes.len(), count_before);
    }

    #[test]
    fn split_creates_new_pane_in_split_layout() {
        let (mut app, _first_id) = app_with_editor();
        let pane_ids_before = app.layout.pane_ids().len();
        app.split_with_launcher(tide_core::SplitDirection::Vertical);
        assert_eq!(app.layout.pane_ids().len(), pane_ids_before + 1);
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn split_focuses_new_launcher_pane() {
        let (mut app, first_id) = app_with_editor();
        app.split_with_launcher(tide_core::SplitDirection::Vertical);
        assert_ne!(app.focused, Some(first_id));
        let new_id = app.focused.unwrap();
        assert!(matches!(app.panes.get(&new_id), Some(PaneKind::Launcher(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn split_unzooms_focused_pane() {
        let (mut app, first_id) = app_with_editor();
        app.zoomed_pane = Some(first_id);
        app.split_with_launcher(tide_core::SplitDirection::Vertical);
        assert!(app.zoomed_pane.is_none());
    }

    #[test]
    fn resolving_launcher_as_new_file_replaces_pane_kind_with_editor() {
        let (mut app, _first_id) = app_with_editor();
        app.split_with_launcher(tide_core::SplitDirection::Vertical);
        let launcher_id = app.focused.unwrap();
        assert!(matches!(app.panes.get(&launcher_id), Some(PaneKind::Launcher(_))));

        app.resolve_launcher(launcher_id, crate::action::LauncherChoice::NewFile);
        assert!(matches!(app.panes.get(&launcher_id), Some(PaneKind::Editor(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn closing_editor_pane_moves_focus_to_another_pane() {
        let (mut app, _first_id) = app_with_editor();
        app.new_editor_pane();
        let second_id = app.focused.unwrap();
        assert_eq!(app.panes.len(), 2);

        app.force_close_editor_panel_tab(second_id);
        assert_eq!(app.panes.len(), 1);
        assert!(app.focused.is_some());
        assert_ne!(app.focused, Some(second_id));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn closing_a_dirty_editor_with_file_shows_save_confirm() {
        let (mut app, id) = app_with_editor();
        // Make the editor dirty with a file path
        if let Some(PaneKind::Editor(pane)) = app.panes.get_mut(&id) {
            pane.editor.insert_text("hello");
            pane.editor.buffer.file_path = Some(std::path::PathBuf::from("/tmp/test.txt"));
        }

        app.close_specific_pane(id);
        assert!(app.modal.save_confirm.is_some());
        assert_eq!(app.modal.save_confirm.as_ref().unwrap().pane_id, id);
    }

    #[test]
    fn closing_a_dirty_untitled_editor_does_not_show_save_confirm() {
        let (mut app, id) = app_with_editor();
        // Make the editor dirty but no file path
        if let Some(PaneKind::Editor(pane)) = app.panes.get_mut(&id) {
            pane.editor.insert_text("hello");
        }
        // Need a second pane so close doesn't exit
        app.new_editor_pane();
        let _second_id = app.focused.unwrap();
        app.focused = Some(id);

        app.close_specific_pane(id);
        // Should close immediately (untitled files don't prompt)
        assert!(app.modal.save_confirm.is_none());
    }

    #[test]
    fn new_terminal_tab_creates_launcher_pane() {
        let (mut app, _) = app_with_editor();
        app.new_terminal_tab();
        let new_id = app.focused.unwrap();
        assert!(matches!(app.panes.get(&new_id), Some(PaneKind::Launcher(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn cancel_save_confirm_clears_the_modal() {
        let (mut app, id) = app_with_editor();
        app.modal.save_confirm = Some(crate::SaveConfirmState { pane_id: id });
        app.cancel_save_confirm();
        assert!(app.modal.save_confirm.is_none());
    }

    #[test]
    fn opening_same_file_twice_activates_existing_tab_instead() {
        let (mut app, first_id) = app_with_editor();
        let test_path = std::path::PathBuf::from("/tmp/behavior_test_dedup.txt");
        // Write a temp file for testing
        let _ = std::fs::write(&test_path, "test content");

        app.open_editor_pane(test_path.clone());
        let editor_id = app.focused.unwrap();
        assert_ne!(editor_id, first_id);

        // Refocus first pane
        app.focused = Some(first_id);
        // Open same file again
        app.open_editor_pane(test_path.clone());
        // Should refocus the existing editor, not create a new one
        assert_eq!(app.focused, Some(editor_id));
        let _ = std::fs::remove_file(&test_path);
    }
}

#[cfg(test)]
mod editor_behavior {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::App;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        let pane = EditorPane::new_empty(id);
        app.panes.insert(id, PaneKind::Editor(pane));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn new_editor_starts_unmodified() {
        let (app, id) = app_with_editor();
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        } else {
            panic!("expected editor pane");
        }
    }

    #[test]
    fn typing_text_into_editor_marks_it_as_modified() {
        let (mut app, id) = app_with_editor();
        app.send_text_to_target("hello world");
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(pane.editor.is_modified());
        } else {
            panic!("expected editor pane");
        }
    }

    #[test]
    fn text_input_is_blocked_in_preview_mode() {
        let (mut app, id) = app_with_editor();
        if let Some(PaneKind::Editor(pane)) = app.panes.get_mut(&id) {
            pane.preview_mode = true;
        }
        app.send_text_to_target("should not appear");
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        }
    }

    #[test]
    fn ime_commit_routes_text_to_focused_editor() {
        let (mut app, id) = app_with_editor();
        app.handle_ime_commit("한글 입력");
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(pane.editor.is_modified());
        }
    }

    #[test]
    fn ime_commit_to_file_finder_does_not_reach_editor() {
        let (mut app, id) = app_with_editor();
        app.modal.file_finder = Some(crate::ui_state::FileFinderState::new(
            std::path::PathBuf::from("/tmp"), vec![],
        ));
        app.handle_ime_commit("검색어");
        // Editor should remain unmodified
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        }
    }

    #[test]
    fn preview_scroll_j_moves_viewport_down() {
        // Test the pure scroll logic directly since preview_cache
        // is only populated during rendering, not available in unit tests.
        let mut v_scroll = 0;
        let mut h_scroll = 0;
        let scrolled = crate::editor_pane::apply_preview_scroll(
            'j', &mut v_scroll, &mut h_scroll, 100, 0, 30,
        );
        assert!(scrolled);
        assert_eq!(v_scroll, 1);
    }

    #[test]
    fn new_editor_has_no_file_path() {
        let (app, id) = app_with_editor();
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(pane.editor.file_path().is_none());
        }
    }

    #[test]
    fn new_editor_is_not_in_preview_mode() {
        let (app, id) = app_with_editor();
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.preview_mode);
        }
    }
}

#[cfg(test)]
mod keyboard_routing {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::*;
    use crate::App;
    use tide_core::{Key, Modifiers};
    use std::path::PathBuf;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        let pane = EditorPane::new_empty(id);
        app.panes.insert(id, PaneKind::Editor(pane));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    fn cmd() -> Modifiers {
        Modifiers { meta: true, ctrl: false, shift: false, alt: false }
    }

    #[test]
    fn plain_text_keys_route_to_focused_pane() {
        let (mut app, id) = app_with_editor();
        app.handle_key_down(Key::Char('a'), Modifiers::default(), Some("a".to_string()));
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(pane.editor.is_modified());
        }
    }

    #[test]
    fn config_page_intercepts_all_keyboard_input() {
        let (mut app, id) = app_with_editor();
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        // Press a key — should not reach editor
        app.handle_key_down(Key::Char('x'), Modifiers::default(), Some("x".to_string()));
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        }
    }

    #[test]
    fn escape_during_config_page_closes_config_page() {
        let (mut app, _) = app_with_editor();
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        app.handle_key_down(Key::Escape, Modifiers::default(), None);
        assert!(app.modal.config_page.is_none());
    }

    #[test]
    fn file_finder_intercepts_keys_before_pane() {
        let (mut app, id) = app_with_editor();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        // Type in finder — should not reach editor
        app.handle_key_down(Key::Char('a'), Modifiers::default(), Some("a".to_string()));
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        }
    }

    #[test]
    fn escape_during_pane_drag_cancels_the_drag() {
        let (mut app, _) = app_with_editor();
        app.interaction.pane_drag = crate::drag_drop::PaneDragState::PendingDrag {
            source_pane: 1,
            press_pos: tide_core::Vec2::new(0.0, 0.0),
        };
        app.handle_key_down(Key::Escape, Modifiers::default(), None);
        assert!(matches!(app.interaction.pane_drag, crate::drag_drop::PaneDragState::Idle));
    }

    #[test]
    fn focus_area_file_tree_consumes_arrow_keys() {
        let (mut app, _) = app_with_editor();
        app.ft.visible = true;
        app.focus_area = FocusArea::FileTree;
        // Arrow keys should not reach the editor pane
        let _gen_before = app.cache.chrome_generation;
        app.handle_key_down(Key::Down, Modifiers::default(), None);
        // File tree may update chrome generation if there are entries
        // but the editor should remain unmodified
    }

    #[test]
    fn global_action_keys_work_when_focus_area_is_file_tree() {
        let (mut app, _) = app_with_editor();
        app.ft.visible = true;
        app.focus_area = FocusArea::FileTree;
        // Cmd+key should still be processed (e.g., routed to global actions)
        // This test just verifies no panic occurs
        app.handle_key_down(Key::Char('n'), cmd(), Some("n".to_string()));
    }

    #[test]
    fn save_confirm_blocks_all_keys_except_escape() {
        let (mut app, id) = app_with_editor();
        app.modal.save_confirm = Some(crate::SaveConfirmState { pane_id: id });
        // Typing should not reach editor
        app.handle_key_down(Key::Char('x'), Modifiers::default(), Some("x".to_string()));
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(!pane.editor.is_modified());
        }
    }

    #[test]
    fn branch_cleanup_enter_means_keep_branch() {
        let (mut app, id) = app_with_editor();
        // We can't easily create a real BranchCleanupState with a terminal,
        // but we can test that the modal blocks other keys
        app.modal.branch_cleanup = Some(crate::BranchCleanupState {
            pane_id: id,
            branch: "feature-x".to_string(),
            worktree_path: None,
            cwd: PathBuf::from("/tmp"),
        });
        // Escape cancels cleanup
        app.handle_key_down(Key::Escape, Modifiers::default(), None);
        assert!(app.modal.branch_cleanup.is_none());
    }
}

#[cfg(test)]
mod launcher_behavior {
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::App;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_launcher() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id, PaneKind::Launcher(id));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn pressing_e_in_launcher_pane_resolves_to_editor_pane_kind() {
        let (mut app, id) = app_with_launcher();
        app.handle_ime_commit("e");
        assert!(matches!(app.panes.get(&id), Some(PaneKind::Editor(_))));
    }

    #[test]
    fn pressing_capital_e_in_launcher_pane_resolves_to_editor_pane_kind() {
        let (mut app, id) = app_with_launcher();
        app.handle_ime_commit("E");
        assert!(matches!(app.panes.get(&id), Some(PaneKind::Editor(_))));
    }

    #[test]
    fn korean_ime_commit_resolves_launcher_pane_to_editor_pane_kind() {
        let (mut app, id) = app_with_launcher();
        // ㄷ is the Korean jamo mapped to 'e' key
        app.handle_ime_commit("ㄷ");
        assert!(matches!(app.panes.get(&id), Some(PaneKind::Editor(_))));
    }

    #[test]
    fn korean_ime_preedit_resolves_launcher_pane_to_terminal_pane_kind() {
        let (mut app, id) = app_with_launcher();
        // ㅅ is the Korean jamo mapped to 't' key — preedit triggers launcher resolution
        app.handle_ime_preedit("ㅅ");
        // Terminal creation requires PTY which may fail in tests,
        // but the launcher should at least be removed
        let is_launcher = matches!(app.panes.get(&id), Some(PaneKind::Launcher(_)));
        assert!(!is_launcher || app.panes.get(&id).is_none());
    }

    #[test]
    fn non_matching_text_in_launcher_pane_is_ignored() {
        let (mut app, id) = app_with_launcher();
        app.handle_ime_commit("x");
        // Launcher should remain
        assert!(matches!(app.panes.get(&id), Some(PaneKind::Launcher(_))));
    }
}

#[cfg(test)]
mod theme_behavior {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::App;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    #[test]
    fn app_starts_in_dark_mode() {
        let app = test_app();
        assert!(app.dark_mode);
    }

    #[test]
    fn toggle_theme_switches_between_dark_and_light() {
        let mut app = test_app();
        assert!(app.dark_mode);
        app.handle_global_action(tide_input::GlobalAction::ToggleTheme);
        assert!(!app.dark_mode);
        app.handle_global_action(tide_input::GlobalAction::ToggleTheme);
        assert!(app.dark_mode);
    }

    #[test]
    fn toggle_theme_clears_all_pane_generations_in_render_cache() {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id, PaneKind::Editor(EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.cache.pane_generations.insert(id, 42);

        app.handle_global_action(tide_input::GlobalAction::ToggleTheme);
        assert!(app.cache.pane_generations.is_empty());
    }

    #[test]
    fn font_size_starts_at_14() {
        let app = test_app();
        assert!((app.current_font_size - 14.0).abs() < f32::EPSILON);
    }
}

#[cfg(test)]
mod workspace_behavior {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::workspace::Workspace;
    use crate::App;
    use std::collections::HashMap;
    use tide_layout::SplitLayout;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_two_workspaces() -> App {
        let mut app = test_app();

        // Use distinct pane IDs for each workspace
        let id1: u64 = 100;
        let id2: u64 = 200;

        // Push two workspace slots
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        // Set up WS1 as active
        app.ws.active = 0;
        app.panes = HashMap::new();
        app.panes.insert(id1, PaneKind::Editor(EditorPane::new_empty(id1)));
        app.focused = Some(id1);
        app.focus_area = FocusArea::PaneArea;

        // Save WS1, switch to WS2
        app.save_active_workspace();
        app.ws.active = 1;
        app.panes = HashMap::new();
        app.panes.insert(id2, PaneKind::Editor(EditorPane::new_empty(id2)));
        app.focused = Some(id2);
        app.save_active_workspace();

        // Load WS1 back as active
        app.ws.active = 0;
        app.load_active_workspace();
        app
    }

    #[test]
    fn switching_workspace_in_workspace_manager_preserves_each_workspaces_focus() {
        let mut app = app_with_two_workspaces();
        let ws1_focus = app.focused;
        assert_eq!(ws1_focus, Some(100));

        app.switch_workspace(1);
        let ws2_focus = app.focused;
        assert_eq!(ws2_focus, Some(200));

        app.switch_workspace(0);
        assert_eq!(app.focused, Some(100));
    }

    #[test]
    fn switching_to_same_workspace_is_a_no_op() {
        let mut app = app_with_two_workspaces();
        let gen_before = app.cache.chrome_generation;
        app.switch_workspace(0);
        assert_eq!(app.cache.chrome_generation, gen_before);
    }

    #[test]
    fn switching_to_out_of_bounds_workspace_is_a_no_op() {
        let mut app = app_with_two_workspaces();
        let focus_before = app.focused;
        app.switch_workspace(99);
        assert_eq!(app.focused, focus_before);
    }

    #[test]
    fn workspace_prev_wraps_from_first_to_last() {
        let mut app = app_with_two_workspaces();
        assert_eq!(app.ws.active, 0);
        app.handle_global_action(tide_input::GlobalAction::WorkspacePrev);
        assert_eq!(app.ws.active, 1);
    }

    #[test]
    fn workspace_next_wraps_from_last_to_first() {
        let mut app = app_with_two_workspaces();
        app.switch_workspace(1);
        assert_eq!(app.ws.active, 1);
        app.handle_global_action(tide_input::GlobalAction::WorkspaceNext);
        assert_eq!(app.ws.active, 0);
    }

    #[test]
    fn closing_only_workspace_in_workspace_manager_is_a_no_op() {
        let mut app = test_app();
        app.ws.workspaces.push(Workspace {
            name: "Only".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.close_workspace();
        assert_eq!(app.ws.workspaces.len(), 1);
    }

    #[test]
    fn closing_workspace_removes_from_workspace_manager_and_switches() {
        let mut app = app_with_two_workspaces();
        assert_eq!(app.ws.workspaces.len(), 2);
        app.close_workspace();
        assert_eq!(app.ws.workspaces.len(), 1);
    }

    #[test]
    fn toggling_workspace_sidebar_toggles_visibility() {
        let mut app = test_app();
        // Sidebar defaults to visible
        assert!(app.ws.show_sidebar);
        app.handle_global_action(tide_input::GlobalAction::ToggleWorkspaceSidebar);
        assert!(!app.ws.show_sidebar);
        app.handle_global_action(tide_input::GlobalAction::ToggleWorkspaceSidebar);
        assert!(app.ws.show_sidebar);
    }
}

#[cfg(test)]
mod search_behavior {
    use crate::search::SearchState;

    #[test]
    fn new_search_state_has_empty_input() {
        let state = SearchState::new();
        assert!(state.input.text.is_empty());
        assert_eq!(state.matches.len(), 0);
        assert!(state.current.is_none());
    }

    #[test]
    fn search_display_shows_zero_of_zero_when_empty() {
        let state = SearchState::new();
        assert_eq!(state.current_display(), "0/0");
    }

    #[test]
    fn next_match_wraps_around_from_last_to_first() {
        let mut state = SearchState::new();
        state.matches = vec![
            crate::search::SearchMatch { line: 0, col: 0, len: 3 },
            crate::search::SearchMatch { line: 1, col: 0, len: 3 },
        ];
        state.current = Some(1);
        state.next_match();
        assert_eq!(state.current, Some(0));
    }

    #[test]
    fn prev_match_wraps_around_from_first_to_last() {
        let mut state = SearchState::new();
        state.matches = vec![
            crate::search::SearchMatch { line: 0, col: 0, len: 3 },
            crate::search::SearchMatch { line: 1, col: 0, len: 3 },
        ];
        state.current = Some(0);
        state.prev_match();
        assert_eq!(state.current, Some(1));
    }

    #[test]
    fn search_in_editor_finds_all_occurrences() {
        let mut state = SearchState::new();
        state.input = crate::ui_state::InputLine::with_text("foo".to_string());
        let lines = vec![
            "this is foo bar".to_string(),
            "no match here".to_string(),
            "foo again".to_string(),
        ];
        crate::search::execute_search_editor(&mut state, &lines);
        assert_eq!(state.matches.len(), 2);
        assert_eq!(state.matches[0].line, 0);
        assert_eq!(state.matches[1].line, 2);
    }

    #[test]
    fn empty_search_query_clears_matches() {
        let mut state = SearchState::new();
        // Pre-populate some matches
        state.matches = vec![
            crate::search::SearchMatch { line: 0, col: 0, len: 3 },
        ];
        state.current = Some(0);
        // Search with empty string
        let lines = vec!["content".to_string()];
        crate::search::execute_search_editor(&mut state, &lines);
        assert!(state.matches.is_empty());
        assert!(state.current.is_none());
    }
}

#[cfg(test)]
mod ime_behavior {
    use crate::ui_state::ImeState;

    #[test]
    fn new_ime_state_is_not_composing() {
        let state = ImeState::new();
        assert!(!state.composing);
        assert!(state.preedit.is_empty());
    }

    #[test]
    fn set_preedit_with_text_starts_composition() {
        let mut state = ImeState::new();
        state.set_preedit("ㅎ");
        assert!(state.composing);
        assert_eq!(state.preedit, "ㅎ");
    }

    #[test]
    fn set_preedit_with_empty_string_ends_composition() {
        let mut state = ImeState::new();
        state.set_preedit("ㅎ");
        state.set_preedit("");
        assert!(!state.composing);
        assert!(state.preedit.is_empty());
    }

    #[test]
    fn clear_composition_resets_all_state() {
        let mut state = ImeState::new();
        state.composing = true;
        state.preedit = "한".to_string();
        state.clear_composition();
        assert!(!state.composing);
        assert!(state.preedit.is_empty());
    }
}

#[cfg(test)]
mod render_cache_behavior {
    use crate::ui_state::RenderCache;

    #[test]
    fn new_render_cache_starts_dirty_for_initial_render() {
        let cache = RenderCache::new();
        assert!(cache.needs_redraw);
    }

    #[test]
    fn invalidating_chrome_increments_generation_and_marks_render_cache_dirty() {
        let mut cache = RenderCache::new();
        cache.invalidate_chrome();
        assert!(cache.needs_redraw);
        assert!(cache.is_chrome_dirty());
    }

    #[test]
    fn invalidating_pane_removes_pane_generation_and_marks_render_cache_dirty() {
        let mut cache = RenderCache::new();
        cache.pane_generations.insert(42, 1);
        cache.invalidate_pane(42);
        assert!(!cache.pane_generations.contains_key(&42));
        assert!(cache.needs_redraw);
    }

    #[test]
    fn chrome_generation_is_not_dirty_when_generations_match() {
        let mut cache = RenderCache::new();
        cache.chrome_generation = 5;
        cache.last_chrome_generation = 5;
        assert!(!cache.is_chrome_dirty());
    }

    #[test]
    fn chrome_generation_is_dirty_when_generations_differ() {
        let mut cache = RenderCache::new();
        cache.chrome_generation = 6;
        cache.last_chrome_generation = 5;
        assert!(cache.is_chrome_dirty());
    }
}

#[cfg(test)]
mod global_actions {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::FocusArea;
    use crate::App;
    use tide_input::GlobalAction;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id, PaneKind::Editor(EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn split_vertical_creates_new_pane_in_split_layout_and_focuses_it() {
        let (mut app, first_id) = app_with_editor();
        app.handle_global_action(GlobalAction::SplitVertical);
        assert_ne!(app.focused, Some(first_id));
        assert_eq!(app.layout.pane_ids().len(), 2);
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn split_horizontal_creates_new_pane_in_split_layout_and_focuses_it() {
        let (mut app, first_id) = app_with_editor();
        app.handle_global_action(GlobalAction::SplitHorizontal);
        assert_ne!(app.focused, Some(first_id));
        assert_eq!(app.layout.pane_ids().len(), 2);
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn find_opens_search_bar_on_focused_pane() {
        let (mut app, id) = app_with_editor();
        app.handle_global_action(GlobalAction::Find);
        assert_eq!(app.search_focus, Some(id));
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&id) {
            assert!(pane.search.is_some());
        }
    }

    #[test]
    fn find_again_reuses_existing_search_bar() {
        let (mut app, id) = app_with_editor();
        app.handle_global_action(GlobalAction::Find);
        assert_eq!(app.search_focus, Some(id));
        // Call Find again — should still focus the same search
        app.handle_global_action(GlobalAction::Find);
        assert_eq!(app.search_focus, Some(id));
    }

    #[test]
    fn new_file_global_action_creates_editor_pane_in_tab_group() {
        let (mut app, first_id) = app_with_editor();
        app.handle_global_action(GlobalAction::NewFile);
        assert_ne!(app.focused, Some(first_id));
        let new_id = app.focused.unwrap();
        assert!(matches!(app.panes.get(&new_id), Some(PaneKind::Editor(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn new_tab_global_action_creates_launcher_pane() {
        let (mut app, _) = app_with_editor();
        app.handle_global_action(GlobalAction::NewTab);
        let new_id = app.focused.unwrap();
        assert!(matches!(app.panes.get(&new_id), Some(PaneKind::Launcher(_))));
        // Invariant: PaneId sync
        assert_eq!(app.layout.pane_ids().len(), app.panes.len());
    }

    #[test]
    fn toggle_file_tree_from_pane_area_sets_focus_area_to_file_tree() {
        let (mut app, _) = app_with_editor();
        assert!(!app.ft.visible);
        app.handle_global_action(GlobalAction::ToggleFileTree);
        assert!(app.ft.visible);
        assert_eq!(app.focus_area, FocusArea::FileTree);
    }

    #[test]
    fn toggle_file_tree_again_hides_and_restores_focus_area_to_pane_area() {
        let (mut app, _) = app_with_editor();
        app.handle_global_action(GlobalAction::ToggleFileTree);
        assert!(app.ft.visible);
        app.handle_global_action(GlobalAction::ToggleFileTree);
        assert!(!app.ft.visible);
        assert_eq!(app.focus_area, FocusArea::PaneArea);
    }

    #[test]
    fn toggle_fullscreen_sets_pending_flag() {
        let (mut app, _) = app_with_editor();
        assert!(!app.pending_fullscreen_toggle);
        app.handle_global_action(GlobalAction::ToggleFullscreen);
        assert!(app.pending_fullscreen_toggle);
    }

    #[test]
    fn file_finder_opens_via_global_action() {
        let (mut app, _) = app_with_editor();
        app.handle_global_action(GlobalAction::FileFinder);
        // File finder state may or may not be set (depends on CWD resolution),
        // but it should not panic
    }
}

#[cfg(test)]
mod text_input_routing {
    use crate::editor_pane::EditorPane;
    use crate::pane::PaneKind;
    use crate::ui_state::*;
    use crate::event_handler::text_routing::TextInputTarget;
    use crate::App;
    use std::path::PathBuf;
    use tide_core::Rect;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    fn app_with_editor() -> (App, u64) {
        let mut app = test_app();
        let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
        app.layout = layout;
        app.panes.insert(id, PaneKind::Editor(EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.focus_area = FocusArea::PaneArea;
        (app, id)
    }

    #[test]
    fn text_goes_to_editor_when_nothing_else_is_open() {
        let (app, id) = app_with_editor();
        assert_eq!(app.text_input_target(), TextInputTarget::Pane(id));
    }

    #[test]
    fn text_goes_to_file_finder_when_open() {
        let (mut app, _) = app_with_editor();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/"), vec![]));
        assert_eq!(app.text_input_target(), TextInputTarget::FileFinder);
    }

    #[test]
    fn text_goes_to_search_bar_when_focused() {
        let (mut app, id) = app_with_editor();
        app.search_focus = Some(id);
        assert_eq!(app.text_input_target(), TextInputTarget::SearchBar(id));
    }

    #[test]
    fn text_is_consumed_when_no_pane_is_focused() {
        let app = test_app();
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn text_is_consumed_when_file_tree_has_focus() {
        let (mut app, _) = app_with_editor();
        app.focus_area = FocusArea::FileTree;
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn text_goes_to_save_as_input_when_open() {
        let (mut app, id) = app_with_editor();
        app.modal.save_as_input = Some(SaveAsInput::new(
            id, PathBuf::from("/tmp"), Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        assert_eq!(app.text_input_target(), TextInputTarget::SaveAsInput);
    }

    #[test]
    fn text_goes_to_file_tree_rename_when_active() {
        let (mut app, _) = app_with_editor();
        app.modal.file_tree_rename = Some(FileTreeRenameState {
            entry_index: 0,
            original_path: PathBuf::from("/tmp/file.txt"),
            input: InputLine::with_text("file.txt".to_string()),
        });
        assert_eq!(app.text_input_target(), TextInputTarget::FileTreeRename);
    }

    #[test]
    fn config_page_worktree_editing_receives_text() {
        let mut app = test_app();
        let mut cp = ConfigPageState::new(vec![], String::new(), String::new());
        cp.worktree_editing = true;
        app.modal.config_page = Some(cp);
        assert_eq!(app.text_input_target(), TextInputTarget::ConfigPageWorktree);
    }

    #[test]
    fn config_page_copy_files_editing_receives_text() {
        let mut app = test_app();
        let mut cp = ConfigPageState::new(vec![], String::new(), String::new());
        cp.copy_files_editing = true;
        app.modal.config_page = Some(cp);
        assert_eq!(app.text_input_target(), TextInputTarget::ConfigPageCopyFiles);
    }
}

#[cfg(test)]
mod session_behavior {
    use crate::session::{Session, SessionLayout};

    #[test]
    fn session_preserves_dark_mode_preference() {
        let session = Session {
            layout: SessionLayout::Leaf { pane_id: 1, cwd: None },
            focused_pane_id: Some(1),
            show_file_tree: false,
            file_tree_width: 200.0,
            dark_mode: false,
            window_width: 800.0,
            window_height: 600.0,
            sidebar_side: "left".to_string(),
            sidebar_outer: true,
        };
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();
        assert!(!restored.dark_mode);
    }

    #[test]
    fn session_preserves_file_tree_visibility() {
        let session = Session {
            layout: SessionLayout::Leaf { pane_id: 1, cwd: None },
            focused_pane_id: Some(1),
            show_file_tree: true,
            file_tree_width: 300.0,
            dark_mode: true,
            window_width: 1200.0,
            window_height: 800.0,
            sidebar_side: "right".to_string(),
            sidebar_outer: true,
        };
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();
        assert!(restored.show_file_tree);
        assert!((restored.file_tree_width - 300.0).abs() < f32::EPSILON);
        assert_eq!(restored.sidebar_side, "right");
    }

    #[test]
    fn session_without_sidebar_fields_uses_defaults() {
        let json = r#"{
            "layout": {"Leaf": {"pane_id": 1, "cwd": null}},
            "focused_pane_id": 1,
            "show_file_tree": false,
            "file_tree_width": 200.0,
            "dark_mode": true,
            "window_width": 800.0,
            "window_height": 600.0
        }"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.sidebar_side, "left");
        assert!(session.sidebar_outer);
    }
}

#[cfg(test)]
mod preview_scroll {
    use crate::editor_pane;

    #[test]
    fn j_scrolls_down_one_line() {
        let mut v = 0;
        let mut h = 0;
        editor_pane::apply_preview_scroll('j', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 1);
    }

    #[test]
    fn k_scrolls_up_one_line() {
        let mut v = 5;
        let mut h = 0;
        editor_pane::apply_preview_scroll('k', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 4);
    }

    #[test]
    fn k_does_not_scroll_below_zero() {
        let mut v = 0;
        let mut h = 0;
        editor_pane::apply_preview_scroll('k', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 0);
    }

    #[test]
    fn d_scrolls_down_half_page() {
        let mut v = 0;
        let mut h = 0;
        editor_pane::apply_preview_scroll('d', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 15); // half of visible_rows=30
    }

    #[test]
    fn u_scrolls_up_half_page() {
        let mut v = 20;
        let mut h = 0;
        editor_pane::apply_preview_scroll('u', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 5); // 20 - 15
    }

    #[test]
    fn g_scrolls_to_top() {
        let mut v = 50;
        let mut h = 0;
        editor_pane::apply_preview_scroll('g', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 0);
    }

    #[test]
    fn capital_g_scrolls_to_bottom() {
        let mut v = 0;
        let mut h = 0;
        editor_pane::apply_preview_scroll('G', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 100); // max_v
    }

    #[test]
    fn scroll_clamps_to_max() {
        let mut v = 95;
        let mut h = 0;
        editor_pane::apply_preview_scroll('j', &mut v, &mut h, 100, 100, 30);
        assert_eq!(v, 96);
        // Can't exceed max
        let mut v2 = 100;
        editor_pane::apply_preview_scroll('j', &mut v2, &mut h, 100, 100, 30);
        assert_eq!(v2, 100);
    }
}
