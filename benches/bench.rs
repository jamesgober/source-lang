//! Benchmarks for the hot path: resolving a global position to its source.
//!
//! `SourceMap::locate` is the operation a diagnostic renderer runs for every span
//! it reports, so its cost is what the `O(log files)` design exists to bound.
//! These benchmarks measure it against a growing number of sources to confirm the
//! lookup scales with the logarithm of the file count, not linearly.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use source_lang::{BytePos, SourceMap};
use std::hint::black_box;

/// Builds a map of `count` sources, each `len` bytes long.
fn build_map(count: usize, len: usize) -> SourceMap {
    let mut map = SourceMap::with_capacity(count);
    let text = "x".repeat(len);
    for i in 0..count {
        map.add(format!("f{i}.src"), text.as_str())
            .expect("benchmark inputs fit");
    }
    map
}

/// A fixed set of probe positions spread across the whole global space, computed
/// once so the measured loop does no arithmetic of its own.
fn probes(span_end: u32, n: usize) -> Vec<BytePos> {
    (0..n)
        .map(|k| {
            // A coprime stride scatters the probes evenly without clustering.
            let step = (k as u64).wrapping_mul(2_654_435_761) % u64::from(span_end);
            BytePos::new(step as u32)
        })
        .collect()
}

fn bench_locate(c: &mut Criterion) {
    let mut group = c.benchmark_group("locate");
    let file_len = 256;

    for &count in &[16usize, 256, 4096, 65_536] {
        let map = build_map(count, file_len);
        let span_end = (count * file_len) as u32;
        let probe_set = probes(span_end, 1024);

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                for &pos in &probe_set {
                    black_box(map.locate(black_box(pos)));
                }
            });
        });
    }

    group.finish();
}

/// Resolving a global position all the way to line/column. This is `locate`
/// followed by a line-index build over the located source, so the cost is
/// dominated by the per-source scan; the benchmark fixes the source length and
/// varies the file count to show the lookup itself stays sub-linear.
fn bench_line_col(c: &mut Criterion) {
    let mut group = c.benchmark_group("line_col");
    let file_len = 256;

    for &count in &[16usize, 256, 4096, 65_536] {
        let map = build_map(count, file_len);
        let span_end = (count * file_len) as u32;
        let probe_set = probes(span_end, 1024);

        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _| {
            b.iter(|| {
                for &pos in &probe_set {
                    black_box(map.line_col(black_box(pos)));
                }
            });
        });
    }

    group.finish();
}

criterion_group!(benches, bench_locate, bench_line_col);
criterion_main!(benches);
