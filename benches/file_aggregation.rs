use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use std::fs;
use std::path::PathBuf;
use std::io::Write;

// Import the aggregator module
// Note: This assumes the aggregator is exposed as a public module
// We'll need to adjust based on actual project structure

fn generate_test_files(count: usize, size_mb: usize) -> Vec<PathBuf> {
    let test_dir = PathBuf::from(".bench_test_files");
    
    // Clean up previous run
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).expect("Failed to create test directory");
    
    let mut paths = Vec::new();
    let content = "x".repeat(size_mb * 1024 * 1024); // MB of data
    
    for i in 0..count {
        let path = test_dir.join(format!("test_file_{}.txt", i));
        let mut file = fs::File::create(&path).expect("Failed to create test file");
        file.write_all(content.as_bytes()).expect("Failed to write test file");
        paths.push(path);
    }
    
    paths
}

fn cleanup_test_files() {
    let test_dir = PathBuf::from(".bench_test_files");
    let _ = fs::remove_dir_all(&test_dir);
}

fn benchmark_aggregation(c: &mut Criterion) {
    let mut group = c.benchmark_group("file_aggregation");
    
    // Test case from Patrick: 20 files × 10MB each
    let paths = generate_test_files(20, 10);
    
    group.bench_function("20x10MB_files", |b| {
        b.iter(|| {
            // Note: We need to call the actual aggregation function here
            // This is a placeholder that needs to be replaced with the actual call
            black_box(&paths);
        });
    });
    
    group.finish();
    cleanup_test_files();
}

criterion_group!(benches, benchmark_aggregation);
criterion_main!(benches);
