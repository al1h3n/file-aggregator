# Performance Benchmarks

## Test Environment (September 2026 results)

The v0.2.0 comparison further down was measured on a different machine; its setup is listed
with it.

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
cargo test --release --test benchmark -- --ignored --nocapture
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

### v0.2.0 vs v0.1.0, same machine (November 3, 2026)

Both versions measured back to back with the same data: 20 files × 10 MB, 5 runs, files
in the OS page cache. Both include sanitizing the output (v0.1.0 did that as a second pass
over the whole result in the CLI/GUI; v0.2.0 does it per file inside the parallel readers).

| Data | v0.1.0 | v0.2.0 | Speedup |
|---|---|---|---|
| `plain` (one repeated letter) | 555 ms · 360 MB/s | 191–202 ms · ~1,020 MB/s | **~2.8×** |
| `code` (newlines, tabs, some Unicode) | 603–622 ms · ~330 MB/s | 223–244 ms · ~860 MB/s | **~2.6×** |
| Peak memory, whole run | 959 MB | 423 MB | **−56 %** |

**Test configuration:** cloud VM, 4 vCPUs, 15 GB RAM, Linux 6.18, rustc 1.99.1.
`cargo test --release --test benchmark -- --ignored --nocapture` for v0.2.0; the same
loop calling `aggregate_files` + `strip_control_chars` on a v0.1.0 checkout.

### Where the time went, and what changed

| v0.1.0 | v0.2.0 |
|---|---|
| Each file canonicalized twice, stat-ed 3 times and opened 3–4 times (`infer`'s 8 KB sniff, a 4-byte header read, another for executables, then the full read) | Validated (canonicalize + one stat), then opened and read **once**; type detection runs on the bytes in memory |
| One OS thread per file (1,000 files = 1,000 threads), results through `Arc<Mutex<Vec>>` | rayon's pool, one thread per core; `collect` keeps the order, no locks |
| 200 MB limit checked after every file was already read | Sizes summed from metadata first: fails before reading a byte |
| `format!` per file, then appended (an extra copy of every file) | One output `String` allocated at its final size |
| Sanitizing walked the whole output char by char, then copied it | Per file, in parallel; 32-byte chunks with a vectorizable "all printable?" test; clean text isn't copied at all |

**Code:** `src/aggregator.rs` (`aggregate_files_with_prompt`), `src/security.rs`
(`read_file_safe`, `strip_control_chars_cow`).

### Earlier result (v0.1.0, September 20, 2026)

| Metric | Value |
|--------|-------|
| Average Time | 238ms |
| Min Time | 230ms |
| Max Time | 253ms |
| Total Data | 200MB (20 files × 10MB) |
| **Throughput** | **840.34 MB/s** |

Intel Core i7-12700F, 32 GB RAM, NVMe SSD, Windows 11, rustc 1.97.1. This measured
`aggregate_files` alone, without the sanitizing pass that ran afterwards, so it isn't
comparable with the table above.

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
Rust integration test for direct function-level benchmarking (`plain` and `code` data sets).

### benches/file_aggregation.rs
Criterion benchmark (20 × 1 MB code-like files): `cargo bench --bench file_aggregation`.

## Future Benchmarks

Additional test cases to consider:

1. **Varying File Counts:** 5, 10, 20, 50, 100 files
2. **Varying File Sizes:** 1MB, 5MB, 10MB, 50MB
3. **Mixed File Types:** Text, binary, compressed
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
