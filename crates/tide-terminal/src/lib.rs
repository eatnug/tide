// Terminal backend implementation (Stream B)
// Implements tide_core::TerminalBackend using alacritty_terminal

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb as AnsiRgb};

use tide_core::{
    Color, CursorShape, CursorState, Key, Modifiers, TerminalBackend, TerminalCell, TerminalGrid,
};

/// Number of scrollback history lines to keep.
const SCROLLBACK_LINES: usize = 10_000;

/// Simple dimensions struct that implements alacritty_terminal's Dimensions trait.
struct TermDimensions {
    cols: usize,
    rows: usize,
}

impl TermDimensions {
    fn new(cols: usize, rows: usize) -> Self {
        Self { cols, rows }
    }
}

impl Dimensions for TermDimensions {
    fn columns(&self) -> usize {
        self.cols
    }

    fn screen_lines(&self) -> usize {
        self.rows
    }

    fn total_lines(&self) -> usize {
        self.rows + SCROLLBACK_LINES
    }
}

/// Event listener that sets a dirty flag when the terminal has new output,
/// and forwards PtyWrite events back to the PTY (needed for DSR/CPR responses).
#[derive(Clone)]
struct TermEventListener {
    dirty: Arc<AtomicBool>,
    /// Lazily initialized after EventLoop creation so PtyWrite can be forwarded.
    pty_writer: Arc<Mutex<Option<Notifier>>>,
    /// Optional callback to wake the event loop when new output arrives.
    waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
}

impl EventListener for TermEventListener {
    fn send_event(&self, event: Event) {
        if let Event::PtyWrite(text) = &event {
            if let Ok(guard) = self.pty_writer.lock() {
                if let Some(notifier) = guard.as_ref() {
                    let _ = notifier.0.send(Msg::Input(Cow::Owned(text.clone().into_bytes())));
                }
            }
        }
        self.dirty.store(true, Ordering::Relaxed);
        if let Ok(guard) = self.waker.lock() {
            if let Some(f) = guard.as_ref() {
                f();
            }
        }
    }
}

/// Terminal backend using alacritty_terminal for PTY management and terminal emulation.
pub struct Terminal {
    /// The alacritty terminal emulator state, wrapped in a FairMutex for thread safety
    term: Arc<FairMutex<Term<TermEventListener>>>,
    /// Notifier to send messages to the PTY event loop
    notifier: Notifier,
    /// Cached grid for the trait's grid() -> &TerminalGrid method
    cached_grid: TerminalGrid,
    /// Detected current working directory (from OSC 7 or fallback)
    current_dir: Option<PathBuf>,
    /// Current column count
    cols: u16,
    /// Current row count
    rows: u16,
    /// The child process ID for CWD detection fallback
    child_pid: Option<u32>,
    /// Dirty flag — set by PTY thread when terminal has new output
    dirty: Arc<AtomicBool>,
    /// Pre-allocated buffer for raw cell data (avoids allocation in sync_grid)
    raw_buf: Vec<(char, AnsiColor, AnsiColor, CellFlags)>,
    /// Previous frame's raw cell data for diffing
    prev_raw_buf: Vec<(char, AnsiColor, AnsiColor, CellFlags)>,
    /// Copied color palette for out-of-lock conversion
    palette_buf: [Option<AnsiRgb>; 256],
    /// Grid generation counter — incremented when grid content changes
    grid_generation: u64,
    /// Stay-at-bottom mode: applied on every sync_grid until user scrolls away
    stay_at_bottom: bool,
    /// Dark/light mode — affects terminal ANSI color palette
    dark_mode: bool,
    /// Last INVERSE cell position detected during sync_grid.
    /// TUI apps (Ink/Claude Code) draw their visual cursor as INVERSE text
    /// while hiding the real terminal cursor, so this tracks the actual input position.
    inverse_cursor: Option<(u16, u16)>,
    /// Shared waker callback for event loop wakeup
    waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
}

impl Terminal {
    /// Create a new terminal backend with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_cwd(cols, rows, None)
    }

    /// Create a new terminal backend, optionally starting in the given directory.
    pub fn with_cwd(cols: u16, rows: u16, cwd: Option<PathBuf>) -> Result<Self, Box<dyn std::error::Error>> {
        let cell_width = 8;
        let cell_height = 16;

        let window_size = WindowSize {
            num_cols: cols,
            num_lines: rows,
            cell_width,
            cell_height,
        };

        let term_size = TermDimensions::new(cols as usize, rows as usize);

        let dirty = Arc::new(AtomicBool::new(true));
        let pty_writer = Arc::new(Mutex::new(None));
        let waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>> = Arc::new(Mutex::new(None));
        let listener = TermEventListener { dirty: dirty.clone(), pty_writer: pty_writer.clone(), waker: waker.clone() };

        let config = TermConfig::default();
        let term = Term::new(config, &term_size, listener.clone());
        let term = Arc::new(FairMutex::new(term));

        // Determine the shell to use
        let shell = Self::detect_shell();

        // Use provided cwd, or fall back to $HOME so .app bundles don't land in /
        let working_directory = cwd.or_else(|| std::env::var("HOME").ok().map(PathBuf::from));
        let mut env = std::collections::HashMap::new();
        env.insert(String::from("TERM"), String::from("xterm-256color"));
        let pty_config = tty::Options {
            shell: Some(tty::Shell::new(shell, vec![String::from("--login")])),
            working_directory,
            env,
            ..tty::Options::default()
        };

        // Spawn the PTY
        let pty = tty::new(&pty_config, window_size, 0)?;

        // Get child PID before moving pty into the event loop
        let child_pid = pty.child().id();

        // Create the event loop that bridges PTY I/O with the terminal emulator
        let event_loop = EventLoop::new(term.clone(), listener, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
        // Allow the event listener to forward PtyWrite events (e.g. DSR/CPR responses) back to PTY
        *pty_writer.lock().unwrap() = Some(Notifier(event_loop.channel()));
        event_loop.spawn();

        // Initialize the cached grid
        let cached_grid = Self::build_empty_grid(cols, rows);

        Ok(Terminal {
            term,
            notifier,
            cached_grid,
            current_dir: None,
            cols,
            rows,
            child_pid: Some(child_pid),
            dirty,
            raw_buf: Vec::new(),
            prev_raw_buf: Vec::new(),
            palette_buf: [None; 256],
            grid_generation: 0,
            stay_at_bottom: false,
            dark_mode: true,
            inverse_cursor: None,
            waker,
        })
    }

    /// Detect the user's preferred shell
    fn detect_shell() -> String {
        std::env::var("SHELL").unwrap_or_else(|_| {
            // Fallback: try /bin/zsh, then /bin/bash
            if std::path::Path::new("/bin/zsh").exists() {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        })
    }

    /// Build an empty grid filled with default cells
    fn build_empty_grid(cols: u16, rows: u16) -> TerminalGrid {
        let cells = (0..rows as usize)
            .map(|_| {
                (0..cols as usize)
                    .map(|_| TerminalCell::default())
                    .collect()
            })
            .collect();
        TerminalGrid { cols, rows, cells }
    }

    /// Convert a named ANSI color to RGB, respecting dark/light mode.
    fn named_color_to_rgb(dark_mode: bool, named: NamedColor) -> Color {
        if dark_mode {
            Self::named_color_dark(named)
        } else {
            Self::named_color_light(named)
        }
    }

    /// Dark mode ANSI palette
    fn named_color_dark(named: NamedColor) -> Color {
        match named {
            // Normal colors
            NamedColor::Black => Color::rgb(0.1, 0.1, 0.14),
            NamedColor::Red => Color::rgb(1.0, 0.33, 0.33),       // #FF5555
            NamedColor::Green => Color::rgb(0.31, 0.98, 0.48),    // #50FA7B
            NamedColor::Yellow => Color::rgb(0.94, 0.9, 0.55),    // #F0E68D
            NamedColor::Blue => Color::rgb(0.39, 0.58, 1.0),      // #6495FF
            NamedColor::Magenta => Color::rgb(0.74, 0.45, 1.0),   // #BD73FF
            NamedColor::Cyan => Color::rgb(0.35, 0.87, 0.93),     // #59DEED
            NamedColor::White => Color::rgb(0.78, 0.8, 0.87),     // #C7CCDE

            // Bright colors
            NamedColor::BrightBlack => Color::rgb(0.4, 0.42, 0.53),  // #676B87
            NamedColor::BrightRed => Color::rgb(1.0, 0.47, 0.42),    // #FF786B
            NamedColor::BrightGreen => Color::rgb(0.45, 1.0, 0.6),   // #73FF99
            NamedColor::BrightYellow => Color::rgb(1.0, 0.98, 0.55), // #FFFA8D
            NamedColor::BrightBlue => Color::rgb(0.53, 0.7, 1.0),    // #87B3FF
            NamedColor::BrightMagenta => Color::rgb(0.85, 0.6, 1.0), // #D999FF
            NamedColor::BrightCyan => Color::rgb(0.47, 0.94, 1.0),   // #78F0FF
            NamedColor::BrightWhite => Color::rgb(0.95, 0.96, 0.98), // #F2F5FA

            // Special
            NamedColor::Foreground => Color::rgb(0.9, 0.91, 0.95),   // #E6E8F2
            NamedColor::Background => Color::rgb(0.0, 0.0, 0.0),     // Transparent → pane BG shows
            _ => Color::rgb(0.9, 0.91, 0.95),
        }
    }

    /// Light mode ANSI palette — dark text on light background
    fn named_color_light(named: NamedColor) -> Color {
        match named {
            // Normal colors — darker variants for readability on light bg
            NamedColor::Black => Color::rgb(0.0, 0.0, 0.0),
            NamedColor::Red => Color::rgb(0.75, 0.10, 0.10),
            NamedColor::Green => Color::rgb(0.10, 0.55, 0.15),
            NamedColor::Yellow => Color::rgb(0.55, 0.42, 0.0),
            NamedColor::Blue => Color::rgb(0.15, 0.30, 0.75),
            NamedColor::Magenta => Color::rgb(0.55, 0.20, 0.75),
            NamedColor::Cyan => Color::rgb(0.0, 0.48, 0.55),
            NamedColor::White => Color::rgb(0.42, 0.42, 0.42),

            // Bright colors
            NamedColor::BrightBlack => Color::rgb(0.35, 0.35, 0.35),
            NamedColor::BrightRed => Color::rgb(0.85, 0.20, 0.15),
            NamedColor::BrightGreen => Color::rgb(0.15, 0.65, 0.20),
            NamedColor::BrightYellow => Color::rgb(0.65, 0.50, 0.0),
            NamedColor::BrightBlue => Color::rgb(0.20, 0.40, 0.85),
            NamedColor::BrightMagenta => Color::rgb(0.65, 0.30, 0.85),
            NamedColor::BrightCyan => Color::rgb(0.15, 0.65, 0.70),
            NamedColor::BrightWhite => Color::rgb(0.75, 0.75, 0.75),

            // Special
            NamedColor::Foreground => Color::rgb(0.12, 0.12, 0.12),  // Dark text
            NamedColor::Background => Color::rgb(0.0, 0.0, 0.0),     // Transparent → pane BG shows
            _ => Color::rgb(0.12, 0.12, 0.12),
        }
    }

    /// Fallback color computation for 256-color palette indices
    fn indexed_color_fallback(idx: u8) -> Color {
        match idx {
            0 => Color::rgb(0.0, 0.0, 0.0),
            1 => Color::rgb(0.8, 0.0, 0.0),
            2 => Color::rgb(0.0, 0.8, 0.0),
            3 => Color::rgb(0.8, 0.8, 0.0),
            4 => Color::rgb(0.0, 0.0, 0.8),
            5 => Color::rgb(0.8, 0.0, 0.8),
            6 => Color::rgb(0.0, 0.8, 0.8),
            7 => Color::rgb(0.75, 0.75, 0.75),
            8 => Color::rgb(0.5, 0.5, 0.5),
            9 => Color::rgb(1.0, 0.0, 0.0),
            10 => Color::rgb(0.0, 1.0, 0.0),
            11 => Color::rgb(1.0, 1.0, 0.0),
            12 => Color::rgb(0.33, 0.33, 1.0),
            13 => Color::rgb(1.0, 0.0, 1.0),
            14 => Color::rgb(0.0, 1.0, 1.0),
            15 => Color::rgb(1.0, 1.0, 1.0),
            // 16-231: 6x6x6 color cube
            16..=231 => {
                let idx = idx - 16;
                let r = idx / 36;
                let g = (idx % 36) / 6;
                let b = idx % 6;
                Color::rgb(
                    if r == 0 { 0.0 } else { (55.0 + 40.0 * r as f32) / 255.0 },
                    if g == 0 { 0.0 } else { (55.0 + 40.0 * g as f32) / 255.0 },
                    if b == 0 { 0.0 } else { (55.0 + 40.0 * b as f32) / 255.0 },
                )
            }
            // 232-255: grayscale ramp
            _ => {
                let v = (8.0 + 10.0 * (idx - 232) as f32) / 255.0;
                Color::rgb(v, v, v)
            }
        }
    }

    /// Read the grid state from alacritty_terminal and update our cached grid.
    /// Two-phase: fast lock (raw copy) then convert colors outside the lock.
    fn sync_grid(&mut self) {
        let (cols, total_lines) = {
            // Phase 1: Hold lock briefly — copy raw cell data + palette
            let mut term = self.term.lock();

            // Apply stay-at-bottom: scroll to bottom on every sync while active
            if self.stay_at_bottom {
                term.scroll_display(Scroll::Bottom);
            }
            let grid = term.grid();
            let cols = grid.columns();
            let total_lines = grid.screen_lines();
            let display_offset = grid.display_offset();
            let total_cells = cols * total_lines;

            // Copy color palette
            let colors = term.colors();
            for i in 0..256 {
                self.palette_buf[i] = colors[i];
            }

            // Copy raw cell data into flat buffer
            // When scrolled (display_offset > 0), read from scrollback history
            self.raw_buf.resize(total_cells, (' ', AnsiColor::Named(NamedColor::Foreground), AnsiColor::Named(NamedColor::Background), CellFlags::empty()));
            for line_idx in 0..total_lines {
                let line = Line(line_idx as i32 - display_offset as i32);
                let base = line_idx * cols;
                for col_idx in 0..cols {
                    let point = Point::new(line, Column(col_idx));
                    let cell = &grid[point];
                    self.raw_buf[base + col_idx] = (cell.c, cell.fg, cell.bg, cell.flags);
                }
            }

            (cols, total_lines)
        }; // Lock released here!

        // Phase 2: Diff with previous frame — only convert changed cells
        let total_cells = cols * total_lines;
        let same_size = self.prev_raw_buf.len() == total_cells;
        let dark_mode = self.dark_mode;

        // Scan for the last INVERSE cell — TUI apps (Ink/Claude Code) draw their
        // visual cursor as an INVERSE cell while hiding the real terminal cursor.
        // We track this position to use as the effective cursor for block cursor
        // rendering and IME preedit overlay positioning.
        self.inverse_cursor = None;
        for idx in (0..total_cells).rev() {
            if self.raw_buf[idx].3.contains(CellFlags::INVERSE) {
                let row = idx / cols;
                let col = idx % cols;
                self.inverse_cursor = Some((row as u16, col as u16));
                break;
            }
        }

        let cells = &mut self.cached_grid.cells;
        cells.resize_with(total_lines, || vec![TerminalCell::default(); cols]);

        let mut any_changed = false;

        for (line_idx, row) in cells.iter_mut().enumerate().take(total_lines) {
            row.resize_with(cols, TerminalCell::default);
            let base = line_idx * cols;

            for (col_idx, tc) in row.iter_mut().enumerate().take(cols) {
                let idx = base + col_idx;
                let raw = self.raw_buf[idx];

                // Skip unchanged cells (same char, fg, bg, flags)
                if same_size && self.prev_raw_buf[idx] == raw {
                    continue;
                }
                any_changed = true;

                let (c, fg, bg, flags) = raw;

                if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    tc.character = '\0';
                    tc.style.background = None;
                    continue;
                }

                let mut fg_color = Self::convert_color(dark_mode, &fg, &self.palette_buf);
                let mut bg_color = Self::convert_color(dark_mode, &bg, &self.palette_buf);
                let mut bg_is_default = matches!(bg, AnsiColor::Named(NamedColor::Background));

                // SGR 7: swap foreground and background
                if flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                    bg_is_default = false;
                }

                let background = if bg_is_default {
                    None
                } else {
                    Some(bg_color)
                };

                tc.character = c;
                tc.style.bold = flags.contains(CellFlags::BOLD);
                tc.style.dim = flags.contains(CellFlags::DIM);
                tc.style.italic = flags.contains(CellFlags::ITALIC);
                tc.style.underline = flags.contains(CellFlags::UNDERLINE)
                    || flags.contains(CellFlags::DOUBLE_UNDERLINE)
                    || flags.contains(CellFlags::UNDERCURL);

                tc.style.foreground = if tc.style.dim {
                    Color::new(fg_color.r * 0.65, fg_color.g * 0.65, fg_color.b * 0.65, fg_color.a)
                } else {
                    fg_color
                };
                tc.style.background = background;
            }
        }

        // Swap buffers for next frame's diff
        std::mem::swap(&mut self.prev_raw_buf, &mut self.raw_buf);

        if any_changed || !same_size {
            self.grid_generation += 1;
        }

        cells.truncate(total_lines);
        self.cached_grid.cols = cols as u16;
        self.cached_grid.rows = total_lines as u16;
    }

    /// Convert color using pre-copied palette (no lock needed)
    fn convert_color(dark_mode: bool, color: &AnsiColor, palette: &[Option<AnsiRgb>; 256]) -> Color {
        match color {
            AnsiColor::Named(named) => Self::named_color_to_rgb(dark_mode, *named),
            AnsiColor::Spec(rgb) => Color::rgb(
                rgb.r as f32 / 255.0,
                rgb.g as f32 / 255.0,
                rgb.b as f32 / 255.0,
            ),
            AnsiColor::Indexed(idx) => {
                // Indices 0-15 → route through our named palette (respects dark/light)
                if *idx < 16 {
                    let named = Self::index_to_named(*idx);
                    return Self::named_color_to_rgb(dark_mode, named);
                }
                if let Some(rgb) = palette[*idx as usize] {
                    Color::rgb(
                        rgb.r as f32 / 255.0,
                        rgb.g as f32 / 255.0,
                        rgb.b as f32 / 255.0,
                    )
                } else {
                    Self::indexed_color_fallback(*idx)
                }
            }
        }
    }

    /// Map indexed color 0-15 to the corresponding NamedColor.
    fn index_to_named(idx: u8) -> NamedColor {
        match idx {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            15 => NamedColor::BrightWhite,
            _ => NamedColor::Foreground,
        }
    }

    /// Detect the CWD of the child process using native OS APIs (no subprocess).
    #[cfg(target_os = "macos")]
    pub fn detect_cwd_fallback(&self) -> Option<PathBuf> {
        let pid = self.child_pid? as i32;

        // Use proc_pidinfo with PROC_PIDVNODEPATHINFO to get CWD directly.
        // This is a direct kernel call — no subprocess spawn, effectively instant.
        const PROC_PIDVNODEPATHINFO: i32 = 9;
        const BUF_SIZE: usize = 2352; // sizeof(proc_vnodepathinfo)
        const PATH_OFFSET: usize = 152; // sizeof(vnode_info) — path follows
        const MAXPATHLEN: usize = 1024;

        let mut buf = [0u8; BUF_SIZE];
        let ret = unsafe {
            libc::proc_pidinfo(
                pid,
                PROC_PIDVNODEPATHINFO,
                0,
                buf.as_mut_ptr() as *mut libc::c_void,
                BUF_SIZE as i32,
            )
        };

        if ret <= 0 {
            return None;
        }

        let path_bytes = &buf[PATH_OFFSET..PATH_OFFSET + MAXPATHLEN];
        let len = path_bytes.iter().position(|&b| b == 0).unwrap_or(0);
        if len == 0 {
            return None;
        }

        let path = std::str::from_utf8(&path_bytes[..len]).ok()?;
        let p = PathBuf::from(path);
        if p.is_dir() { Some(p) } else { None }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn detect_cwd_fallback(&self) -> Option<PathBuf> {
        if let Some(pid) = self.child_pid {
            let path = format!("/proc/{}/cwd", pid);
            std::fs::read_link(path).ok()
        } else {
            None
        }
    }

    /// Convert a key event to the byte sequence that should be sent to the PTY
    pub fn key_to_bytes(key: &Key, modifiers: &Modifiers) -> Vec<u8> {
        match key {
            Key::Char(c) => {
                if modifiers.ctrl {
                    // Ctrl+A..Z maps to 0x01..0x1A
                    let lower = c.to_ascii_lowercase();
                    if lower.is_ascii_lowercase() {
                        return vec![(lower as u8) - b'a' + 1];
                    }
                }
                if modifiers.alt {
                    // Alt sends ESC prefix
                    let mut bytes = vec![0x1b];
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    bytes.extend_from_slice(s.as_bytes());
                    return bytes;
                }
                let mut buf = [0u8; 4];
                let s = c.encode_utf8(&mut buf);
                s.as_bytes().to_vec()
            }
            Key::Enter => {
                if modifiers.shift {
                    vec![0x1b, b'[', b'1', b'3', b';', b'2', b'u'] // CSI u: ESC[13;2u
                } else {
                    vec![0x0d] // CR
                }
            }
            Key::Backspace => vec![0x7f],   // DEL
            Key::Tab => {
                if modifiers.shift {
                    vec![0x1b, b'[', b'Z'] // Shift+Tab = CSI Z
                } else {
                    vec![0x09]
                }
            }
            Key::Escape => vec![0x1b],
            Key::Delete => vec![0x1b, b'[', b'3', b'~'],
            Key::Up => Self::arrow_bytes(b'A', modifiers),
            Key::Down => Self::arrow_bytes(b'B', modifiers),
            Key::Right => Self::arrow_bytes(b'C', modifiers),
            Key::Left => Self::arrow_bytes(b'D', modifiers),
            Key::Home => vec![0x1b, b'[', b'H'],
            Key::End => vec![0x1b, b'[', b'F'],
            Key::PageUp => vec![0x1b, b'[', b'5', b'~'],
            Key::PageDown => vec![0x1b, b'[', b'6', b'~'],
            Key::Insert => vec![0x1b, b'[', b'2', b'~'],
            Key::F(n) => match n {
                1 => vec![0x1b, b'O', b'P'],
                2 => vec![0x1b, b'O', b'Q'],
                3 => vec![0x1b, b'O', b'R'],
                4 => vec![0x1b, b'O', b'S'],
                5 => vec![0x1b, b'[', b'1', b'5', b'~'],
                6 => vec![0x1b, b'[', b'1', b'7', b'~'],
                7 => vec![0x1b, b'[', b'1', b'8', b'~'],
                8 => vec![0x1b, b'[', b'1', b'9', b'~'],
                9 => vec![0x1b, b'[', b'2', b'0', b'~'],
                10 => vec![0x1b, b'[', b'2', b'1', b'~'],
                11 => vec![0x1b, b'[', b'2', b'3', b'~'],
                12 => vec![0x1b, b'[', b'2', b'4', b'~'],
                _ => vec![],
            },
        }
    }

    /// Build the CSI escape sequence for an arrow key with modifier support.
    /// Plain arrow: `\e[{dir}`, with modifiers: `\e[1;{mod}{dir}`
    /// Modifier codes: 2=Shift, 3=Alt, 5=Ctrl, etc.
    fn arrow_bytes(dir: u8, modifiers: &Modifiers) -> Vec<u8> {
        let modifier_code = 1
            + if modifiers.shift { 1 } else { 0 }
            + if modifiers.alt { 2 } else { 0 }
            + if modifiers.ctrl { 4 } else { 0 };
        if modifier_code > 1 {
            // CSI 1 ; {modifier} {dir}
            let code = b'0' + modifier_code;
            vec![0x1b, b'[', b'1', b';', code, dir]
        } else {
            vec![0x1b, b'[', dir]
        }
    }
}

impl Terminal {
    /// Set a waker callback that will be called from the PTY thread when new output arrives.
    /// This allows the event loop to sleep with `ControlFlow::Wait` and be woken up on demand.
    pub fn set_waker(&self, f: Box<dyn Fn() + Send>) {
        *self.waker.lock().unwrap() = Some(f);
    }

    /// Returns true if the terminal has new output since the last process() call.
    pub fn has_new_output(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Returns the grid generation counter. Increments when grid content changes.
    pub fn grid_generation(&self) -> u64 {
        self.grid_generation
    }

    /// Returns the current column count.
    pub fn current_cols(&self) -> u16 {
        self.cols
    }

    /// Returns the current row count.
    pub fn current_rows(&self) -> u16 {
        self.rows
    }

    /// Search the full scrollback + screen buffer for case-insensitive substring matches.
    /// Returns `(absolute_line_from_top, char_col, char_len)` tuples.
    pub fn search_buffer(&self, query: &str) -> Vec<(usize, usize, usize)> {
        let mut results = Vec::new();
        if query.is_empty() {
            return results;
        }

        let query_lower = query.to_lowercase();
        let query_char_len = query.chars().count();
        let term = self.term.lock();
        let grid = term.grid();
        let total_lines = grid.screen_lines();
        let history_len = grid.history_size();
        let cols = grid.columns();

        // Iterate history lines (from oldest to newest) then screen lines
        for abs_line in 0..(history_len + total_lines) {
            // History lines: negative Line indices; screen lines: 0..total_lines
            let line_idx = Line(abs_line as i32 - history_len as i32);
            let mut row_text = String::with_capacity(cols);
            for col_idx in 0..cols {
                let point = Point::new(line_idx, Column(col_idx));
                let c = grid[point].c;
                row_text.push(if c == '\0' { ' ' } else { c });
            }

            let row_lower = row_text.to_lowercase();
            let mut start = 0; // byte offset for string slicing
            while let Some(byte_pos) = row_lower[start..].find(&query_lower) {
                let byte_col = start + byte_pos;
                // Convert byte offset to char column index
                let char_col = row_text[..byte_col].chars().count();
                results.push((abs_line, char_col, query_char_len));
                // Advance by one character (not one byte) to find overlapping matches
                start = byte_col + row_lower[byte_col..].chars().next().map_or(1, |c| c.len_utf8());
            }
        }

        results
    }

    /// Get the current display offset (how many lines scrolled up into history).
    pub fn display_offset(&self) -> usize {
        let term = self.term.lock();
        term.grid().display_offset()
    }

    /// Get the number of history (scrollback) lines.
    pub fn history_size(&self) -> usize {
        let term = self.term.lock();
        term.grid().history_size()
    }

    /// Set dark/light mode for the terminal color palette.
    /// Forces a full grid re-render so all colors are updated.
    pub fn set_dark_mode(&mut self, dark: bool) {
        if self.dark_mode != dark {
            self.dark_mode = dark;
            // Clear prev_raw_buf to force full re-render of all cells
            self.prev_raw_buf.clear();
            self.dirty.store(true, Ordering::Relaxed);
        }
    }

    /// Enter stay-at-bottom mode: every sync_grid will scroll to bottom until
    /// the user explicitly scrolls away via scroll_display().
    pub fn request_scroll_to_bottom(&mut self) {
        self.stay_at_bottom = true;
        self.dirty.store(true, Ordering::Relaxed);
    }

    /// Scroll the terminal display by the given delta (positive = scroll up into history).
    /// Cancels stay-at-bottom mode since the user is explicitly scrolling.
    pub fn scroll_display(&mut self, delta: i32) {
        // User is explicitly scrolling — cancel stay-at-bottom
        self.stay_at_bottom = false;

        let mut term = self.term.lock();
        let old_offset = term.grid().display_offset();
        term.scroll_display(Scroll::Delta(delta));
        let new_offset = term.grid().display_offset();
        drop(term);

        if old_offset != new_offset {
            // Force a grid sync on the next frame
            self.dirty.store(true, Ordering::Relaxed);
        }
    }
}

impl TerminalBackend for Terminal {
    fn write(&mut self, data: &[u8]) {
        // Send bytes to the PTY via the notifier channel
        let _ = self.notifier.0.send(Msg::Input(Cow::Owned(data.to_vec())));
    }

    fn process(&mut self) {
        // Only sync when the PTY thread has produced new output
        if self.dirty.swap(false, Ordering::Relaxed) {
            self.sync_grid();
        }
    }

    fn grid(&self) -> &TerminalGrid {
        &self.cached_grid
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        if self.cols == cols && self.rows == rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;

        let cell_width = 8;
        let cell_height = 16;

        let window_size = WindowSize {
            num_cols: cols,
            num_lines: rows,
            cell_width,
            cell_height,
        };

        let term_size = TermDimensions::new(cols as usize, rows as usize);

        // Resize the terminal emulator
        {
            let mut term = self.term.lock();
            term.resize(term_size);
        }

        // Notify the PTY about the resize
        let _ = self.notifier.0.send(Msg::Resize(window_size));
    }

    fn cwd(&self) -> Option<PathBuf> {
        self.current_dir.clone()
    }

    fn cursor(&self) -> CursorState {
        let term = self.term.lock();
        let cursor = &term.grid().cursor;
        let point = cursor.point;

        let shape = match term.cursor_style().shape {
            alacritty_terminal::vte::ansi::CursorShape::Block => CursorShape::Block,
            alacritty_terminal::vte::ansi::CursorShape::Beam => CursorShape::Beam,
            alacritty_terminal::vte::ansi::CursorShape::Underline => CursorShape::Underline,
            _ => CursorShape::Block,
        };

        // SHOW_CURSOR mode flag is set when DECTCEM is enabled (cursor visible)
        let visible = term.mode().contains(TermMode::SHOW_CURSOR);

        // When the terminal cursor is hidden (TUI apps like Claude Code / Ink),
        // the real cursor position is unreliable.  Use the INVERSE cell position
        // detected during sync_grid as the effective cursor location instead.
        let (row, col) = if !visible {
            if let Some((inv_row, inv_col)) = self.inverse_cursor {
                (inv_row, inv_col)
            } else {
                (point.line.0 as u16, point.column.0 as u16)
            }
        } else {
            (point.line.0 as u16, point.column.0 as u16)
        };

        CursorState {
            row,
            col,
            visible,
            shape,
        }
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Signal the event loop to shut down
        #[allow(unused)]
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

mod tests;
