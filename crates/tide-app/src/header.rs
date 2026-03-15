// Per-pane header rendering: title + close button + kind-specific badges.
// When a TabGroup has multiple tabs, renders a tab bar instead of single-pane header.

use std::collections::HashMap;

use tide_core::{PaneId, Rect, Renderer, TextStyle, Vec2};
use tide_layout::TabGroup;
use tide_renderer::WgpuRenderer;

use crate::pane::PaneKind;
use crate::theme::*;

/// Clickable zone within a pane header.
#[derive(Debug, Clone)]
pub struct HeaderHitZone {
    pub pane_id: PaneId,
    pub rect: Rect,
    pub action: HeaderHitAction,
}

/// Action triggered by clicking a header hit zone.
#[derive(Debug, Clone, PartialEq)]
pub enum HeaderHitAction {
    Close,
    GitBranch,
    GitStatus,
    EditorCompare,
    EditorBack,
    EditorFileName,
    MarkdownPreview,
    DiffRefresh,
    Maximize,
    /// Click on a tab in a multi-tab header to switch to it.
    Tab(PaneId),
    /// Click the close button on a specific tab in a multi-tab header.
    TabClose(PaneId),
}

/// Badge specification for editor pane headers.
/// Computed by `editor_header_badges()` and consumed by both single-pane
/// and tab-bar rendering paths to ensure badge consistency.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EditorBadge {
    pub text: String,
    pub action: Option<HeaderHitAction>,
}

/// Compute which right-side badges an editor pane should display.
/// This is the single source of truth — both `render_pane_header` and
/// `render_tab_bar` use this, preventing badge divergence between paths.
pub(crate) fn editor_header_badges(ep: &crate::editor_pane::EditorPane) -> Vec<EditorBadge> {
    let mut badges = Vec::new();

    // Markdown preview toggle
    if ep.is_markdown() && !ep.diff_mode {
        let text = if ep.preview_mode { "edit" } else { "preview" };
        badges.push(EditorBadge {
            text: text.to_string(),
            action: Some(HeaderHitAction::MarkdownPreview),
        });
    }

    // Diff mode back button
    if ep.diff_mode {
        badges.push(EditorBadge {
            text: "back".to_string(),
            action: Some(HeaderHitAction::EditorBack),
        });
    } else if ep.disk_changed && ep.editor.is_modified() && !ep.file_deleted {
        // Conflict: compare button + label
        badges.push(EditorBadge {
            text: "compare".to_string(),
            action: Some(HeaderHitAction::EditorCompare),
        });
        badges.push(EditorBadge {
            text: "conflict".to_string(),
            action: None,
        });
    }

    // Deleted badge
    if ep.file_deleted {
        badges.push(EditorBadge {
            text: "deleted".to_string(),
            action: None,
        });
    }

    badges
}

/// Render the header for a pane (or tab bar for multi-tab groups).
/// When `tab_group` has more than 1 tab, renders a tab bar.
/// Otherwise renders the single-pane header as before.
/// Returns hit zones for click handling.
pub fn render_pane_header(
    id: PaneId,
    rect: Rect,
    panes: &HashMap<PaneId, PaneKind>,
    focused: Option<PaneId>,
    tab_group: Option<&TabGroup>,
    is_zoomed: bool,
    p: &ThemePalette,
    renderer: &mut WgpuRenderer,
) -> Vec<HeaderHitZone> {
    // Always render tab bar style (consistent look whether 1 tab or many)
    if let Some(tg) = tab_group {
        return render_tab_bar(tg, rect, panes, focused, is_zoomed, p, renderer);
    }

    let mut zones = Vec::new();
    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let is_focused = focused == Some(id);

    let text_y = rect.y + (TAB_BAR_HEIGHT - cell_height) / 2.0;

    // Align content to header padding (matches Tide.pen padding: [0, 12])
    let content_left = rect.x + PANE_PADDING;
    let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_size.width).floor();
    let content_right = rect.x + PANE_PADDING + grid_cols * cell_size.width;

    // Badge colors based on focus state
    let badge_bg = if is_focused { p.badge_bg } else { p.badge_bg_unfocused };

    // Close button as bare icon (no badge background)
    let is_modified = match panes.get(&id) {
        Some(PaneKind::Editor(ep)) => ep.editor.is_modified(),
        _ => false,
    };
    let (close_icon_str, close_color) = if is_modified {
        ("\u{f111}", p.editor_modified) // filled circle
    } else {
        ("\u{f00d}", p.close_icon) // x icon with close_icon color
    };
    let close_w = cell_size.width + BADGE_PADDING_H * 2.0;
    let close_x = content_right - close_w;
    {
        let close_style = TextStyle {
            foreground: close_color,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };
        renderer.draw_chrome_text(
            close_icon_str,
            Vec2::new(close_x + BADGE_PADDING_H, text_y),
            close_style,
            Rect::new(close_x, text_y - 1.0, close_w, cell_height + 2.0),
        );
    }
    zones.push(HeaderHitZone {
        pane_id: id,
        rect: Rect::new(close_x, rect.y, close_w, TAB_BAR_HEIGHT),
        action: HeaderHitAction::Close,
    });

    // Maximize button (expand icon, left of close)
    let max_w = cell_size.width + BADGE_PADDING_H * 2.0;
    let max_x = close_x - BADGE_GAP - max_w;
    {
        let max_style = TextStyle {
            foreground: p.close_icon,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };
        renderer.draw_chrome_text(
            "\u{f065}", // expand icon
            Vec2::new(max_x + BADGE_PADDING_H, text_y),
            max_style,
            Rect::new(max_x, text_y - 1.0, max_w, cell_height + 2.0),
        );
    }
    zones.push(HeaderHitZone {
        pane_id: id,
        rect: Rect::new(max_x, rect.y, max_w, TAB_BAR_HEIGHT),
        action: HeaderHitAction::Maximize,
    });
    let mut badge_right = max_x - BADGE_GAP;
    let available_w = content_right - content_left;
    if available_w < 20.0 {
        return zones;
    }

    // Determine title and badges based on pane kind
    match panes.get(&id) {
        Some(PaneKind::Terminal(pane)) => {
            // Dead process badge
            if pane.context.child_dead {
                let dead_text = "exited";
                let dead_w = dead_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                let dead_x = badge_right - dead_w;
                if dead_x > content_left + 40.0 {
                    render_badge_colored(renderer, dead_x, text_y, dead_w, cell_height, dead_text, p.badge_deleted, badge_bg, BADGE_RADIUS);
                    badge_right = dead_x - BADGE_GAP;
                }
            }

            // Git status badge — green tinted, focused pane only (per Tide.pen)
            if is_focused {
                if let Some(ref git) = pane.context.git_info {
                    if git.status.changed_files > 0 {
                        let stat_text = format!(
                            "{} +{} -{}",
                            git.status.changed_files, git.status.additions, git.status.deletions
                        );
                        let stat_color = p.git_added;
                        let stat_bg = tide_core::Color::new(p.git_added.r, p.git_added.g, p.git_added.b, 0.094);
                        let badge_w = stat_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                        let badge_x = badge_right - badge_w;
                        if badge_x > content_left + 60.0 {
                            render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &stat_text, stat_color, stat_bg, BADGE_RADIUS);
                            zones.push(HeaderHitZone {
                                pane_id: id,
                                rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                                action: HeaderHitAction::GitStatus,
                            });
                            badge_right = badge_x - BADGE_GAP;
                        }
                    }
                }
            }

            // Combined git branch + worktree badge (single badge, popup handles switching)
            if let Some(ref git) = pane.context.git_info {
                let branch_display = if pane.context.worktree_count >= 2 {
                    format!("\u{e0a0} {}", git.branch)
                } else {
                    format!("\u{e0a0} {}", git.branch)
                };
                let branch_color = if is_focused { p.badge_git_branch } else { p.tab_text };
                let badge_w = branch_display.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                let badge_x = badge_right - badge_w;
                if badge_x > content_left + 60.0 {
                    render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &branch_display, branch_color, badge_bg, BADGE_RADIUS);
                    zones.push(HeaderHitZone {
                        pane_id: id,
                        rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                        action: HeaderHitAction::GitBranch,
                    });
                    badge_right = badge_x - BADGE_GAP;
                }
            }

            // Title: plain text label (not a badge)
            let title = if let Some(ref cwd) = pane.context.cwd {
                let dir_name = cwd.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| cwd.display().to_string());
                dir_name
            } else {
                format!("Terminal {}", id)
            };
            let title_text_color = if !pane.context.shell_idle {
                p.badge_text_dimmed
            } else if is_focused {
                p.tab_text_focused
            } else {
                p.tab_text
            };
            let title_style = TextStyle {
                foreground: title_text_color,
                background: None,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
            };
            let title_w = ((title.chars().count() as f32 + 1.0) * cell_size.width)
                .min(badge_right - content_left);
            if title_w > 20.0 {
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(content_left, text_y),
                    title_style,
                    Rect::new(content_left, rect.y, title_w, TAB_BAR_HEIGHT),
                );
            }
        }
        Some(PaneKind::Editor(ep)) => {
            // Right-side badges from shared logic
            for badge in editor_header_badges(ep) {
                let badge_w = badge.text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                let badge_x = badge_right - badge_w;
                let min_x = if badge.text == "compare" { content_left + 60.0 } else { content_left + 40.0 };
                if badge_x > min_x {
                    let (text_color, bg) = match badge.action {
                        Some(HeaderHitAction::EditorBack) | Some(HeaderHitAction::EditorCompare) => {
                            (p.badge_text, p.conflict_bar_btn)
                        }
                        None if badge.text == "deleted" => (p.badge_deleted, badge_bg),
                        None if badge.text == "conflict" => (p.badge_conflict, badge_bg),
                        _ => {
                            let c = if is_focused { p.badge_text } else { p.tab_text };
                            (c, badge_bg)
                        }
                    };
                    render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &badge.text, text_color, bg, BADGE_RADIUS);
                    if let Some(action) = badge.action {
                        zones.push(HeaderHitZone {
                            pane_id: id,
                            rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                            action,
                        });
                    }
                    badge_right = badge_x - BADGE_GAP;
                }
            }

            // Title badge: file icon + name (clickable for save-as on untitled)
            let file_name = ep.title();
            let icon = crate::ui::file_icon(&file_name, false, false);
            let title = format!("{} {}", icon, file_name);
            let title_color = if is_focused { p.badge_text } else { p.tab_text };
            let title_w = (title.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0)
                .min(badge_right - content_left);
            if title_w > 20.0 {
                render_badge_colored(renderer, content_left, text_y, title_w, cell_height, &title, title_color, badge_bg, BADGE_RADIUS);
                zones.push(HeaderHitZone {
                    pane_id: id,
                    rect: Rect::new(content_left, rect.y, title_w, TAB_BAR_HEIGHT),
                    action: HeaderHitAction::EditorFileName,
                });
            }
        }
        Some(PaneKind::Browser(_bp)) => {
            // Browser panes render their own header via the nav bar; no header badges needed.
            let title = "Browser";
            let title_color = if is_focused { p.tab_text_focused } else { p.tab_text };
            let title_w = (title.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0)
                .min(badge_right - content_left);
            if title_w > 20.0 {
                render_badge_colored(renderer, content_left, text_y, title_w, cell_height, title, title_color, badge_bg, BADGE_RADIUS);
            }
        }
        Some(PaneKind::Diff(dp)) => {
            let diff_text_color = if is_focused { p.badge_text } else { p.tab_text };
            // Refresh badge
            let refresh_text = "\u{f021}"; // refresh icon
            let refresh_w = refresh_text.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
            let refresh_x = badge_right - refresh_w;
            if refresh_x > content_left + 60.0 {
                render_badge_colored(renderer, refresh_x, text_y, refresh_w, cell_height, refresh_text, diff_text_color, badge_bg, BADGE_RADIUS);
                zones.push(HeaderHitZone {
                    pane_id: id,
                    rect: Rect::new(refresh_x, rect.y, refresh_w, TAB_BAR_HEIGHT),
                    action: HeaderHitAction::DiffRefresh,
                });
                badge_right = refresh_x - BADGE_GAP;
            }

            // Stats badge
            let (add, del) = dp.total_stats();
            if add > 0 || del > 0 {
                let stats_text = format!("+{} -{}", add, del);
                let stats_w = stats_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                let stats_x = badge_right - stats_w;
                if stats_x > content_left + 60.0 {
                    render_badge_colored(renderer, stats_x, text_y, stats_w, cell_height, &stats_text, diff_text_color, badge_bg, BADGE_RADIUS);
                    badge_right = stats_x - BADGE_GAP;
                }
            }

            // File count badge
            if !dp.files.is_empty() {
                let count_text = format!("{} files", dp.files.len());
                let count_w = count_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                let count_x = badge_right - count_w;
                if count_x > content_left + 40.0 {
                    render_badge_colored(renderer, count_x, text_y, count_w, cell_height, &count_text, diff_text_color, badge_bg, BADGE_RADIUS);
                    badge_right = count_x - BADGE_GAP;
                }
            }

            // Title badge
            let title = "Git Changes";
            let title_w = (title.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0)
                .min(badge_right - content_left);
            if title_w > 20.0 {
                render_badge_colored(renderer, content_left, text_y, title_w, cell_height, title, diff_text_color, badge_bg, BADGE_RADIUS);
            }
        }
        Some(PaneKind::Launcher(_)) => {
            let title = "New Tab";
            let title_color = if is_focused { p.tab_text_focused } else { p.tab_text };
            let title_w = (title.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0)
                .min(badge_right - content_left);
            if title_w > 20.0 {
                render_badge_colored(renderer, content_left, text_y, title_w, cell_height, title, title_color, badge_bg, BADGE_RADIUS);
            }
        }
        None => {}
    }

    zones
}

/// Render a tab bar for a TabGroup with multiple tabs.
/// Each tab shows: icon + name + close(x). Active tab has accent underline.
fn render_tab_bar(
    tg: &TabGroup,
    rect: Rect,
    panes: &HashMap<PaneId, PaneKind>,
    focused: Option<PaneId>,
    is_zoomed: bool,
    p: &ThemePalette,
    renderer: &mut WgpuRenderer,
) -> Vec<HeaderHitZone> {
    let mut zones = Vec::new();
    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let active_pane = tg.active_pane();
    let is_group_focused = focused == Some(active_pane);

    let text_y = rect.y + (TAB_BAR_HEIGHT - cell_height) / 2.0;
    let content_left = rect.x + PANE_PADDING;
    let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_size.width).floor();
    let content_right = rect.x + PANE_PADDING + grid_cols * cell_size.width;

    // Maximize/minimize button stays at rightmost position
    let max_icon = if is_zoomed { "\u{f066}" } else { "\u{f065}" }; // compress / expand
    let max_w = cell_size.width + BADGE_PADDING_H * 2.0;
    let max_x = content_right - max_w;
    {
        let max_style = TextStyle {
            foreground: if is_zoomed { p.badge_git_branch } else { p.close_icon },
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };
        renderer.draw_chrome_text(
            max_icon,
            Vec2::new(max_x + BADGE_PADDING_H, text_y),
            max_style,
            Rect::new(max_x, text_y - 1.0, max_w, cell_height + 2.0),
        );
    }
    zones.push(HeaderHitZone {
        pane_id: active_pane,
        rect: Rect::new(max_x, rect.y, max_w, TAB_BAR_HEIGHT),
        action: HeaderHitAction::Maximize,
    });

    // Render git badges for the active pane (right-aligned, left of maximize button)
    let badge_bg = if is_group_focused { p.badge_bg } else { p.badge_bg_unfocused };
    let mut badge_right = max_x - BADGE_GAP;

    if let Some(PaneKind::Terminal(pane)) = panes.get(&active_pane) {
        // Dead process badge
        if pane.context.child_dead {
            let dead_text = "exited";
            let dead_w = dead_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
            let dead_x = badge_right - dead_w;
            if dead_x > content_left + 40.0 {
                render_badge_colored(renderer, dead_x, text_y, dead_w, cell_height, dead_text, p.badge_deleted, badge_bg, BADGE_RADIUS);
                badge_right = dead_x - BADGE_GAP;
            }
        }

        // Git status badge (e.g. "3 +10 -2") — only when focused
        if is_group_focused {
            if let Some(ref git) = pane.context.git_info {
                if git.status.changed_files > 0 {
                    let stat_text = format!(
                        "{} +{} -{}",
                        git.status.changed_files, git.status.additions, git.status.deletions
                    );
                    let stat_color = p.git_added;
                    let stat_bg = tide_core::Color::new(p.git_added.r, p.git_added.g, p.git_added.b, 0.094);
                    let badge_w = stat_text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
                    let badge_x = badge_right - badge_w;
                    if badge_x > content_left + 60.0 {
                        render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &stat_text, stat_color, stat_bg, BADGE_RADIUS);
                        zones.push(HeaderHitZone {
                            pane_id: active_pane,
                            rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                            action: HeaderHitAction::GitStatus,
                        });
                        badge_right = badge_x - BADGE_GAP;
                    }
                }
            }
        }

        // Git branch badge
        if let Some(ref git) = pane.context.git_info {
            let branch_display = format!("\u{e0a0} {}", git.branch);
            let branch_color = if is_group_focused { p.badge_git_branch } else { p.tab_text };
            let badge_w = branch_display.chars().count() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
            let badge_x = badge_right - badge_w;
            if badge_x > content_left + 60.0 {
                render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &branch_display, branch_color, badge_bg, BADGE_RADIUS);
                zones.push(HeaderHitZone {
                    pane_id: active_pane,
                    rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                    action: HeaderHitAction::GitBranch,
                });
                badge_right = badge_x - BADGE_GAP;
            }
        }
    }

    // Editor badges for the active pane (shared logic with single-pane path)
    if let Some(PaneKind::Editor(ep)) = panes.get(&active_pane) {
        for badge in editor_header_badges(ep) {
            let badge_w = badge.text.len() as f32 * cell_size.width + BADGE_PADDING_H * 2.0;
            let badge_x = badge_right - badge_w;
            let min_x = if badge.text == "compare" { content_left + 60.0 } else { content_left + 40.0 };
            if badge_x > min_x {
                let (text_color, bg) = match badge.action {
                    Some(HeaderHitAction::EditorBack) | Some(HeaderHitAction::EditorCompare) => {
                        (p.badge_text, p.conflict_bar_btn)
                    }
                    None if badge.text == "deleted" => (p.badge_deleted, badge_bg),
                    None if badge.text == "conflict" => (p.badge_conflict, badge_bg),
                    _ => {
                        let c = if is_group_focused { p.badge_text } else { p.tab_text };
                        (c, badge_bg)
                    }
                };
                render_badge_colored(renderer, badge_x, text_y, badge_w, cell_height, &badge.text, text_color, bg, BADGE_RADIUS);
                if let Some(action) = badge.action {
                    zones.push(HeaderHitZone {
                        pane_id: active_pane,
                        rect: Rect::new(badge_x, rect.y, badge_w, TAB_BAR_HEIGHT),
                        action,
                    });
                }
                badge_right = badge_x - BADGE_GAP;
            }
        }
    }

    // Render tabs left-to-right with horizontal scrolling.
    // When tabs overflow, auto-scroll so the active tab is always visible.
    let tab_right_limit = badge_right;
    let available_w = tab_right_limit - content_left;

    // Maximum characters for a tab label (truncate with ellipsis beyond this).
    let max_name_chars: usize = 24;

    // Pre-compute all tab widths to determine total width and scroll offset.
    let tab_widths: Vec<f32> = tg.tabs.iter().map(|&tab_id| {
        let name = crate::ui::pane_title(panes, tab_id);
        let name_char_count = name.chars().count().min(max_name_chars);
        let tab_content_chars = 1 + 1 + name_char_count + 1 + 1; // icon + gap + name + gap + close
        BADGE_PADDING_H * 2.0 + tab_content_chars as f32 * cell_size.width + BADGE_GAP
    }).collect();
    let total_w: f32 = tab_widths.iter().sum();

    // Compute scroll offset to ensure active tab is visible
    let scroll_offset = if total_w <= available_w {
        0.0
    } else {
        let active_start: f32 = tab_widths[..tg.active].iter().sum();
        let active_end = active_start + tab_widths[tg.active];
        // Scroll so active tab is fully visible, preferring minimal scroll
        let mut offset = 0.0;
        if active_end > available_w {
            offset = active_end - available_w;
        }
        if active_start < offset {
            offset = active_start;
        }
        offset
    };

    let mut tab_x = content_left - scroll_offset;

    for (i, &tab_id) in tg.tabs.iter().enumerate() {
        let is_active = i == tg.active;

        // Compute tab label: icon + name (truncate long names with ellipsis)
        let full_name = crate::ui::pane_title(panes, tab_id);
        let icon = tab_icon(panes, tab_id);
        let name = if full_name.chars().count() > max_name_chars {
            let truncated: String = full_name.chars().take(max_name_chars - 1).collect();
            format!("{}\u{2026}", truncated) // append "…"
        } else {
            full_name
        };
        // Tab content: icon(1) + space(1) + name + space(1) + close_icon(1)
        let name_char_count = name.chars().count();
        let tab_content_chars = 1 + 1 + name_char_count + 1 + 1; // icon + gap + name + gap + close
        let tab_w = BADGE_PADDING_H * 2.0 + tab_content_chars as f32 * cell_size.width;

        // Skip tabs entirely to the left of viewport
        if tab_x + tab_w + BADGE_GAP < content_left {
            tab_x += tab_w + BADGE_GAP;
            continue;
        }
        // Stop if tab starts past the right edge
        if tab_x > tab_right_limit {
            break;
        }

        // Text color based on active/inactive state
        let text_color = if is_active && is_group_focused {
            p.tab_text_focused
        } else if is_active {
            p.tab_text_focused
        } else {
            p.tab_text
        };

        let close_color = if is_active {
            p.close_icon
        } else {
            p.tab_text
        };

        let style = TextStyle {
            foreground: text_color,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };

        // Clip rect: intersect tab rect with the visible tab area
        let clip_left = tab_x.max(content_left);
        let clip_right = (tab_x + tab_w).min(tab_right_limit);
        let clip_w = (clip_right - clip_left).max(0.0);
        let clip = Rect::new(clip_left, rect.y, clip_w, TAB_BAR_HEIGHT);

        // Draw icon
        let icon_x = tab_x + BADGE_PADDING_H;
        renderer.draw_chrome_text(
            &icon,
            Vec2::new(icon_x, text_y),
            style,
            clip,
        );

        // Draw name
        let name_x = icon_x + 2.0 * cell_size.width; // icon + space
        renderer.draw_chrome_text(
            &name,
            Vec2::new(name_x, text_y),
            style,
            clip,
        );

        // Draw close icon (per-tab)
        let is_modified = match panes.get(&tab_id) {
            Some(PaneKind::Editor(ep)) => ep.editor.is_modified(),
            _ => false,
        };
        let (close_icon_str, close_icon_color) = if is_modified {
            ("\u{f111}", p.editor_modified) // filled circle for modified
        } else {
            ("\u{f00d}", close_color) // x icon
        };
        let close_icon_x = name_x + (name_char_count as f32 + 1.0) * cell_size.width;
        let close_style = TextStyle {
            foreground: close_icon_color,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };
        renderer.draw_chrome_text(
            close_icon_str,
            Vec2::new(close_icon_x, text_y),
            close_style,
            clip,
        );

        // Hit zone for the close icon on this tab
        let close_hit_w = cell_size.width + BADGE_PADDING_H;
        zones.push(HeaderHitZone {
            pane_id: tab_id,
            rect: Rect::new(close_icon_x - BADGE_PADDING_H / 2.0, rect.y, close_hit_w, TAB_BAR_HEIGHT),
            action: HeaderHitAction::TabClose(tab_id),
        });

        // Hit zone for the entire tab (for switching)
        zones.push(HeaderHitZone {
            pane_id: tab_id,
            rect: Rect::new(tab_x, rect.y, tab_w, TAB_BAR_HEIGHT),
            action: HeaderHitAction::Tab(tab_id),
        });

        // Active tab: draw bottom 2px accent bar
        if is_active {
            let accent_y = rect.y + TAB_BAR_HEIGHT - 2.0;
            renderer.draw_chrome_rect(
                Rect::new(tab_x, accent_y, tab_w, 2.0),
                p.dock_tab_underline,
            );
        }

        tab_x += tab_w + BADGE_GAP;
    }

    zones
}

/// Get the icon character for a pane (used in tab bar labels).
fn tab_icon(panes: &HashMap<PaneId, PaneKind>, id: PaneId) -> String {
    match panes.get(&id) {
        Some(PaneKind::Terminal(_)) => "\u{f120}".to_string(), // terminal icon
        Some(PaneKind::Editor(ep)) => {
            let name = ep.title();
            let icon = crate::ui::file_icon(&name, false, false);
            icon.to_string()
        }
        Some(PaneKind::Diff(_)) => "\u{f126}".to_string(), // code-fork icon
        Some(PaneKind::Browser(_)) => "\u{f0ac}".to_string(), // globe icon
        Some(PaneKind::Launcher(_)) => "+".to_string(), // plus icon for launcher
        None => "\u{f15b}".to_string(), // generic file icon
    }
}

/// Render a badge pill with custom background color.
pub(crate) fn render_badge_colored(
    renderer: &mut WgpuRenderer,
    x: f32,
    text_y: f32,
    width: f32,
    cell_height: f32,
    text: &str,
    text_color: tide_core::Color,
    bg_color: tide_core::Color,
    radius: f32,
) {
    let badge_y = text_y - 1.0;
    let badge_h = cell_height + 2.0;
    renderer.draw_chrome_rounded_rect(
        Rect::new(x, badge_y, width, badge_h),
        bg_color,
        radius,
    );
    let style = TextStyle {
        foreground: text_color,
        background: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
    };
    renderer.draw_chrome_text(
        text,
        Vec2::new(x + BADGE_PADDING_H, text_y),
        style,
        Rect::new(x, badge_y, width, badge_h),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tide_core::PaneId;
    use crate::editor_pane::EditorPane;

    fn make_editor(id: PaneId) -> EditorPane {
        EditorPane::new_empty(id)
    }

    fn make_markdown_editor(id: PaneId) -> EditorPane {
        let mut ep = make_editor(id);
        ep.editor.buffer.file_path = Some(PathBuf::from("README.md"));
        ep
    }

    #[test]
    fn plain_file_no_badges() {
        let ep = make_editor(1);
        let badges = editor_header_badges(&ep);
        assert!(badges.is_empty());
    }

    #[test]
    fn markdown_shows_preview_badge() {
        let ep = make_markdown_editor(1);
        let badges = editor_header_badges(&ep);
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "preview");
        assert_eq!(badges[0].action, Some(HeaderHitAction::MarkdownPreview));
    }

    #[test]
    fn markdown_preview_mode_shows_edit_badge() {
        let mut ep = make_markdown_editor(1);
        ep.preview_mode = true;
        let badges = editor_header_badges(&ep);
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "edit");
        assert_eq!(badges[0].action, Some(HeaderHitAction::MarkdownPreview));
    }

    #[test]
    fn markdown_diff_mode_shows_back_not_preview() {
        let mut ep = make_markdown_editor(1);
        ep.diff_mode = true;
        let badges = editor_header_badges(&ep);
        // diff_mode suppresses preview badge, shows back instead
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "back");
        assert_eq!(badges[0].action, Some(HeaderHitAction::EditorBack));
    }

    #[test]
    fn diff_mode_shows_back_badge() {
        let mut ep = make_editor(1);
        ep.diff_mode = true;
        let badges = editor_header_badges(&ep);
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "back");
        assert_eq!(badges[0].action, Some(HeaderHitAction::EditorBack));
    }

    #[test]
    fn conflict_shows_compare_and_label() {
        let mut ep = make_editor(1);
        ep.disk_changed = true;
        // Make the editor modified by inserting text
        ep.editor.handle_action(tide_editor::EditorActionKind::InsertChar('x'));
        assert!(ep.editor.is_modified());
        let badges = editor_header_badges(&ep);
        assert_eq!(badges.len(), 2);
        assert_eq!(badges[0].text, "compare");
        assert_eq!(badges[0].action, Some(HeaderHitAction::EditorCompare));
        assert_eq!(badges[1].text, "conflict");
        assert_eq!(badges[1].action, None);
    }

    #[test]
    fn file_deleted_shows_deleted_badge() {
        let mut ep = make_editor(1);
        ep.file_deleted = true;
        let badges = editor_header_badges(&ep);
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "deleted");
        assert_eq!(badges[0].action, None);
    }

    #[test]
    fn deleted_and_conflict_skips_conflict_when_deleted() {
        let mut ep = make_editor(1);
        ep.disk_changed = true;
        ep.file_deleted = true;
        ep.editor.handle_action(tide_editor::EditorActionKind::InsertChar('x'));
        let badges = editor_header_badges(&ep);
        // file_deleted suppresses the compare/conflict badges (condition: !ep.file_deleted)
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "deleted");
    }

    #[test]
    fn diff_mode_suppresses_conflict() {
        let mut ep = make_editor(1);
        ep.diff_mode = true;
        ep.disk_changed = true;
        ep.editor.handle_action(tide_editor::EditorActionKind::InsertChar('x'));
        let badges = editor_header_badges(&ep);
        // diff_mode takes priority over conflict
        assert_eq!(badges.len(), 1);
        assert_eq!(badges[0].text, "back");
    }

    #[test]
    fn markdown_extensions_all_recognized() {
        for ext in &["md", "markdown", "mdown", "mkd"] {
            let mut ep = make_editor(1);
            ep.editor.buffer.file_path = Some(PathBuf::from(format!("file.{}", ext)));
            let badges = editor_header_badges(&ep);
            assert!(!badges.is_empty(), "expected badge for .{} file", ext);
            assert_eq!(badges[0].text, "preview");
        }
    }
}

