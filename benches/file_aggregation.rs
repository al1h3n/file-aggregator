//! Criterion benchmark: `cargo bench --bench file_aggregation`.
//!
//! Aggregates 20 code-like files of 1 MB (newlines, tabs, a little Unicode) so the
//! sanitizer and the parallel reader both do realistic work.

use criterion::{Criterion, Throughput, criterion_group, criterion_main};
use std::hint::black_box;
use std::path::PathBuf;

const SNIPPET: &str = "fn main() {\n\tlet greeting = \"héllo — wörld\";\n\tprintln!(\"{greeting} {}\", 42);\n}\n\n";

fn generate_test_files(dir: &std::path::Path, count: usize, size: usize) -> Vec<PathBuf> {
    let content = SNIPPET.repeat(size / SNIPPET.len());
    (0..count)
        .map(|i| {
            let path = dir.join(format!("test_file_{i}.rs"));
            std::fs::write(&path, &content).expect("write test file");
            path
        })
        .collect()
}

fn benchmark_aggregation(c: &mut Criterion) {
    let dir = tempfile::tempdir().expect("temp dir");
    let paths = generate_test_files(dir.path(), 20, 1024 * 1024);
    let bytes: u64 = paths.iter().map(|p| std::fs::metadata(p).unwrap().len()).sum();

    let mut group = c.benchmark_group("file_aggregation");
    group.throughput(Throughput::Bytes(bytes));
    group.bench_function("20x1MB_code_files", |b| b.iter(|| black_box(file_aggregator::aggregator::aggregate_files(black_box(&paths)).unwrap())));
    group.finish();
}

criterion_group!(benches, benchmark_aggregation);
criterion_main!(benches);
