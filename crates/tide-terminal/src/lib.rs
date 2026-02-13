// Terminal backend implementation (Stream B)
// Implements tide_core::TerminalBackend using alacritty_terminal

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb as AnsiRgb};

use tide_core::{
    Color, CursorShape, CursorState, Key, Modifiers, TerminalBackend, TerminalCell, TerminalGrid,
    TextStyle,
};

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
        self.rows
    }
}

/// Event listener that sets a dirty flag when the terminal has new output.
#[derive(Clone)]
struct TermEventListener {
    dirty: Arc<AtomicBool>,
}

impl EventListener for TermEventListener {
    fn send_event(&self, _event: Event) {
        self.dirty.store(true, Ordering::Relaxed);
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
}

impl Terminal {
    /// Create a new terminal backend with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
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
        let listener = TermEventListener { dirty: dirty.clone() };

        let config = TermConfig::default();
        let term = Term::new(config, &term_size, listener.clone());
        let term = Arc::new(FairMutex::new(term));

        // Determine the shell to use
        let shell = Self::detect_shell();

        // Create PTY config
        let pty_config = tty::Options {
            shell: Some(tty::Shell::new(shell, Vec::new())),
            ..tty::Options::default()
        };

        // Spawn the PTY
        let pty = tty::new(&pty_config, window_size, 0)?;

        // Get child PID before moving pty into the event loop
        let child_pid = pty.child().id();

        // Create the event loop that bridges PTY I/O with the terminal emulator
        let event_loop = EventLoop::new(term.clone(), listener, pty, false, false)?;
        let notifier = Notifier(event_loop.channel());
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

    /// Convert an alacritty_terminal color to our Color type
    fn convert_color(color: &AnsiColor, colors: &alacritty_terminal::term::color::Colors) -> Color {
        match color {
            AnsiColor::Named(named) => Self::named_color_to_rgb(*named),
            AnsiColor::Spec(rgb) => Color::rgb(
                rgb.r as f32 / 255.0,
                rgb.g as f32 / 255.0,
                rgb.b as f32 / 255.0,
            ),
            AnsiColor::Indexed(idx) => {
                // Look up in the color palette
                if let Some(rgb) = colors[*idx as usize] {
                    Color::rgb(
                        rgb.r as f32 / 255.0,
                        rgb.g as f32 / 255.0,
                        rgb.b as f32 / 255.0,
                    )
                } else {
                    // Fallback to default color computation for indexed colors
                    Self::indexed_color_fallback(*idx)
                }
            }
        }
    }

    /// Convert a named ANSI color to RGB (vibrant Warp-inspired palette)
    fn named_color_to_rgb(named: NamedColor) -> Color {
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
            NamedColor::Background => Color::rgb(0.0, 0.0, 0.0),    // Transparent → pane BG shows
            _ => Color::rgb(0.9, 0.91, 0.95),
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
            let term = self.term.lock();
            let grid = term.grid();
            let cols = grid.columns();
            let total_lines = grid.screen_lines();
            let total_cells = cols * total_lines;

            // Copy color palette
            let colors = term.colors();
            for i in 0..256 {
                self.palette_buf[i] = colors[i];
            }

            // Copy raw cell data into flat buffer
            self.raw_buf.resize(total_cells, (' ', AnsiColor::Named(NamedColor::Foreground), AnsiColor::Named(NamedColor::Background), CellFlags::empty()));
            for line_idx in 0..total_lines {
                let line = Line(line_idx as i32);
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

        let cells = &mut self.cached_grid.cells;
        cells.resize_with(total_lines, || vec![TerminalCell::default(); cols]);

        let mut any_changed = false;

        for line_idx in 0..total_lines {
            let row = &mut cells[line_idx];
            row.resize_with(cols, TerminalCell::default);
            let base = line_idx * cols;

            for col_idx in 0..cols {
                let idx = base + col_idx;
                let raw = self.raw_buf[idx];

                // Skip unchanged cells (same char, fg, bg, flags)
                if same_size && self.prev_raw_buf[idx] == raw {
                    continue;
                }
                any_changed = true;

                let (c, fg, bg, flags) = raw;

                if flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                    let tc = &mut row[col_idx];
                    tc.character = '\0';
                    tc.style.background = None;
                    continue;
                }

                let fg_color = Self::convert_color_palette(&fg, &self.palette_buf);
                let bg_color = Self::convert_color_palette(&bg, &self.palette_buf);

                let background = if bg_color.r == 0.0 && bg_color.g == 0.0 && bg_color.b == 0.0 {
                    None
                } else {
                    Some(bg_color)
                };

                let tc = &mut row[col_idx];
                tc.character = c;
                tc.style.foreground = fg_color;
                tc.style.background = background;
                tc.style.bold = flags.contains(CellFlags::BOLD);
                tc.style.italic = flags.contains(CellFlags::ITALIC);
                tc.style.underline = flags.contains(CellFlags::UNDERLINE)
                    || flags.contains(CellFlags::DOUBLE_UNDERLINE)
                    || flags.contains(CellFlags::UNDERCURL);
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
    fn convert_color_palette(color: &AnsiColor, palette: &[Option<AnsiRgb>; 256]) -> Color {
        match color {
            AnsiColor::Named(named) => Self::named_color_to_rgb(*named),
            AnsiColor::Spec(rgb) => Color::rgb(
                rgb.r as f32 / 255.0,
                rgb.g as f32 / 255.0,
                rgb.b as f32 / 255.0,
            ),
            AnsiColor::Indexed(idx) => {
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
                    if lower >= 'a' && lower <= 'z' {
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
            Key::Enter => vec![0x0d],       // CR
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
            Key::Up => {
                if modifiers.alt {
                    vec![0x1b, 0x1b, b'[', b'A']
                } else {
                    vec![0x1b, b'[', b'A']
                }
            }
            Key::Down => {
                if modifiers.alt {
                    vec![0x1b, 0x1b, b'[', b'B']
                } else {
                    vec![0x1b, b'[', b'B']
                }
            }
            Key::Right => {
                if modifiers.alt {
                    vec![0x1b, 0x1b, b'[', b'C']
                } else {
                    vec![0x1b, b'[', b'C']
                }
            }
            Key::Left => {
                if modifiers.alt {
                    vec![0x1b, 0x1b, b'[', b'D']
                } else {
                    vec![0x1b, b'[', b'D']
                }
            }
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
}

impl Terminal {
    /// Returns true if the terminal has new output since the last process() call.
    pub fn has_new_output(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Returns the grid generation counter. Increments when grid content changes.
    pub fn grid_generation(&self) -> u64 {
        self.grid_generation
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

        CursorState {
            row: point.line.0 as u16,
            col: point.column.0 as u16,
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let color = Terminal::named_color_to_rgb(NamedColor::Red);
        assert_eq!(color, Color::rgb(0.8, 0.0, 0.0));
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
}
