//! Benchmark: git status lookup for file tree directories.
//!
//! Compares the old O(n) linear scan approach vs the new O(1) HashMap lookup
//! at various file counts (100, 1000, 5000).

use std::collections::HashMap;
use std::path::PathBuf;

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use tide_core::FileGitStatus;

// ── Helpers ──

fn merge_git_status(a: FileGitStatus, b: FileGitStatus) -> FileGitStatus {
    use FileGitStatus::*;
    match (a, b) {
        (Conflict, _) | (_, Conflict) => Conflict,
        (Modified, _) | (_, Modified) => Modified,
        _ => a,
    }
}

/// Generate a synthetic git status map with `n` files spread across directories.
fn generate_status_map(n: usize, root: &PathBuf) -> HashMap<PathBuf, FileGitStatus> {
    let statuses = [
        FileGitStatus::Modified,
        FileGitStatus::Added,
        FileGitStatus::Untracked,
    ];
    let mut map = HashMap::with_capacity(n);
    for i in 0..n {
        let dir_idx = i % 50; // spread across 50 directories
        let path = root
            .join(format!("dir{}", dir_idx))
            .join(format!("file{}.rs", i));
        map.insert(path, statuses[i % statuses.len()]);
    }
    map
}

/// Generate directory paths to query (simulates rendering 50 directory entries).
fn generate_dir_queries(root: &PathBuf) -> Vec<PathBuf> {
    (0..50).map(|i| root.join(format!("dir{}", i))).collect()
}

/// Pre-compute directory status cache from file status map (the new approach).
fn precompute_dir_status(
    status_map: &HashMap<PathBuf, FileGitStatus>,
    tree_root: &PathBuf,
) -> HashMap<PathBuf, FileGitStatus> {
    let mut dir_status: HashMap<PathBuf, FileGitStatus> = HashMap::new();
    for (path, &status) in status_map {
        let mut ancestor = path.parent();
        while let Some(dir) = ancestor {
            if dir < tree_root.as_path() {
                break;
            }
            let entry = dir_status.entry(dir.to_path_buf()).or_insert(status);
            *entry = merge_git_status(*entry, status);
            if dir == tree_root.as_path() {
                break;
            }
            ancestor = dir.parent();
        }
    }
    dir_status
}

// ── Old approach: linear scan per directory ──

fn old_lookup_dir_status(
    dir_path: &PathBuf,
    status_map: &HashMap<PathBuf, FileGitStatus>,
) -> Option<FileGitStatus> {
    let mut best: Option<FileGitStatus> = None;
    for (path, status) in status_map {
        if path.starts_with(dir_path) {
            best = Some(match (best, status) {
                (None, s) => *s,
                (Some(FileGitStatus::Conflict), _) => FileGitStatus::Conflict,
                (_, FileGitStatus::Conflict) => FileGitStatus::Conflict,
                (Some(FileGitStatus::Modified), _) => FileGitStatus::Modified,
                (_, FileGitStatus::Modified) => FileGitStatus::Modified,
                (Some(existing), _) => existing,
            });
        }
    }
    best
}

// ── Benchmarks ──

fn bench_dir_lookup(c: &mut Criterion) {
    let root = PathBuf::from("/tmp/bench_repo");

    let mut group = c.benchmark_group("dir_git_status_lookup");
    for &n in &[100, 1000, 5000] {
        let status_map = generate_status_map(n, &root);
        let dir_cache = precompute_dir_status(&status_map, &root);
        let dirs = generate_dir_queries(&root);

        group.bench_with_input(BenchmarkId::new("old_linear_scan", n), &n, |b, _| {
            b.iter(|| {
                for dir in &dirs {
                    black_box(old_lookup_dir_status(dir, &status_map));
                }
            });
        });

        group.bench_with_input(BenchmarkId::new("new_hashmap_lookup", n), &n, |b, _| {
            b.iter(|| {
                for dir in &dirs {
                    black_box(dir_cache.get(dir).copied());
                }
            });
        });
    }
    group.finish();
}

fn bench_precompute(c: &mut Criterion) {
    let root = PathBuf::from("/tmp/bench_repo");

    let mut group = c.benchmark_group("dir_git_status_precompute");
    for &n in &[100, 1000, 5000] {
        let status_map = generate_status_map(n, &root);

        group.bench_with_input(BenchmarkId::new("precompute_cost", n), &n, |b, _| {
            b.iter(|| {
                black_box(precompute_dir_status(&status_map, &root));
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_dir_lookup, bench_precompute);
criterion_main!(benches);
