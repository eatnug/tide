// Terminal backend implementation (Stream B)
// Implements tide_core::TerminalBackend using alacritty_terminal

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::event_loop::{EventLoop, Msg, Notifier};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags as CellFlags;
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode};
use alacritty_terminal::tty;
use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb as AnsiRgb};

pub mod git;
mod color;
mod key_input;

use tide_core::{
    Color, CursorShape, CursorState, TerminalBackend, TerminalCell, TerminalGrid,
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
    /// Detected URL ranges per row: Vec of (start_col, end_col) for each row
    url_ranges: Vec<Vec<(usize, usize)>>,
    /// Pending PTY resize notification (debounced to avoid SIGWINCH storms during animations)
    pending_pty_resize: Option<(WindowSize, Instant)>,
}

impl Terminal {
    /// Create a new terminal backend with the given dimensions.
    pub fn new(cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
        Self::with_cwd(cols, rows, None, true)
    }

    /// Create a new terminal backend, optionally starting in the given directory.
    pub fn with_cwd(cols: u16, rows: u16, cwd: Option<PathBuf>, dark_mode: bool) -> Result<Self, Box<dyn std::error::Error>> {
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
        // Advertise 24-bit true color support so apps (Claude Code, etc.)
        // use their RGB theme instead of mapping to the 16-color ANSI palette.
        env.insert(String::from("COLORTERM"), String::from("truecolor"));
        // Suppress zsh's partial-line indicator (%) on initial startup
        env.insert(String::from("PROMPT_EOL_MARK"), String::new());
        // Signal dark/light mode to the shell (many prompts and tools read this)
        if dark_mode {
            env.insert(String::from("COLORFGBG"), String::from("15;0"));
        } else {
            env.insert(String::from("COLORFGBG"), String::from("0;15"));
        }
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
        if let Ok(mut guard) = pty_writer.lock() {
            *guard = Some(Notifier(event_loop.channel()));
        }
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
            dark_mode,
            inverse_cursor: None,
            waker,
            url_ranges: Vec::new(),
            pending_pty_resize: None,
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
        // Skip WIDE_CHAR_SPACER cells so that the detected position is always
        // the main (first) cell of a wide character — this ensures the cursor
        // overlay covers the full character width instead of just the spacer.
        self.inverse_cursor = None;
        for idx in (0..total_cells).rev() {
            let flags = self.raw_buf[idx].3;
            if flags.contains(CellFlags::INVERSE)
                && !flags.contains(CellFlags::WIDE_CHAR_SPACER)
            {
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
                if dark_mode {
                    fg_color = Self::ensure_dark_fg_contrast(fg_color);
                } else {
                    fg_color = Self::ensure_light_fg_contrast(fg_color);
                }
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

        // Scan for URLs in the grid
        if any_changed || !same_size {
            self.detect_urls();
        }
    }

    /// Detect URLs in the cached grid and store column ranges per row.
    fn detect_urls(&mut self) {
        static URL_RE: OnceLock<regex::Regex> = OnceLock::new();
        let re = URL_RE.get_or_init(|| {
            regex::Regex::new(r#"https?://[^\s<>"{}|\\^`\[\]]+"#).unwrap()
        });

        let rows = self.cached_grid.cells.len();
        self.url_ranges.resize(rows, Vec::new());

        for (row_idx, row) in self.cached_grid.cells.iter().enumerate() {
            self.url_ranges[row_idx].clear();
            let row_text: String = row.iter().map(|c| {
                if c.character == '\0' { ' ' } else { c.character }
            }).collect();
            for m in re.find_iter(&row_text) {
                // Convert byte offsets to char (column) indices
                let start_col = row_text[..m.start()].chars().count();
                let end_col = start_col + m.as_str().chars().count();
                self.url_ranges[row_idx].push((start_col, end_col));
            }
        }
        self.url_ranges.truncate(rows);
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
}

impl Terminal {
    /// Set a waker callback that will be called from the PTY thread when new output arrives.
    /// This allows the event loop to sleep with `ControlFlow::Wait` and be woken up on demand.
    pub fn set_waker(&self, f: Box<dyn Fn() + Send>) {
        if let Ok(mut guard) = self.waker.lock() {
            *guard = Some(f);
        }
    }

    /// Returns the child PID of the shell process.
    pub fn child_pid(&self) -> Option<u32> {
        self.child_pid
    }

    /// Detect whether the shell is idle (no foreground child process running).
    /// Uses native kernel API — no subprocess spawn.
    #[cfg(target_os = "macos")]
    pub fn is_shell_idle(&self) -> bool {
        let pid = match self.child_pid {
            Some(p) => p,
            None => return false,
        };
        // Use proc_listchildpids to check if shell has any child processes.
        // This is a direct kernel call — no subprocess spawn, effectively instant.
        let mut pids = [0i32; 16];
        let ret = unsafe {
            libc::proc_listchildpids(
                pid as i32,
                pids.as_mut_ptr() as *mut libc::c_void,
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            )
        };
        // ret = number of child PIDs found. 0 means no children → shell is idle.
        ret <= 0
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_shell_idle(&self) -> bool {
        let pid = match self.child_pid {
            Some(p) => p,
            None => return false,
        };
        // Linux: check /proc/PID/stat for foreground process group
        let stat_path = format!("/proc/{}/stat", pid);
        if let Ok(contents) = std::fs::read_to_string(&stat_path) {
            let fields: Vec<&str> = contents.split_whitespace().collect();
            // Field 4 = pgrp, field 7 = tpgid (foreground process group)
            if fields.len() > 7 {
                let pgrp = fields[4].parse::<i32>().unwrap_or(0);
                let tpgid = fields[7].parse::<i32>().unwrap_or(-1);
                return pgrp == tpgid;
            }
        }
        false
    }

    /// Returns true if the terminal has new output since the last process() call.
    pub fn has_new_output(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }

    /// Returns the grid generation counter. Increments when grid content changes.
    pub fn grid_generation(&self) -> u64 {
        self.grid_generation
    }

    /// Force a sync_grid cycle for benchmarking purposes.
    /// Sets the dirty flag and calls process() to trigger the full sync pipeline.
    #[doc(hidden)]
    pub fn bench_sync_grid(&mut self) {
        self.dirty.store(true, Ordering::Relaxed);
        self.sync_grid();
    }

    /// Inject bytes directly into the terminal emulator for benchmarking.
    /// Bypasses the PTY — feeds data straight into vte::ansi::Processor → Term.
    #[doc(hidden)]
    pub fn bench_write_to_term(&self, data: &[u8]) {
        use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};
        let mut processor: Processor<StdSyncHandler> = Processor::new();
        let mut term = self.term.lock();
        processor.advance(&mut *term, data);
    }

    /// Returns detected URL column ranges per row.
    pub fn url_ranges(&self) -> &[Vec<(usize, usize)>] {
        &self.url_ranges
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
        // Flush debounced PTY resize if 50ms have elapsed
        if let Some((window_size, stamp)) = self.pending_pty_resize {
            if stamp.elapsed().as_millis() >= 50 {
                self.pending_pty_resize = None;
                let _ = self.notifier.0.send(Msg::Resize(window_size));
            }
        }

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

        // Resize the terminal grid immediately (for correct rendering)
        {
            let mut term = self.term.lock();
            term.resize(term_size);
        }

        // Debounce PTY resize notification (SIGWINCH) to avoid prompt artifacts
        // during rapid resize events (e.g. macOS maximize/restore animation)
        self.pending_pty_resize = Some((window_size, Instant::now()));
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
