//! End-to-end timing: `cargo test --release --test benchmark -- --ignored --nocapture`.
//!
//! 20 files × 10 MB (200 MB, the total limit). Two data sets:
//! - `plain`: one repeated letter, the original benchmark (best case for every scan);
//! - `code`: code-like text with newlines, tabs and some Unicode (what users really paste).

use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

const SNIPPET: &str = "fn main() {\n\tlet greeting = \"héllo — wörld\";\n\tprintln!(\"{greeting} {}\", 42);\n}\n\n";

fn run(name: &str, content: &str) {
    let dir = tempfile::tempdir().expect("temp dir");
    let paths: Vec<PathBuf> = (0..20)
        .map(|i| {
            let path = dir.path().join(format!("test_{i}.txt"));
            fs::write(&path, content).expect("write test file");
            path
        })
        .collect();
    let total_mb = (content.len() * paths.len()) as f64 / (1024.0 * 1024.0);

    let times: Vec<Duration> = (0..5)
        .map(|_| {
            let start = Instant::now();
            let out = file_aggregator::aggregator::aggregate_files(&paths).expect("aggregation failed");
            let elapsed = start.elapsed();
            std::hint::black_box(out);
            elapsed
        })
        .collect();
    let avg = times.iter().sum::<Duration>() / times.len() as u32;
    let min = times.iter().min().unwrap();
    let max = times.iter().max().unwrap();
    println!(
        "{name:>5}: {total_mb:.0} MB  avg {:.1} ms  min {:.1} ms  max {:.1} ms  {:.0} MB/s",
        avg.as_secs_f64() * 1e3,
        min.as_secs_f64() * 1e3,
        max.as_secs_f64() * 1e3,
        total_mb / avg.as_secs_f64()
    );
}

#[test]
#[ignore] // Run with: cargo test --release --test benchmark -- --ignored --nocapture
fn benchmark_parallel_io() {
    const SIZE: usize = 10 * 1024 * 1024;
    println!("=== File Aggregation Benchmark: 20 files x 10 MB, 5 runs, output sanitized ===");
    run("plain", &"x".repeat(SIZE));
    run("code", &SNIPPET.repeat(SIZE / SNIPPET.len()));
}
