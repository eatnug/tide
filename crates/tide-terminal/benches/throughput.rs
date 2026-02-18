//! Benchmarks: VTE parsing throughput and sync_grid performance.
//!
//! Run with: cargo bench --package tide-terminal

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use tide_terminal::Terminal;

// ── Data generators ──

/// Pure ASCII text (simulates `cat large_file`).
fn gen_ascii(size: usize) -> Vec<u8> {
    let line = b"abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789\r\n";
    line.iter().cycle().take(size).copied().collect()
}

/// 256-color ANSI escape sequences.
fn gen_ansi_color(size: usize) -> Vec<u8> {
    let mut buf = Vec::with_capacity(size);
    let mut i: u8 = 0;
    while buf.len() < size {
        // \x1b[38;5;Nm — set foreground to 256-color N
        let seq = format!("\x1b[38;5;{}mX", i);
        buf.extend_from_slice(seq.as_bytes());
        i = i.wrapping_add(1);
    }
    buf.truncate(size);
    buf
}

/// Fast scrolling (newline-heavy output).
fn gen_scroll(size: usize) -> Vec<u8> {
    let line = b"line\n";
    line.iter().cycle().take(size).copied().collect()
}

/// Unicode CJK wide characters (Korean/Chinese/Japanese).
fn gen_unicode(size: usize) -> Vec<u8> {
    let text = "가나다라마바사아자차카타파하 你好世界 こんにちは\r\n";
    let bytes = text.as_bytes();
    bytes.iter().cycle().take(size).copied().collect()
}

// ── VTE throughput benchmarks ──

fn bench_vte_throughput(c: &mut Criterion) {
    let sizes: &[(usize, &str)] = &[
        (10 * 1024, "10KB"),
        (100 * 1024, "100KB"),
        (1024 * 1024, "1MB"),
    ];

    let generators: &[(&str, fn(usize) -> Vec<u8>)] = &[
        ("ascii", gen_ascii),
        ("ansi_color", gen_ansi_color),
        ("scroll", gen_scroll),
        ("unicode", gen_unicode),
    ];

    for &(gen_name, gen_fn) in generators {
        let mut group = c.benchmark_group(format!("vte_throughput/{}", gen_name));

        for &(size, label) in sizes {
            let data = gen_fn(size);
            group.throughput(Throughput::Bytes(data.len() as u64));

            group.bench_with_input(BenchmarkId::new("parse", label), &data, |b, data| {
                // Create a fresh terminal for each benchmark iteration group.
                // Terminal::new() spawns a PTY — we only use it as a Term host.
                let terminal = Terminal::new(120, 40).expect("create terminal");

                b.iter(|| {
                    terminal.bench_write_to_term(black_box(data));
                });
            });
        }
        group.finish();
    }
}

// ── sync_grid benchmarks ──

fn bench_sync_grid(c: &mut Criterion) {
    let mut group = c.benchmark_group("sync_grid");

    // Full redraw: first sync after populating the screen
    group.bench_function("full_redraw/120x40", |b| {
        let mut terminal = Terminal::new(120, 40).expect("create terminal");

        // Fill the terminal with content
        let fill = gen_ascii(120 * 40);
        terminal.bench_write_to_term(&fill);

        b.iter(|| {
            terminal.bench_sync_grid();
            black_box(());
        });
    });

    // Partial update: only a few lines change
    group.bench_function("partial_update/120x40", |b| {
        let mut terminal = Terminal::new(120, 40).expect("create terminal");

        // Fill and sync once to establish baseline
        let fill = gen_ascii(120 * 40);
        terminal.bench_write_to_term(&fill);
        terminal.bench_sync_grid();

        // Write a small change (one line)
        let small_change = b"partial update line\r\n";

        b.iter(|| {
            terminal.bench_write_to_term(small_change);
            terminal.bench_sync_grid();
            black_box(());
        });
    });

    // No change: diff fast-path (nothing changed since last sync)
    group.bench_function("no_change/120x40", |b| {
        let mut terminal = Terminal::new(120, 40).expect("create terminal");

        // Fill and sync to establish baseline
        let fill = gen_ascii(120 * 40);
        terminal.bench_write_to_term(&fill);
        terminal.bench_sync_grid();

        b.iter(|| {
            terminal.bench_sync_grid();
            black_box(());
        });
    });

    group.finish();
}

criterion_group!(benches, bench_vte_throughput, bench_sync_grid);
criterion_main!(benches);
