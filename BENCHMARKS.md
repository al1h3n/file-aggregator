# Performance Benchmarks

## Test Environment

**Hardware Specifications:**
- **CPU:** Intel Core i7-12700F (12th Gen, 12 cores, 20 threads)
- **RAM:** 32 GB (34,220,879,872 bytes)
- **Storage:** NVMe SSD 2TB
- **OS:** Windows 11

## Benchmark Methodology

### Test Case: Parallel File I/O Performance

**Scenario:** Aggregate 20 files × 10MB each (200MB total)

**What We Measure:**
1. Total aggregation time (file reading + formatting)
2. Throughput (MB/s)
3. Comparison: Sequential vs Parallel I/O

**Expected Results:**
- Sequential I/O: ~2-4 seconds (baseline)
- Parallel I/O (stdlib threads): 4-6x speedup → ~500-1000ms
- Actual speedup depends on disk type and thread count

## Running Benchmarks

### Option 1: PowerShell Script (Windows)

```powershell
.\benchmark_test.ps1
```

**What it does:**
- Generates 20 test files (10MB each)
- Runs 5 iterations
- Reports average time, min, max, throughput
- Auto-cleanup

### Option 2: Bash Script (Linux/macOS)

```bash
./benchmark_test.sh
```

Same functionality as PowerShell version.

### Option 3: Rust Integration Test

```bash
cargo test --test benchmark -- --ignored --nocapture
```

**Advantages:**
- Tests the actual aggregation function directly
- No binary build required (test framework)
- More accurate (no CLI overhead)

### Option 4: Manual Benchmark

```bash
# 1. Generate test files
mkdir test_data
for i in {0..19}; do
  dd if=/dev/urandom of=test_data/file_$i.txt bs=1M count=10
done

# 2. Time the aggregation
time ./file-aggregator add test_data/*.txt -o output.md

# 3. Cleanup
rm -rf test_data output.md
```

## Performance Results

### Current Implementation: Parallel I/O (stdlib threads)

Once built, expected results:

| Metric | Sequential | Parallel (20 threads) | Speedup |
|--------|-----------|----------------------|---------|
| Time | ~2000ms | ~400-500ms | 4-5x |
| Throughput | ~100 MB/s | ~400-500 MB/s | 4-5x |

**Note:** Results will vary based on:
- Storage type (NVMe > SATA SSD > HDD)
- OS disk caching
- Available CPU cores
- System load

### Implementation Details

The parallel I/O uses:
- `std::thread::spawn()` - one thread per file
- `Arc<Mutex<Vec>>` - thread-safe result collection
- Order preservation via index tracking

**Code location:** `src/aggregator.rs` lines 23-81

### Comparison: Before vs After

| Version | Implementation | Expected Time (200MB) |
|---------|---------------|----------------------|
| Before | Sequential I/O | ~2000ms |
| After (Current) | Parallel (stdlib threads) | ~400-500ms |

### Real-World Performance Factors

1. **SSD vs HDD:**
   - NVMe SSD: Full parallel benefit (4-6x)
   - SATA SSD: Good parallel benefit (3-4x)
   - HDD: Limited benefit due to seek time

2. **File Count:**
   - More files = better parallelization
   - Overhead for <5 files
   - Optimal: 10-50 files

3. **File Size:**
   - Large files (>5MB): Parallel shines
   - Small files (<1MB): Overhead dominates

## How to Interpret Results

**Good Performance:**
- Throughput > 300 MB/s on SSD
- Speedup > 3x vs sequential
- Consistent times across iterations

**Poor Performance Indicators:**
- Throughput < 100 MB/s on SSD
- High variance between runs
- No speedup vs sequential

**Troubleshooting:**
- Antivirus scanning files: Disable temporarily
- Disk cache cold: Run warm-up iteration
- High system load: Close background apps

## Benchmark Scripts

### benchmark_test.ps1
Comprehensive PowerShell benchmark with automatic test file generation and cleanup.

### benchmark_test.sh
Linux/macOS version using dd for file generation.

### tests/benchmark.rs
Rust integration test for direct function-level benchmarking.

## Future Benchmarks

Additional test cases to consider:

1. **Varying File Counts:** 5, 10, 20, 50, 100 files
2. **Varying File Sizes:** 1MB, 5MB, 10MB, 50MB
3. **Mixed File Types:** Text, binary, compressed
4. **Memory Usage:** Track peak RAM during aggregation
5. **CPU Utilization:** Monitor thread efficiency

## Contributing Benchmark Results

When submitting benchmark results, please include:

1. Hardware specs (CPU, RAM, storage type)
2. OS and version
3. Rust version (`rustc --version`)
4. Full benchmark output
5. Any unusual system conditions

Format:
```
**System:** Intel i7-12700F, 32GB RAM, NVMe SSD, Windows 11
**Rust:** rustc 1.75.0
**Results:** 20 files × 10MB = 450ms average (444 MB/s throughput)
```
