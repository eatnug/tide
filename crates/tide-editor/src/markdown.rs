// Markdown-to-styled-lines renderer for preview mode.

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use pulldown_cmark::Alignment;
use tide_core::{Color, TextStyle};

use crate::highlight::StyledSpan;

/// Color palette for markdown preview rendering.
pub struct MarkdownTheme {
    pub body: Color,
    pub h1: Color,
    pub h2: Color,
    pub h3: Color,
    pub h4: Color,
    pub bold: Color,
    pub italic: Color,
    pub code_fg: Color,
    pub code_bg: Color,
    pub code_block_bg: Color,
    pub link: Color,
    pub blockquote: Color,
    pub rule: Color,
    pub list_marker: Color,
}

impl MarkdownTheme {
    pub fn dark() -> Self {
        Self {
            body: Color::new(0.85, 0.85, 0.85, 1.0),
            h1: Color::new(0.55, 0.75, 1.0, 1.0),
            h2: Color::new(0.55, 0.85, 0.65, 1.0),
            h3: Color::new(0.95, 0.75, 0.45, 1.0),
            h4: Color::new(0.80, 0.65, 0.90, 1.0),
            bold: Color::new(0.95, 0.95, 0.95, 1.0),
            italic: Color::new(0.78, 0.78, 0.78, 1.0),
            code_fg: Color::new(0.90, 0.70, 0.50, 1.0),
            code_bg: Color::new(1.0, 1.0, 1.0, 0.06),
            code_block_bg: Color::new(1.0, 1.0, 1.0, 0.04),
            link: Color::new(0.45, 0.65, 1.0, 1.0),
            blockquote: Color::new(0.55, 0.55, 0.55, 1.0),
            rule: Color::new(0.35, 0.35, 0.35, 1.0),
            list_marker: Color::new(0.55, 0.75, 1.0, 1.0),
        }
    }

    pub fn light() -> Self {
        Self {
            body: Color::new(0.15, 0.15, 0.15, 1.0),
            h1: Color::new(0.10, 0.35, 0.70, 1.0),
            h2: Color::new(0.10, 0.50, 0.25, 1.0),
            h3: Color::new(0.60, 0.40, 0.10, 1.0),
            h4: Color::new(0.45, 0.25, 0.60, 1.0),
            bold: Color::new(0.05, 0.05, 0.05, 1.0),
            italic: Color::new(0.25, 0.25, 0.25, 1.0),
            code_fg: Color::new(0.60, 0.30, 0.10, 1.0),
            code_bg: Color::new(0.0, 0.0, 0.0, 0.06),
            code_block_bg: Color::new(0.0, 0.0, 0.0, 0.04),
            link: Color::new(0.15, 0.35, 0.80, 1.0),
            blockquote: Color::new(0.45, 0.45, 0.45, 1.0),
            rule: Color::new(0.70, 0.70, 0.70, 1.0),
            list_marker: Color::new(0.10, 0.35, 0.70, 1.0),
        }
    }
}

/// A single preview line with styled spans and optional full-row background.
pub struct PreviewLine {
    pub spans: Vec<StyledSpan>,
    pub bg_color: Option<Color>,
}

/// Wrap text into lines that fit within `max_width` display columns.
fn wrap_cell_text(text: &str, max_width: usize) -> Vec<String> {
    use unicode_width::UnicodeWidthChar;
    if max_width == 0 {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0usize;
    for ch in text.chars() {
        let cw = ch.width().unwrap_or(1);
        if current_width + cw > max_width && current_width > 0 {
            lines.push(current);
            current = String::new();
            current_width = 0;
        }
        current.push(ch);
        current_width += cw;
    }
    lines.push(current);
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

/// Render a collected table into preview lines.
fn render_table(
    rows: &[Vec<String>],
    _alignments: &[Alignment],
    header_count: usize,
    theme: &MarkdownTheme,
    indent: usize,
    effective_width: usize,
    result: &mut Vec<PreviewLine>,
) {
    if rows.is_empty() {
        return;
    }
    let num_cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    if num_cols == 0 {
        return;
    }

    // Calculate column widths (minimum 3 for readability)
    let mut col_widths: Vec<usize> = vec![3; num_cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < num_cols {
                col_widths[i] = col_widths[i].max(cell.width());
            }
        }
    }

    // Clamp total width to effective_width
    let border_overhead = num_cols + 1; // one │ per column + closing │
    let padding_overhead = num_cols * 2; // 1 space on each side of each cell
    let total_content_width: usize = col_widths.iter().sum();
    let total_width = total_content_width + border_overhead + padding_overhead;
    if total_width > effective_width && total_content_width > 0 {
        let available = effective_width.saturating_sub(border_overhead + padding_overhead);
        let scale = available as f64 / total_content_width as f64;
        for w in &mut col_widths {
            *w = ((*w as f64 * scale).floor() as usize).max(1);
        }
    }

    let border_style = TextStyle {
        foreground: theme.blockquote,
        background: None,
        bold: false, dim: false, italic: false, underline: false,
    };
    let header_style = TextStyle {
        foreground: theme.bold,
        background: None,
        bold: true, dim: false, italic: false, underline: false,
    };
    let cell_style = TextStyle {
        foreground: theme.body,
        background: None,
        bold: false, dim: false, italic: false, underline: false,
    };
    let indent_style = TextStyle {
        foreground: theme.body,
        background: None,
        bold: false, dim: false, italic: false, underline: false,
    };

    // Helper: build a horizontal rule line
    let make_rule = |left: &str, mid: &str, right: &str, fill: &str| -> PreviewLine {
        let mut text = String::new();
        text.push_str(left);
        for (i, w) in col_widths.iter().enumerate() {
            text.push_str(&fill.repeat(*w + 2)); // +2 for padding
            if i + 1 < num_cols {
                text.push_str(mid);
            }
        }
        text.push_str(right);
        PreviewLine {
            spans: vec![
                StyledSpan { text: " ".repeat(indent), style: indent_style },
                StyledSpan { text, style: border_style },
            ],
            bg_color: None,
        }
    };

    // Helper: build one visual line of a (possibly multi-line) row
    let make_visual_line = |wrapped_cells: &[Vec<String>], line_idx: usize, is_header: bool| -> PreviewLine {
        let style = if is_header { header_style } else { cell_style };
        let mut spans = vec![
            StyledSpan { text: " ".repeat(indent), style: indent_style },
            StyledSpan { text: "\u{2502}".to_string(), style: border_style },
        ];
        for (i, w) in col_widths.iter().enumerate() {
            let line_text = wrapped_cells.get(i)
                .and_then(|lines| lines.get(line_idx))
                .map(|s| s.as_str())
                .unwrap_or("");
            let line_w = line_text.width();
            let pad_right = w.saturating_sub(line_w);
            let padded = format!(" {}{} ", line_text, " ".repeat(pad_right));
            spans.push(StyledSpan { text: padded, style });
            spans.push(StyledSpan { text: "\u{2502}".to_string(), style: border_style });
        }
        PreviewLine { spans, bg_color: None }
    };

    // Top border ┌───┬───┐
    result.push(make_rule("\u{250C}", "\u{252C}", "\u{2510}", "\u{2500}"));

    for (ri, row) in rows.iter().enumerate() {
        // Row separator ├───┼───┤ (between every row, including between data rows)
        if ri > 0 {
            result.push(make_rule("\u{251C}", "\u{253C}", "\u{2524}", "\u{2500}"));
        }

        // Wrap each cell to fit its column width
        let wrapped_cells: Vec<Vec<String>> = (0..num_cols).map(|i| {
            let cell_text = row.get(i).map(|s| s.as_str()).unwrap_or("");
            wrap_cell_text(cell_text, col_widths[i])
        }).collect();
        let row_height = wrapped_cells.iter().map(|lines| lines.len()).max().unwrap_or(1);
        let is_header = ri < header_count;

        for line_idx in 0..row_height {
            result.push(make_visual_line(&wrapped_cells, line_idx, is_header));
        }
    }

    // Bottom border └───┴───┘
    result.push(make_rule("\u{2514}", "\u{2534}", "\u{2518}", "\u{2500}"));
}

/// Render markdown content into styled preview lines with word wrapping.
pub fn render_markdown_preview(
    lines: &[String],
    theme: &MarkdownTheme,
    wrap_width: usize,
) -> Vec<PreviewLine> {
    use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, HeadingLevel, CodeBlockKind};

    let source: String = lines.join("\n");
    let opts = Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES;
    let parser = Parser::new_ext(&source, opts);

    let mut result: Vec<PreviewLine> = Vec::new();

    // Style stack for nested formatting
    let mut bold = false;
    let mut italic = false;
    let mut in_link = false;
    let mut in_code_block = false;
    let mut heading_level: Option<HeadingLevel> = None;
    let mut in_blockquote = false;
    let mut list_depth: usize = 0;
    let mut ordered_counters: Vec<u64> = Vec::new();
    let mut pending_list_marker: Option<String> = None;

    // Table state
    let mut in_table = false;
    let mut table_alignments: Vec<Alignment> = Vec::new();
    let mut table_rows: Vec<Vec<String>> = Vec::new(); // rows of cells
    let mut table_current_row: Vec<String> = Vec::new();
    let mut table_cell_text = String::new();
    let mut in_table_cell = false;
    let mut table_header_rows: usize = 0;

    // Current line accumulator
    let mut current_spans: Vec<StyledSpan> = Vec::new();
    let mut current_col: usize = 0;
    let mut current_bg: Option<Color> = None;

    let indent = 2; // 2-cell left indent for all content
    let effective_width = wrap_width.saturating_sub(indent);

    let flush_line = |spans: &mut Vec<StyledSpan>, bg: &Option<Color>, out: &mut Vec<PreviewLine>, col: &mut usize| {
        // Add leading indent
        let mut line_spans = vec![StyledSpan {
            text: " ".repeat(indent),
            style: TextStyle {
                foreground: theme.body,
                background: None,
                bold: false, dim: false, italic: false, underline: false,
            },
        }];
        line_spans.append(spans);
        out.push(PreviewLine {
            spans: line_spans,
            bg_color: *bg,
        });
        *col = 0;
    };

    let push_empty_line = |out: &mut Vec<PreviewLine>| {
        out.push(PreviewLine {
            spans: vec![],
            bg_color: None,
        });
    };

    let style_for = |theme: &MarkdownTheme, heading: &Option<HeadingLevel>, bold: bool, italic: bool, in_link: bool, in_code_block: bool, in_blockquote: bool| -> TextStyle {
        if in_code_block {
            return TextStyle {
                foreground: theme.code_fg,
                background: None,
                bold: false, dim: false, italic: false, underline: false,
            };
        }
        if in_link {
            return TextStyle {
                foreground: theme.link,
                background: None,
                bold: false, dim: false, italic: false, underline: true,
            };
        }
        if let Some(level) = heading {
            let color = match level {
                HeadingLevel::H1 => theme.h1,
                HeadingLevel::H2 => theme.h2,
                HeadingLevel::H3 => theme.h3,
                _ => theme.h4,
            };
            return TextStyle {
                foreground: color,
                background: None,
                bold: true, dim: false,
                italic: matches!(level, HeadingLevel::H4 | HeadingLevel::H5 | HeadingLevel::H6),
                underline: false,
            };
        }
        if in_blockquote {
            return TextStyle {
                foreground: theme.blockquote,
                background: None,
                bold, dim: false, italic: true, underline: false,
            };
        }
        if bold && italic {
            return TextStyle {
                foreground: theme.bold,
                background: None,
                bold: true, dim: false, italic: true, underline: false,
            };
        }
        if bold {
            return TextStyle {
                foreground: theme.bold,
                background: None,
                bold: true, dim: false, italic: false, underline: false,
            };
        }
        if italic {
            return TextStyle {
                foreground: theme.italic,
                background: None,
                bold: false, dim: false, italic: true, underline: false,
            };
        }
        TextStyle {
            foreground: theme.body,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        }
    };

    for event in parser {
        // When inside a table, intercept events to collect cell text
        if in_table {
            match event {
                Event::Start(Tag::TableHead) => {}
                Event::End(TagEnd::TableHead) => {
                    table_header_rows = table_rows.len();
                }
                Event::Start(Tag::TableRow) => {
                    table_current_row.clear();
                }
                Event::End(TagEnd::TableRow) => {
                    table_rows.push(table_current_row.clone());
                    table_current_row.clear();
                }
                Event::Start(Tag::TableCell) => {
                    in_table_cell = true;
                    table_cell_text.clear();
                }
                Event::End(TagEnd::TableCell) => {
                    in_table_cell = false;
                    table_current_row.push(table_cell_text.clone());
                    table_cell_text.clear();
                }
                Event::Text(ref text) if in_table_cell => {
                    table_cell_text.push_str(text);
                }
                Event::Code(ref code) if in_table_cell => {
                    table_cell_text.push_str(code);
                }
                Event::End(TagEnd::Table) => {
                    // Render the collected table
                    let header_count = table_header_rows;
                    if !result.is_empty() {
                        push_empty_line(&mut result);
                    }
                    render_table(&table_rows, &table_alignments, header_count, theme, indent, effective_width, &mut result);
                    push_empty_line(&mut result);
                    in_table = false;
                    table_rows.clear();
                    table_alignments.clear();
                    table_current_row.clear();
                    table_header_rows = 0;
                }
                _ => {
                    // Capture any other text-like events inside cells
                    if in_table_cell {
                        if let Event::SoftBreak = event {
                            table_cell_text.push(' ');
                        }
                    }
                }
            }
            continue;
        }

        match event {
            Event::Start(Tag::Table(alignments)) => {
                in_table = true;
                table_alignments = alignments;
                table_rows.clear();
                table_current_row.clear();
                table_header_rows = 0;
            }
            Event::Start(Tag::Heading { level, .. }) => {
                heading_level = Some(level);
                // Add spacing before headings
                if !result.is_empty() {
                    push_empty_line(&mut result);
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                heading_level = None;
            }
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                push_empty_line(&mut result);
            }
            Event::Start(Tag::BlockQuote(_)) => {
                in_blockquote = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                if !current_spans.is_empty() {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
                in_blockquote = false;
                push_empty_line(&mut result);
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                in_code_block = true;
                current_bg = Some(theme.code_block_bg);
                // Show language label if available
                if let CodeBlockKind::Fenced(lang) = &kind {
                    let lang_str = lang.as_ref();
                    if !lang_str.is_empty() {
                        current_spans.push(StyledSpan {
                            text: format!(" {}", lang_str),
                            style: TextStyle {
                                foreground: theme.blockquote,
                                background: None,
                                bold: false, dim: true, italic: true, underline: false,
                            },
                        });
                        flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                    }
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if !current_spans.is_empty() {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
                current_bg = None;
                in_code_block = false;
                push_empty_line(&mut result);
            }
            Event::Start(Tag::List(start)) => {
                list_depth += 1;
                if let Some(n) = start {
                    ordered_counters.push(n);
                } else {
                    ordered_counters.push(0); // 0 = unordered
                }
            }
            Event::End(TagEnd::List(_)) => {
                if !current_spans.is_empty() {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
                list_depth = list_depth.saturating_sub(1);
                ordered_counters.pop();
                if list_depth == 0 {
                    push_empty_line(&mut result);
                }
            }
            Event::Start(Tag::Item) => {
                let list_indent = "  ".repeat(list_depth.saturating_sub(1));
                let marker = if let Some(counter) = ordered_counters.last_mut() {
                    if *counter > 0 {
                        let m = format!("{}{}. ", list_indent, counter);
                        *counter += 1;
                        m
                    } else {
                        format!("{}\u{2022} ", list_indent) // bullet
                    }
                } else {
                    format!("{}\u{2022} ", list_indent)
                };
                pending_list_marker = Some(marker);
            }
            Event::End(TagEnd::Item) => {
                if !current_spans.is_empty() {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
            }
            Event::Start(Tag::Emphasis) => {
                italic = true;
            }
            Event::End(TagEnd::Emphasis) => {
                italic = false;
            }
            Event::Start(Tag::Strong) => {
                bold = true;
            }
            Event::End(TagEnd::Strong) => {
                bold = false;
            }
            Event::Start(Tag::Link { .. }) => {
                in_link = true;
            }
            Event::End(TagEnd::Link) => {
                in_link = false;
            }
            Event::Start(Tag::Strikethrough) => {}
            Event::End(TagEnd::Strikethrough) => {}
            Event::Text(text) => {
                // Emit pending list marker before first text in a list item
                if let Some(marker) = pending_list_marker.take() {
                    current_spans.push(StyledSpan {
                        text: marker.clone(),
                        style: TextStyle {
                            foreground: theme.list_marker,
                            background: None,
                            bold: false, dim: false, italic: false, underline: false,
                        },
                    });
                    current_col += marker.width();
                }

                let style = style_for(theme, &heading_level, bold, italic, in_link, in_code_block, in_blockquote);

                if in_code_block {
                    // Code blocks: render line by line, no word wrapping.
                    // Each line from split gets its own output line.
                    let code_lines: Vec<&str> = text.split('\n').collect();
                    let last_idx = code_lines.len() - 1;
                    for (li, line) in code_lines.iter().enumerate() {
                        // Flush previous code line if there's content accumulated
                        if current_col > 0 {
                            flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                        }
                        if !line.is_empty() {
                            let padded = format!(" {}", line);
                            current_col += padded.width();
                            current_spans.push(StyledSpan {
                                text: padded,
                                style,
                            });
                        } else if !(li == last_idx && text.ends_with('\n')) {
                            // Empty line in code block — emit blank line with bg
                            result.push(PreviewLine {
                                spans: vec![StyledSpan {
                                    text: " ".repeat(indent),
                                    style: TextStyle {
                                        foreground: theme.body,
                                        background: None,
                                        bold: false, dim: false, italic: false, underline: false,
                                    },
                                }],
                                bg_color: current_bg,
                            });
                        }
                    }
                } else {
                    // Normal text: word wrap at effective_width
                    let blockquote_prefix = if in_blockquote { "\u{2502} " } else { "" };
                    let prefix_len = blockquote_prefix.width();

                    if current_col == 0 && !blockquote_prefix.is_empty() {
                        current_spans.push(StyledSpan {
                            text: blockquote_prefix.to_string(),
                            style: TextStyle {
                                foreground: theme.blockquote,
                                background: None,
                                bold: false, dim: false, italic: false, underline: false,
                            },
                        });
                        current_col += prefix_len;
                    }

                    for word in text.split_inclusive(char::is_whitespace) {
                        let word_len = word.width();
                        // If word fits after wrapping to a new line, do a simple word wrap
                        if current_col + word_len > effective_width && current_col > prefix_len && word_len <= effective_width {
                            flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                            if !blockquote_prefix.is_empty() {
                                current_spans.push(StyledSpan {
                                    text: blockquote_prefix.to_string(),
                                    style: TextStyle {
                                        foreground: theme.blockquote,
                                        background: None,
                                        bold: false, dim: false, italic: false, underline: false,
                                    },
                                });
                                current_col += prefix_len;
                            }
                            current_spans.push(StyledSpan {
                                text: word.to_string(),
                                style,
                            });
                            current_col += word_len;
                        } else if current_col + word_len > effective_width {
                            // Word is too wide even on its own line — break character by character
                            let mut char_buf = String::new();
                            for ch in word.chars() {
                                let ch_w = ch.width().unwrap_or(1);
                                if current_col + ch_w > effective_width && current_col > prefix_len {
                                    // Flush accumulated chars
                                    if !char_buf.is_empty() {
                                        current_spans.push(StyledSpan { text: char_buf.clone(), style });
                                        char_buf.clear();
                                    }
                                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                                    if !blockquote_prefix.is_empty() {
                                        current_spans.push(StyledSpan {
                                            text: blockquote_prefix.to_string(),
                                            style: TextStyle {
                                                foreground: theme.blockquote,
                                                background: None,
                                                bold: false, dim: false, italic: false, underline: false,
                                            },
                                        });
                                        current_col += prefix_len;
                                    }
                                }
                                char_buf.push(ch);
                                current_col += ch_w;
                            }
                            if !char_buf.is_empty() {
                                current_spans.push(StyledSpan { text: char_buf, style });
                            }
                        } else {
                            // Word fits on current line
                            current_spans.push(StyledSpan {
                                text: word.to_string(),
                                style,
                            });
                            current_col += word_len;
                        }
                    }
                }
            }
            Event::Code(code) => {
                // Inline code: `code`
                if let Some(marker) = pending_list_marker.take() {
                    current_spans.push(StyledSpan {
                        text: marker.clone(),
                        style: TextStyle {
                            foreground: theme.list_marker,
                            background: None,
                            bold: false, dim: false, italic: false, underline: false,
                        },
                    });
                    current_col += marker.width();
                }
                let code_text = format!(" {} ", code);
                let code_len = code_text.width();
                if current_col + code_len > effective_width && current_col > 0 {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
                current_spans.push(StyledSpan {
                    text: code_text,
                    style: TextStyle {
                        foreground: theme.code_fg,
                        background: Some(theme.code_bg),
                        bold: false, dim: false, italic: false, underline: false,
                    },
                });
                current_col += code_len;
            }
            Event::SoftBreak => {
                // Treat soft breaks as spaces (markdown paragraph continuation)
                current_spans.push(StyledSpan {
                    text: " ".to_string(),
                    style: style_for(theme, &heading_level, bold, italic, in_link, in_code_block, in_blockquote),
                });
                current_col += 1;
            }
            Event::HardBreak => {
                flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
            }
            Event::Rule => {
                if !current_spans.is_empty() {
                    flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
                }
                let rule_len = effective_width.min(60);
                result.push(PreviewLine {
                    spans: vec![
                        StyledSpan {
                            text: " ".repeat(indent),
                            style: TextStyle {
                                foreground: theme.body,
                                background: None,
                                bold: false, dim: false, italic: false, underline: false,
                            },
                        },
                        StyledSpan {
                            text: "\u{2500}".repeat(rule_len),
                            style: TextStyle {
                                foreground: theme.rule,
                                background: None,
                                bold: false, dim: false, italic: false, underline: false,
                            },
                        },
                    ],
                    bg_color: None,
                });
                push_empty_line(&mut result);
            }
            // Ignore other events (HTML, footnotes, etc.)
            _ => {}
        }
    }

    // Flush any remaining content
    if !current_spans.is_empty() {
        flush_line(&mut current_spans, &current_bg, &mut result, &mut current_col);
    }

    result
}
