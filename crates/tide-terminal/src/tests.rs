#[cfg(test)]
mod tests {
    use crate::*;
    use tide_core::{Key, Modifiers};

    #[test]
    fn test_key_to_bytes_char() {
        let bytes = Terminal::key_to_bytes(&Key::Char('a'), &Modifiers::default());
        assert_eq!(bytes, vec![b'a']);
    }

    #[test]
    fn test_key_to_bytes_ctrl_c() {
        let mods = Modifiers {
            ctrl: true,
            ..Default::default()
        };
        let bytes = Terminal::key_to_bytes(&Key::Char('c'), &mods);
        assert_eq!(bytes, vec![3]); // ETX
    }

    #[test]
    fn test_key_to_bytes_enter() {
        let bytes = Terminal::key_to_bytes(&Key::Enter, &Modifiers::default());
        assert_eq!(bytes, vec![0x0d]);
    }

    #[test]
    fn test_key_to_bytes_escape() {
        let bytes = Terminal::key_to_bytes(&Key::Escape, &Modifiers::default());
        assert_eq!(bytes, vec![0x1b]);
    }

    #[test]
    fn test_key_to_bytes_arrow_up() {
        let bytes = Terminal::key_to_bytes(&Key::Up, &Modifiers::default());
        assert_eq!(bytes, vec![0x1b, b'[', b'A']);
    }

    #[test]
    fn test_key_to_bytes_f1() {
        let bytes = Terminal::key_to_bytes(&Key::F(1), &Modifiers::default());
        assert_eq!(bytes, vec![0x1b, b'O', b'P']);
    }

    #[test]
    fn test_named_color_to_rgb() {
        let color = Terminal::named_color_to_rgb(true, NamedColor::Red);
        assert_eq!(color, Color::rgb(1.0, 0.33, 0.33));
    }

    #[test]
    fn test_indexed_color_fallback_grayscale() {
        let color = Terminal::indexed_color_fallback(232);
        // 232 = first grayscale entry: (8 + 10*0) / 255
        let expected = 8.0 / 255.0;
        assert!((color.r - expected).abs() < 0.001);
    }

    #[test]
    fn test_build_empty_grid() {
        let grid = Terminal::build_empty_grid(80, 24);
        assert_eq!(grid.cols, 80);
        assert_eq!(grid.rows, 24);
        assert_eq!(grid.cells.len(), 24);
        assert_eq!(grid.cells[0].len(), 80);
        assert_eq!(grid.cells[0][0].character, ' ');
    }

    #[test]
    fn test_trim_url_trailing_paren() {
        // Unbalanced closing paren should be trimmed
        assert_eq!(crate::trim_url_trailing("https://example.com/page)"), "https://example.com/page");
        // Balanced parens (Wikipedia-style) should be preserved
        assert_eq!(crate::trim_url_trailing("https://en.wikipedia.org/wiki/Foo_(bar)"), "https://en.wikipedia.org/wiki/Foo_(bar)");
        // Trailing punctuation
        assert_eq!(crate::trim_url_trailing("https://example.com/page."), "https://example.com/page");
        assert_eq!(crate::trim_url_trailing("https://example.com/page,"), "https://example.com/page");
        assert_eq!(crate::trim_url_trailing("https://example.com/page;"), "https://example.com/page");
        // Combined: paren + punctuation
        assert_eq!(crate::trim_url_trailing("https://example.com/page)."), "https://example.com/page");
        // No trimming needed
        assert_eq!(crate::trim_url_trailing("https://example.com/page"), "https://example.com/page");
    }
}
