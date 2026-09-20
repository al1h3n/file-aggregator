# File Aggregation Performance Benchmark
Write-Host "=== File Aggregation Performance Benchmark ===" -ForegroundColor Cyan
Write-Host "Test case: 20 files × 10MB each (200MB total)"
Write-Host ""

$TEST_DIR = ".bench_test_files"

# Clean up previous test files
if (Test-Path $TEST_DIR) {
    Remove-Item -Recurse -Force $TEST_DIR
}
New-Item -ItemType Directory -Path $TEST_DIR | Out-Null

Write-Host "Generating test files..."
# Generate 20 files of 10MB each
for ($i = 0; $i -lt 20; $i++) {
    $bytes = New-Object byte[] (10MB)
    (New-Object Random).NextBytes($bytes)
    [IO.File]::WriteAllBytes("$TEST_DIR/test_file_$i.txt", $bytes)
}

Write-Host "Files generated. Building project in release mode..."
cargo build --release 2>&1 | Out-Null

Write-Host "Starting benchmark..."
Write-Host ""

$ITERATIONS = 5
$times = @()

for ($run = 1; $run -le $ITERATIONS; $run++) {
    Write-Host "Run $run/$ITERATIONS..."
    
    $startTime = Get-Date
    
    # Run aggregation
    & .\target\release\file-aggregator.exe add (Get-ChildItem "$TEST_DIR\*.txt").FullName -o "nul" 2>&1 | Out-Null
    
    $endTime = Get-Date
    $duration = ($endTime - $startTime).TotalMilliseconds
    
    Write-Host "  Time: $([math]::Round($duration, 2))ms"
    $times += $duration
}

$avgTime = ($times | Measure-Object -Average).Average

Write-Host ""
Write-Host "=== Results ===" -ForegroundColor Green
Write-Host "Average time: $([math]::Round($avgTime, 2))ms"
Write-Host "Total data: 200MB"
Write-Host "Throughput: $([math]::Round(200000 / $avgTime, 2))MB/s"

# Cleanup
Remove-Item -Recurse -Force $TEST_DIR

Write-Host ""
Write-Host "Benchmark complete." -ForegroundColor Green
