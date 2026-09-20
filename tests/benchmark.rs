use std::fs;
use std::path::PathBuf;
use std::io::Write;
use std::time::Instant;

#[test]
#[ignore] // Run with: cargo test --test benchmark -- --ignored --nocapture
fn benchmark_parallel_io() {
    // Setup: Create test directory with 20 x 10MB files
    let test_dir = PathBuf::from(".bench_test");
    let _ = fs::remove_dir_all(&test_dir);
    fs::create_dir_all(&test_dir).expect("Failed to create test dir");
    
    println!("=== File Aggregation Performance Benchmark ===");
    println!("Test case: 20 files × 10MB each (200MB total)");
    println!();
    
    // Generate test files
    println!("Generating test files...");
    let mut paths = Vec::new();
    let content = "x".repeat(10 * 1024 * 1024); // 10MB
    
    for i in 0..20 {
        let path = test_dir.join(format!("test_{}.txt", i));
        let mut file = fs::File::create(&path).expect("Failed to create file");
        file.write_all(content.as_bytes()).expect("Failed to write");
        paths.push(path);
    }
    
    println!("Files generated. Running benchmark...");
    println!();
    
    // Run benchmark iterations
    let iterations = 5;
    let mut times = Vec::new();
    
    for run in 1..=iterations {
        let start = Instant::now();
        
        // Call the aggregation function
        let result = file_aggregator::aggregator::aggregate_files(&paths);
        
        let duration = start.elapsed();
        let ms = duration.as_millis();
        
        println!("Run {}/{}: {}ms", run, iterations, ms);
        
        assert!(result.is_ok(), "Aggregation failed: {:?}", result.err());
        times.push(ms);
    }
    
    // Calculate statistics
    let avg = times.iter().sum::<u128>() / times.len() as u128;
    let min = *times.iter().min().unwrap();
    let max = *times.iter().max().unwrap();
    
    println!();
    println!("=== Results ===");
    println!("Average time: {}ms", avg);
    println!("Min time: {}ms", min);
    println!("Max time: {}ms", max);
    println!("Total data: 200MB");
    println!("Throughput: {:.2}MB/s", 200000.0 / avg as f64);
    println!();
    
    // Cleanup
    let _ = fs::remove_dir_all(&test_dir);
    
    println!("Benchmark complete.");
    println!();
    println!("Note: Parallel I/O using stdlib threads implemented in aggregator.rs");
    println!("Expected speedup: 4-6x compared to sequential I/O");
}
