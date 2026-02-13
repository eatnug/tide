#[cfg(test)]
mod tests {
    use crate::*;
    use std::fs;
    use tempfile::TempDir;
    use tide_core::FileTreeSource;

    /// Helper to create a temp directory with some structure.
    fn setup_temp_dir() -> TempDir {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let root = tmp.path();

        // Create directories
        fs::create_dir(root.join("alpha_dir")).unwrap();
        fs::create_dir(root.join("beta_dir")).unwrap();

        // Create files
        fs::write(root.join("charlie.txt"), "hello").unwrap();
        fs::write(root.join("able.txt"), "world").unwrap();

        // Create a file inside alpha_dir
        fs::write(root.join("alpha_dir").join("inner.txt"), "inner").unwrap();

        tmp
    }

    #[test]
    fn test_set_root_populates_entries() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();
        assert!(!entries.is_empty(), "entries should be populated after set_root");
    }

    #[test]
    fn test_directories_sorted_before_files() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();

        // Find the index where directories end and files begin.
        let first_file_idx = entries.iter().position(|e| !e.entry.is_dir);
        let last_dir_idx = entries.iter().rposition(|e| e.entry.is_dir);

        if let (Some(first_file), Some(last_dir)) = (first_file_idx, last_dir_idx) {
            assert!(
                last_dir < first_file,
                "All directories should come before all files. last_dir={}, first_file={}",
                last_dir,
                first_file
            );
        }
    }

    #[test]
    fn test_alphabetical_within_groups() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();
        let names: Vec<&str> = entries.iter().map(|e| e.entry.name.as_str()).collect();

        // Directories: alpha_dir, beta_dir  (alphabetical)
        // Files: able.txt, charlie.txt  (alphabetical)
        assert_eq!(names, vec!["alpha_dir", "beta_dir", "able.txt", "charlie.txt"]);
    }

    #[test]
    fn test_toggle_expands_and_collapses_directory() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");

        // Initially collapsed: should have 4 top-level entries.
        assert_eq!(tree.visible_entries().len(), 4);

        // Expand alpha_dir.
        tree.toggle(&alpha_path);

        // Now we should see the inner file too: 4 + 1 = 5.
        assert_eq!(tree.visible_entries().len(), 5);

        // The alpha_dir entry should be marked as expanded.
        let alpha_entry = tree
            .visible_entries()
            .iter()
            .find(|e| e.entry.path == alpha_path)
            .expect("alpha_dir should be visible");
        assert!(alpha_entry.is_expanded);

        // Collapse alpha_dir.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 4);

        let alpha_entry = tree
            .visible_entries()
            .iter()
            .find(|e| e.entry.path == alpha_path)
            .expect("alpha_dir should be visible");
        assert!(!alpha_entry.is_expanded);
    }

    #[test]
    fn test_visible_entries_respects_collapsed_state() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        let beta_path = tmp.path().join("beta_dir");

        // Expand alpha_dir (has inner.txt) -- should add 1 child.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Expand beta_dir (empty) -- no new children.
        tree.toggle(&beta_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Collapse alpha_dir -- removes 1 child.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 4);
    }

    #[test]
    fn test_depth_of_nested_entries() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);

        for entry in tree.visible_entries() {
            if entry.entry.path == alpha_path {
                assert_eq!(entry.depth, 0, "alpha_dir should be at depth 0");
            }
            if entry.entry.name == "inner.txt" {
                assert_eq!(entry.depth, 1, "inner.txt should be at depth 1");
            }
        }
    }

    #[test]
    fn test_refresh_picks_up_new_files() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let initial_count = tree.visible_entries().len();
        assert_eq!(initial_count, 4);

        // Create a new file in the root.
        fs::write(tmp.path().join("new_file.txt"), "new").unwrap();

        // Before refresh, tree doesn't know about the new file.
        assert_eq!(tree.visible_entries().len(), 4);

        // After refresh, tree picks it up.
        tree.refresh();
        assert_eq!(tree.visible_entries().len(), 5);

        // The new file should be in the list.
        let has_new = tree
            .visible_entries()
            .iter()
            .any(|e| e.entry.name == "new_file.txt");
        assert!(has_new, "new_file.txt should appear after refresh");
    }

    #[test]
    fn test_refresh_picks_up_new_files_in_expanded_dir() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Add a new file inside expanded alpha_dir.
        fs::write(alpha_path.join("new_inner.txt"), "new inner").unwrap();

        tree.refresh();
        assert_eq!(tree.visible_entries().len(), 6);
    }

    #[test]
    fn test_set_root_resets_state() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Create a new temp dir and set it as root.
        let tmp2 = TempDir::new().unwrap();
        fs::write(tmp2.path().join("only.txt"), "only").unwrap();

        tree.set_root(tmp2.path().to_path_buf());

        assert_eq!(tree.root(), tmp2.path());
        assert_eq!(tree.visible_entries().len(), 1);
        assert!(!tree.expanded.contains(&alpha_path));
    }

    #[test]
    fn test_has_children_flag() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        for entry in tree.visible_entries() {
            if entry.entry.is_dir {
                assert!(entry.has_children, "directories should have has_children=true");
            } else {
                assert!(!entry.has_children, "files should have has_children=false");
            }
        }
    }

    #[test]
    fn test_permission_error_skips_entry() {
        // read_directory should not panic on a nonexistent path
        let entries = read_directory(Path::new("/nonexistent_path_12345"));
        assert!(entries.is_empty());
    }

    #[test]
    fn test_toggle_nonexistent_path_does_not_panic() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        // Toggling a path that doesn't exist should not panic.
        tree.toggle(Path::new("/nonexistent_path_12345"));
    }

    #[test]
    fn test_symlink_followed() {
        let tmp = setup_temp_dir();
        let root = tmp.path();

        // Create a directory and a symlink to it.
        let real_dir = root.join("real_dir");
        fs::create_dir(&real_dir).unwrap();
        fs::write(real_dir.join("file_in_real.txt"), "content").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_dir, root.join("link_dir")).unwrap();
        }

        let mut tree = FsTree::new(root.to_path_buf());

        #[cfg(unix)]
        {
            // The symlink should appear as a directory.
            let link_entry = tree
                .visible_entries()
                .iter()
                .find(|e| e.entry.name == "link_dir");
            assert!(link_entry.is_some(), "symlink should be visible");
            assert!(
                link_entry.unwrap().entry.is_dir,
                "symlink to dir should show as dir"
            );

            // Expanding the symlink should show the contents of real_dir.
            tree.toggle(&root.join("link_dir"));
            let has_inner = tree
                .visible_entries()
                .iter()
                .any(|e| e.entry.name == "file_in_real.txt");
            assert!(has_inner, "expanding symlink dir should show inner files");
        }
    }
}
