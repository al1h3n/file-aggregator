#!/bin/bash
set -e

echo "=== File Aggregation Performance Benchmark ==="
echo "Test case: 20 files × 10MB each (200MB total)"
echo ""

# Create test directory
TEST_DIR=".bench_test_files"
rm -rf "$TEST_DIR"
mkdir -p "$TEST_DIR"

echo "Generating test files..."
# Generate 20 files of 10MB each
for i in {0..19}; do
    dd if=/dev/urandom of="$TEST_DIR/test_file_$i.txt" bs=1M count=10 status=none
done

echo "Files generated. Starting benchmark..."
echo ""

# Build the project in release mode
cargo build --release

# Run 5 iterations and measure time
TOTAL_TIME=0
ITERATIONS=5

for i in $(seq 1 $ITERATIONS); do
    echo "Run $i/$ITERATIONS..."
    START=$(date +%s%N)
    
    # Run aggregation
    ./target/release/file-aggregator add "$TEST_DIR"/*.txt -o /dev/null 2>&1 || true
    
    END=$(date +%s%N)
    DURATION=$((($END - $START) / 1000000)) # Convert to milliseconds
    echo "  Time: ${DURATION}ms"
    TOTAL_TIME=$(($TOTAL_TIME + $DURATION))
done

AVG_TIME=$(($TOTAL_TIME / $ITERATIONS))

echo ""
echo "=== Results ==="
echo "Average time: ${AVG_TIME}ms"
echo "Total data: 200MB"
echo "Throughput: $((200000 / $AVG_TIME))MB/s"

# Cleanup
rm -rf "$TEST_DIR"

echo ""
echo "Benchmark complete."
