// Syntax highlighting via syntect.

use std::path::Path;

use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::easy::HighlightLines;

use tide_core::{Color, TextStyle};

/// A styled span of text produced by syntax highlighting.
pub struct StyledSpan {
    pub text: String,
    pub style: TextStyle,
}

pub struct Highlighter {
    syntax_set: SyntaxSet,
    theme: Theme,
}

impl Highlighter {
    pub fn new() -> Self {
        let syntax_set = SyntaxSet::load_defaults_newlines();
        let theme_set = ThemeSet::load_defaults();
        let theme = theme_set.themes["base16-ocean.dark"].clone();
        Self { syntax_set, theme }
    }

    /// Detect syntax from file extension. Returns None if unknown.
    pub fn detect_syntax(&self, path: &Path) -> Option<&SyntaxReference> {
        let ext = path.extension()?.to_str()?;
        self.syntax_set.find_syntax_by_extension(ext)
    }

    /// Highlight a range of lines. Only processes the visible viewport for performance.
    pub fn highlight_lines(
        &self,
        lines: &[String],
        syntax: &SyntaxReference,
        start_line: usize,
        count: usize,
    ) -> Vec<Vec<StyledSpan>> {
        let mut h = HighlightLines::new(syntax, &self.theme);
        let mut result = Vec::with_capacity(count);

        // Process lines from start to build up highlighter state
        for (i, line) in lines.iter().enumerate() {
            let line_with_newline = format!("{}\n", line);
            let regions = h.highlight_line(&line_with_newline, &self.syntax_set);
            match regions {
                Ok(regions) => {
                    if i >= start_line && i < start_line + count {
                        let spans: Vec<StyledSpan> = regions
                            .into_iter()
                            .map(|(style, text)| {
                                let fg = Color::new(
                                    style.foreground.r as f32 / 255.0,
                                    style.foreground.g as f32 / 255.0,
                                    style.foreground.b as f32 / 255.0,
                                    style.foreground.a as f32 / 255.0,
                                );
                                let bg = if style.background.a > 0
                                    && (style.background.r, style.background.g, style.background.b)
                                        != (0, 0, 0)
                                {
                                    Some(Color::new(
                                        style.background.r as f32 / 255.0,
                                        style.background.g as f32 / 255.0,
                                        style.background.b as f32 / 255.0,
                                        style.background.a as f32 / 255.0,
                                    ))
                                } else {
                                    None
                                };
                                StyledSpan {
                                    text: text.trim_end_matches('\n').to_string(),
                                    style: TextStyle {
                                        foreground: fg,
                                        background: bg,
                                        bold: style
                                            .font_style
                                            .contains(syntect::highlighting::FontStyle::BOLD),
                                        italic: style
                                            .font_style
                                            .contains(syntect::highlighting::FontStyle::ITALIC),
                                        underline: style
                                            .font_style
                                            .contains(syntect::highlighting::FontStyle::UNDERLINE),
                                    },
                                }
                            })
                            .collect();
                        result.push(spans);
                    }
                }
                Err(_) => {
                    if i >= start_line && i < start_line + count {
                        result.push(Vec::new());
                    }
                }
            }

            if i >= start_line + count {
                break;
            }
        }

        result
    }

    pub fn syntax_set(&self) -> &SyntaxSet {
        &self.syntax_set
    }

    /// Get the plain text syntax (fallback when no syntax detected).
    pub fn plain_text_syntax(&self) -> &SyntaxReference {
        self.syntax_set.find_syntax_plain_text()
    }
}
