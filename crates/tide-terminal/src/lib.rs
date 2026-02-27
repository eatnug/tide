// Terminal backend implementation
// Implements tide_core::TerminalBackend using alacritty_terminal
//
// Threading model:
//   PTY Thread (alacritty EventLoop) — reads PTY, parses VT, updates Term state
//   Sync Thread — copies grid state from Term, converts colors, produces snapshots
//   Main Thread — swaps in latest snapshot, renders, handles input
//
// The sync thread decouples expensive grid synchronization from the main thread,
// so input events are never blocked by terminal output processing.

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

// ──────────────────────────────────────────────
// Shared snapshot: exchange point between sync thread and main thread
// ──────────────────────────────────────────────

struct SharedSnapshot {
    grid: TerminalGrid,
    inverse_cursor: Option<(u16, u16)>,
    url_ranges: Vec<Vec<(usize, usize)>>,
    generation: u64,
    cursor: CursorState,
}

// ──────────────────────────────────────────────
// Event listener (PTY thread → sync thread signaling)
// ──────────────────────────────────────────────

/// Event listener that sets a dirty flag when the terminal has new output,
/// forwards PtyWrite events back to the PTY, and wakes the sync thread.
#[derive(Clone)]
struct TermEventListener {
    dirty: Arc<AtomicBool>,
    /// Lazily initialized after EventLoop creation so PtyWrite can be forwarded.
    pty_writer: Arc<Mutex<Option<Notifier>>>,
    /// Handle to the grid sync thread — unparked when new output arrives.
    sync_thread: Arc<Mutex<Option<std::thread::Thread>>>,
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
        // Wake the sync thread to process new output
        if let Ok(guard) = self.sync_thread.lock() {
            if let Some(ref thread) = *guard {
                thread.unpark();
            }
        }
    }
}

// ──────────────────────────────────────────────
// GridSyncer: owns all state for grid synchronization (runs on sync thread)
// ──────────────────────────────────────────────

struct GridSyncer {
    term: Arc<FairMutex<Term<TermEventListener>>>,
    raw_buf: Vec<(char, AnsiColor, AnsiColor, CellFlags)>,
    prev_raw_buf: Vec<(char, AnsiColor, AnsiColor, CellFlags)>,
    palette_buf: [Option<AnsiRgb>; 256],
    grid: TerminalGrid,
    inverse_cursor: Option<(u16, u16)>,
    cached_cursor: CursorState,
    url_ranges: Vec<Vec<(usize, usize)>>,
    grid_generation: u64,
    url_row_buf: String,
    last_url_detect: Instant,
    dark_mode: Arc<AtomicBool>,
    dark_mode_changed: Arc<AtomicBool>,
    stay_at_bottom: Arc<AtomicBool>,
}

impl GridSyncer {
    /// Run one grid synchronization cycle.
    /// Phase 1: Lock Term briefly to copy raw cell data + palette.
    /// Phase 2: Convert colors and diff against previous frame (no lock held).
    fn sync(&mut self) {
        // Check if dark mode changed — force full re-render
        if self.dark_mode_changed.swap(false, Ordering::Relaxed) {
            self.prev_raw_buf.clear();
        }

        let dark_mode = self.dark_mode.load(Ordering::Relaxed);
        let stay_at_bottom = self.stay_at_bottom.load(Ordering::Relaxed);

        // Phase 1: Hold lock briefly — copy raw cell data + palette + cursor
        let (cols, total_lines) = {
            let mut term = self.term.lock();

            if stay_at_bottom {
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
            self.raw_buf.resize(
                total_cells,
                (' ', AnsiColor::Named(NamedColor::Foreground), AnsiColor::Named(NamedColor::Background), CellFlags::empty()),
            );
            for line_idx in 0..total_lines {
                let line = Line(line_idx as i32 - display_offset as i32);
                let base = line_idx * cols;
                for col_idx in 0..cols {
                    let point = Point::new(line, Column(col_idx));
                    let cell = &grid[point];
                    self.raw_buf[base + col_idx] = (cell.c, cell.fg, cell.bg, cell.flags);
                }
            }

            // Read cursor state while we have the lock
            let cursor_point = grid.cursor.point;
            let cursor_shape = match term.cursor_style().shape {
                alacritty_terminal::vte::ansi::CursorShape::Block => CursorShape::Block,
                alacritty_terminal::vte::ansi::CursorShape::Beam => CursorShape::Beam,
                alacritty_terminal::vte::ansi::CursorShape::Underline => CursorShape::Underline,
                _ => CursorShape::Block,
            };
            let cursor_visible = term.mode().contains(TermMode::SHOW_CURSOR);

            self.cached_cursor = CursorState {
                row: cursor_point.line.0 as u16,
                col: cursor_point.column.0 as u16,
                visible: cursor_visible,
                shape: cursor_shape,
            };

            (cols, total_lines)
        }; // Lock released here!

        // Phase 2: Diff with previous frame — only convert changed cells
        let total_cells = cols * total_lines;
        let same_size = self.prev_raw_buf.len() == total_cells;

        // Scan for the last INVERSE cell — TUI apps (Ink/Claude Code) draw their
        // visual cursor as an INVERSE cell while hiding the real terminal cursor.
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

        // Apply INVERSE cursor fallback to cached_cursor
        if !self.cached_cursor.visible {
            if let Some((inv_row, inv_col)) = self.inverse_cursor {
                self.cached_cursor.row = inv_row;
                self.cached_cursor.col = inv_col;
            }
        }

        let cells = &mut self.grid.cells;
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
                    // Preserve background for selection/ANSI highlights on
                    // the second half of wide characters (Korean, CJK, etc.).
                    let mut bg_color = Terminal::convert_color(dark_mode, &bg, &self.palette_buf);
                    let mut bg_is_default = matches!(bg, AnsiColor::Named(NamedColor::Background));
                    if flags.contains(CellFlags::INVERSE) {
                        let fg_color = Terminal::convert_color(dark_mode, &fg, &self.palette_buf);
                        bg_color = fg_color;
                        bg_is_default = false;
                    }
                    tc.style.background = if bg_is_default { None } else { Some(bg_color) };
                    continue;
                }

                let mut fg_color = Terminal::convert_color(dark_mode, &fg, &self.palette_buf);
                let mut bg_color = Terminal::convert_color(dark_mode, &bg, &self.palette_buf);
                let mut bg_is_default = matches!(bg, AnsiColor::Named(NamedColor::Background));

                // SGR 7: swap foreground and background
                if flags.contains(CellFlags::INVERSE) {
                    std::mem::swap(&mut fg_color, &mut bg_color);
                    bg_is_default = false;
                }

                if dark_mode {
                    fg_color = Terminal::ensure_dark_fg_contrast(fg_color);
                } else {
                    fg_color = Terminal::ensure_light_fg_contrast(fg_color);
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
        self.grid.cols = cols as u16;
        self.grid.rows = total_lines as u16;

        // Scan for URLs in the grid (throttled to avoid regex cost on rapid output)
        if (any_changed || !same_size)
            && self.last_url_detect.elapsed().as_millis() >= 200
        {
            self.detect_urls();
            self.last_url_detect = Instant::now();
        }
    }

    /// Detect URLs in the grid and store column ranges per row.
    fn detect_urls(&mut self) {
        static URL_RE: OnceLock<regex::Regex> = OnceLock::new();
        let re = URL_RE.get_or_init(|| {
            regex::Regex::new(r#"https?://[^\s<>"{}|\\^`\[\]]+"#).unwrap()
        });

        let rows = self.grid.cells.len();
        self.url_ranges.resize(rows, Vec::new());

        for (row_idx, row) in self.grid.cells.iter().enumerate() {
            self.url_ranges[row_idx].clear();
            self.url_row_buf.clear();
            for c in row.iter() {
                self.url_row_buf.push(if c.character == '\0' { ' ' } else { c.character });
            }
            for m in re.find_iter(&self.url_row_buf) {
                let start_col = self.url_row_buf[..m.start()].chars().count();
                let end_col = start_col + m.as_str().chars().count();
                self.url_ranges[row_idx].push((start_col, end_col));
            }
        }
        self.url_ranges.truncate(rows);
    }
}

// ──────────────────────────────────────────────
// Sync thread entry point
// ──────────────────────────────────────────────

fn grid_sync_thread_main(
    thread_handle: Arc<Mutex<Option<std::thread::Thread>>>,
    mut syncer: GridSyncer,
    dirty: Arc<AtomicBool>,
    snapshot: Arc<Mutex<SharedSnapshot>>,
    snapshot_ready: Arc<AtomicBool>,
    waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
    shutdown: Arc<AtomicBool>,
) {
    // Install our thread handle so PTY thread / main thread can unpark us
    {
        let mut guard = thread_handle.lock().unwrap();
        *guard = Some(std::thread::current());
    }

    loop {
        // Process all pending dirty flags before parking
        while dirty.swap(false, Ordering::Relaxed) {
            if shutdown.load(Ordering::Relaxed) {
                return;
            }

            syncer.sync();

            // Copy results into shared snapshot
            {
                let mut snap = snapshot.lock().unwrap();
                snap.grid.clone_from(&syncer.grid);
                snap.inverse_cursor = syncer.inverse_cursor;
                snap.url_ranges.clone_from(&syncer.url_ranges);
                snap.generation = syncer.grid_generation;
                snap.cursor = syncer.cached_cursor;
            }
            snapshot_ready.store(true, Ordering::Relaxed);

            // Wake main thread event loop
            if let Ok(guard) = waker.lock() {
                if let Some(f) = guard.as_ref() {
                    f();
                }
            }
        }

        // Park until PTY thread or main thread unparks us
        std::thread::park();

        if shutdown.load(Ordering::Relaxed) {
            return;
        }
    }
}

// ──────────────────────────────────────────────
// Terminal backend
// ──────────────────────────────────────────────

/// Terminal backend using alacritty_terminal for PTY management and terminal emulation.
pub struct Terminal {
    /// The alacritty terminal emulator state, wrapped in a FairMutex for thread safety
    term: Arc<FairMutex<Term<TermEventListener>>>,
    /// Notifier to send messages to the PTY event loop
    notifier: Notifier,
    /// Cached grid — swapped in from the sync thread's SharedSnapshot
    cached_grid: TerminalGrid,
    /// Detected current working directory (from OSC 7 or fallback)
    current_dir: Option<PathBuf>,
    /// Current column count
    cols: u16,
    /// Current row count
    rows: u16,
    /// The child process ID for CWD detection fallback
    child_pid: Option<u32>,
    /// Atomic flag: sync thread has a new snapshot ready to consume
    snapshot_ready: Arc<AtomicBool>,
    /// Shared snapshot for grid exchange with sync thread
    snapshot: Arc<Mutex<SharedSnapshot>>,
    /// Last INVERSE cell position (read from snapshot)
    inverse_cursor: Option<(u16, u16)>,
    /// Cached cursor state (read from snapshot)
    cached_cursor: CursorState,
    /// Detected URL ranges per row (read from snapshot)
    url_ranges: Vec<Vec<(usize, usize)>>,
    /// Grid generation counter
    grid_generation: u64,
    /// Stay-at-bottom mode (shared with sync thread via atomic)
    stay_at_bottom: Arc<AtomicBool>,
    /// Dark/light mode (shared with sync thread via atomic)
    dark_mode: Arc<AtomicBool>,
    /// Signal to sync thread: dark mode changed, force full re-render
    dark_mode_changed: Arc<AtomicBool>,
    /// Dirty flag (shared with PTY thread and sync thread)
    dirty: Arc<AtomicBool>,
    /// Shared waker callback — installed by main thread, called by sync thread
    waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>>,
    /// Pending PTY resize notification (debounced to avoid SIGWINCH storms)
    pending_pty_resize: Option<(WindowSize, Instant)>,
    /// Handle to sync thread for unparking
    sync_thread_handle: Arc<Mutex<Option<std::thread::Thread>>>,
    /// Shutdown flag for sync thread
    sync_shutdown: Arc<AtomicBool>,
    /// Sync thread join handle (joined on Drop)
    _sync_join: Option<std::thread::JoinHandle<()>>,
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
        let sync_thread_handle: Arc<Mutex<Option<std::thread::Thread>>> = Arc::new(Mutex::new(None));
        let listener = TermEventListener {
            dirty: dirty.clone(),
            pty_writer: pty_writer.clone(),
            sync_thread: sync_thread_handle.clone(),
        };

        let config = TermConfig::default();
        let term = Term::new(config, &term_size, listener.clone());
        let term = Arc::new(FairMutex::new(term));

        // Determine the shell to use
        let shell = Self::detect_shell();

        // Use provided cwd, or fall back to $HOME so .app bundles don't land in /
        let working_directory = cwd.or_else(|| std::env::var("HOME").ok().map(PathBuf::from));
        let mut env = std::collections::HashMap::new();
        env.insert(String::from("TERM"), String::from("xterm-256color"));
        env.insert(String::from("COLORTERM"), String::from("truecolor"));
        env.insert(String::from("PROMPT_EOL_MARK"), String::new());
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
        if let Ok(mut guard) = pty_writer.lock() {
            *guard = Some(Notifier(event_loop.channel()));
        }
        event_loop.spawn();

        // Initialize shared state for the sync thread
        let cached_grid = Self::build_empty_grid(cols, rows);
        let stay_at_bottom = Arc::new(AtomicBool::new(false));
        let dark_mode_flag = Arc::new(AtomicBool::new(dark_mode));
        let dark_mode_changed = Arc::new(AtomicBool::new(false));
        let snapshot_ready = Arc::new(AtomicBool::new(false));
        let sync_shutdown = Arc::new(AtomicBool::new(false));
        let waker: Arc<Mutex<Option<Box<dyn Fn() + Send>>>> = Arc::new(Mutex::new(None));

        let snapshot = Arc::new(Mutex::new(SharedSnapshot {
            grid: Self::build_empty_grid(cols, rows),
            inverse_cursor: None,
            url_ranges: Vec::new(),
            generation: 0,
            cursor: CursorState { row: 0, col: 0, visible: true, shape: CursorShape::Block },
        }));

        // Create the GridSyncer with all sync-related state
        let syncer = GridSyncer {
            term: term.clone(),
            raw_buf: Vec::new(),
            prev_raw_buf: Vec::new(),
            palette_buf: [None; 256],
            grid: Self::build_empty_grid(cols, rows),
            inverse_cursor: None,
            cached_cursor: CursorState { row: 0, col: 0, visible: true, shape: CursorShape::Block },
            url_ranges: Vec::new(),
            grid_generation: 0,
            url_row_buf: String::new(),
            last_url_detect: Instant::now(),
            dark_mode: dark_mode_flag.clone(),
            dark_mode_changed: dark_mode_changed.clone(),
            stay_at_bottom: stay_at_bottom.clone(),
        };

        // Spawn the grid sync thread
        let sync_join = {
            let handle = sync_thread_handle.clone();
            let dirty = dirty.clone();
            let snapshot = snapshot.clone();
            let snapshot_ready = snapshot_ready.clone();
            let waker = waker.clone();
            let shutdown = sync_shutdown.clone();
            std::thread::Builder::new()
                .name("grid-sync".to_string())
                .spawn(move || {
                    grid_sync_thread_main(handle, syncer, dirty, snapshot, snapshot_ready, waker, shutdown);
                })
                .expect("failed to spawn grid sync thread")
        };

        Ok(Terminal {
            term,
            notifier,
            cached_grid,
            current_dir: None,
            cols,
            rows,
            child_pid: Some(child_pid),
            snapshot_ready,
            snapshot,
            inverse_cursor: None,
            cached_cursor: CursorState { row: 0, col: 0, visible: true, shape: CursorShape::Block },
            url_ranges: Vec::new(),
            grid_generation: 0,
            stay_at_bottom,
            dark_mode: dark_mode_flag,
            dark_mode_changed,
            dirty,
            waker,
            pending_pty_resize: None,
            sync_thread_handle,
            sync_shutdown,
            _sync_join: Some(sync_join),
        })
    }

    /// Detect the user's preferred shell
    fn detect_shell() -> String {
        std::env::var("SHELL").unwrap_or_else(|_| {
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

    /// Detect the CWD of the child process using native OS APIs (no subprocess).
    #[cfg(target_os = "macos")]
    pub fn detect_cwd_fallback(&self) -> Option<PathBuf> {
        let pid = self.child_pid? as i32;

        const PROC_PIDVNODEPATHINFO: i32 = 9;
        const BUF_SIZE: usize = 2352;
        const PATH_OFFSET: usize = 152;
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

    /// Unpark the sync thread so it processes pending dirty flags.
    fn notify_sync_thread(&self) {
        if let Ok(guard) = self.sync_thread_handle.lock() {
            if let Some(ref thread) = *guard {
                thread.unpark();
            }
        }
    }

    /// Consume the latest snapshot from the sync thread (if available).
    fn consume_snapshot(&mut self) {
        if !self.snapshot_ready.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut snap) = self.snapshot.lock() {
            std::mem::swap(&mut self.cached_grid, &mut snap.grid);
            self.inverse_cursor = snap.inverse_cursor;
            std::mem::swap(&mut self.url_ranges, &mut snap.url_ranges);
            self.grid_generation = snap.generation;
            self.cached_cursor = snap.cursor;
        }
        self.snapshot_ready.store(false, Ordering::Relaxed);
    }
}

impl Terminal {
    /// Set a waker callback that will be called from the sync thread when a new
    /// grid snapshot is ready. This allows the event loop to sleep with
    /// `ControlFlow::Wait` and be woken up on demand.
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
    #[cfg(target_os = "macos")]
    pub fn is_shell_idle(&self) -> bool {
        let pid = match self.child_pid {
            Some(p) => p,
            None => return false,
        };
        let mut pids = [0i32; 16];
        let ret = unsafe {
            libc::proc_listchildpids(
                pid as i32,
                pids.as_mut_ptr() as *mut libc::c_void,
                (pids.len() * std::mem::size_of::<i32>()) as i32,
            )
        };
        ret <= 0
    }

    #[cfg(not(target_os = "macos"))]
    pub fn is_shell_idle(&self) -> bool {
        let pid = match self.child_pid {
            Some(p) => p,
            None => return false,
        };
        let stat_path = format!("/proc/{}/stat", pid);
        if let Ok(contents) = std::fs::read_to_string(&stat_path) {
            let fields: Vec<&str> = contents.split_whitespace().collect();
            if fields.len() > 7 {
                let pgrp = fields[4].parse::<i32>().unwrap_or(0);
                let tpgid = fields[7].parse::<i32>().unwrap_or(-1);
                return pgrp == tpgid;
            }
        }
        false
    }

    /// Returns true if the sync thread has produced a new snapshot since the
    /// last `process()` call.
    pub fn has_new_output(&self) -> bool {
        self.snapshot_ready.load(Ordering::Relaxed)
    }

    /// Returns the grid generation counter. Increments when grid content changes.
    pub fn grid_generation(&self) -> u64 {
        self.grid_generation
    }

    /// Force a sync_grid cycle for benchmarking purposes.
    /// Sets the dirty flag, wakes the sync thread, and spins until the snapshot is ready.
    #[doc(hidden)]
    pub fn bench_sync_grid(&mut self) {
        self.dirty.store(true, Ordering::Relaxed);
        self.notify_sync_thread();
        // Spin until snapshot is ready
        while !self.snapshot_ready.load(Ordering::Relaxed) {
            std::thread::yield_now();
        }
        self.consume_snapshot();
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

        for abs_line in 0..(history_len + total_lines) {
            let line_idx = Line(abs_line as i32 - history_len as i32);
            let mut row_text = String::with_capacity(cols);
            for col_idx in 0..cols {
                let point = Point::new(line_idx, Column(col_idx));
                let c = grid[point].c;
                row_text.push(if c == '\0' { ' ' } else { c });
            }

            let row_lower = row_text.to_lowercase();
            let mut start = 0;
            while let Some(byte_pos) = row_lower[start..].find(&query_lower) {
                let byte_col = start + byte_pos;
                let char_col = row_text[..byte_col].chars().count();
                results.push((abs_line, char_col, query_char_len));
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

    /// Check if the terminal has bracketed paste mode enabled.
    pub fn is_bracketed_paste_mode(&self) -> bool {
        let term = self.term.lock();
        term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// Set dark/light mode for the terminal color palette.
    /// Signals the sync thread to force a full grid re-render.
    pub fn set_dark_mode(&mut self, dark: bool) {
        if self.dark_mode.load(Ordering::Relaxed) != dark {
            self.dark_mode.store(dark, Ordering::Relaxed);
            self.dark_mode_changed.store(true, Ordering::Relaxed);
            self.dirty.store(true, Ordering::Relaxed);
            self.notify_sync_thread();
        }
    }

    /// Enter stay-at-bottom mode: every sync_grid will scroll to bottom until
    /// the user explicitly scrolls away via scroll_display().
    pub fn request_scroll_to_bottom(&mut self) {
        self.stay_at_bottom.store(true, Ordering::Relaxed);
        self.dirty.store(true, Ordering::Relaxed);
        self.notify_sync_thread();
    }

    /// Scroll the terminal display by the given delta (positive = scroll up into history).
    /// Cancels stay-at-bottom mode since the user is explicitly scrolling.
    pub fn scroll_display(&mut self, delta: i32) {
        self.stay_at_bottom.store(false, Ordering::Relaxed);

        let mut term = self.term.lock();
        let old_offset = term.grid().display_offset();
        term.scroll_display(Scroll::Delta(delta));
        let new_offset = term.grid().display_offset();
        drop(term);

        if old_offset != new_offset {
            self.dirty.store(true, Ordering::Relaxed);
            self.notify_sync_thread();
        }
    }
}

impl TerminalBackend for Terminal {
    fn write(&mut self, data: &[u8]) {
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

        // Consume the latest snapshot from the sync thread (cheap: just pointer swaps)
        self.consume_snapshot();
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

        {
            let mut term = self.term.lock();
            term.resize(term_size);
        }

        // Debounce PTY resize notification (SIGWINCH) to avoid prompt artifacts
        self.pending_pty_resize = Some((window_size, Instant::now()));

        // Trigger a sync so the grid reflects the new dimensions promptly
        self.dirty.store(true, Ordering::Relaxed);
        self.notify_sync_thread();
    }

    fn cwd(&self) -> Option<PathBuf> {
        self.current_dir.clone()
    }

    fn cursor(&self) -> CursorState {
        self.cached_cursor
    }
}

/// Wait for a child process to exit after SIGHUP, polling with `waitpid`.
/// If the child doesn't exit within 200ms, escalate to SIGKILL.
fn wait_for_child_exit(pid: u32) {
    use std::time::{Duration, Instant};

    let deadline = Instant::now() + Duration::from_millis(200);
    loop {
        let ret = unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
        // ret > 0: child exited; ret == -1: ECHILD (already reaped)
        if ret != 0 {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }

    // Child didn't exit in time — escalate to SIGKILL
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
    let kill_deadline = Instant::now() + Duration::from_millis(50);
    loop {
        let ret = unsafe { libc::waitpid(pid as i32, std::ptr::null_mut(), libc::WNOHANG) };
        if ret != 0 || Instant::now() >= kill_deadline {
            return;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // Send SIGHUP to the child process group so the shell can run trap
        // handlers and clean up (e.g. pyenv rehash lock files).  Without this,
        // closing a PTY fd kills the shell instantly and leaves stale locks.
        if let Some(pid) = self.child_pid {
            unsafe {
                // Negative PID targets the entire process group
                libc::kill(-(pid as i32), libc::SIGHUP);
            }
            // Poll waitpid until the child exits or 200ms deadline.
            // This ensures cleanup handlers (e.g. `rm -f .pyenv-shim`) finish
            // before we close the PTY fd, preventing stale lock files.
            wait_for_child_exit(pid);
        }

        // Signal the sync thread to shut down and wait for it
        self.sync_shutdown.store(true, Ordering::Relaxed);
        self.notify_sync_thread();
        if let Some(handle) = self._sync_join.take() {
            let _ = handle.join();
        }

        // Signal the PTY event loop to shut down
        #[allow(unused)]
        let _ = self.notifier.0.send(Msg::Shutdown);
    }
}

mod tests;
