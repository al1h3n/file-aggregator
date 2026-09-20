# File Aggregator

Secure command-line and GUI tool for aggregating multiple files into a single markdown document. Built with Rust for cross-platform reliability and memory safety.

## What It Does

File Aggregator takes multiple files from your filesystem and combines them into a single markdown file with proper formatting, security controls, and optional clipboard output. Each file's content is preserved in fenced code blocks with syntax highlighting support.

## Features

- **Dual Interface**: GUI with drag-and-drop support + full-featured CLI
- **Cross-Platform**: Windows, macOS and Linux support, single binary with no runtime dependencies
- **Security Controls**:
  - Path validation with canonical resolution (prevents traversal attacks)
  - Per-file size limits (50 MB max)
  - Total aggregation size limits (200 MB max)
  - Binary file detection via magic bytes (not extensions)
  - Executable warning prompts
  - Control character stripping from output
- **Binary File Support**: Automatically encodes binary files to base64
- **Smart Output**: Markdown-formatted with file paths and syntax-highlighted code blocks
- **Clipboard Integration**: Optional copy-to-clipboard with size guards

## Build Instructions

### Prerequisites

- Rust 1.70 or later ([install via rustup](https://rustup.rs/))

### Build from Source

```bash
# Clone the repository
git clone <repository-url>
cd file-aggregator

# Build release binary
cargo build --release

# Binary will be at: target/release/file-aggregator
```

### Platform-Specific Builds

```bash
# macOS (Apple Silicon)
cargo build --release --target aarch64-apple-darwin

# macOS (Intel)
cargo build --release --target x86_64-apple-darwin

# Linux
cargo build --release --target x86_64-unknown-linux-gnu
```

### Building on Windows

**Prerequisites:**
- Visual Studio 2022 Build Tools with "Desktop development with C++" workload
- Rust toolchain (rustup will auto-detect MSVC)

**Important:** Ensure MSVC `link.exe` is in PATH before Git's Unix `link.exe`.

**Verify toolchain:**
```powershell
# Should show MSVC linker, not Git's Unix link
where.exe link
# First result should be: C:\Program Files\Microsoft Visual Studio\...
```

**Build:**
```powershell
cargo build --release --target x86_64-pc-windows-msvc
```

**Troubleshooting:**
If you see "link: missing operand" error, your PATH has Git's link.exe before MSVC's. Fix by:
1. Temporarily removing Git from PATH, or
2. Running from "x64 Native Tools Command Prompt for VS 2022"

Expected binary size: 5-8 MB per platform.

## Installation

### Download Pre-Built Binary (Recommended)

**Windows:**
1. Download `file-aggregator-windows.exe` from [Releases](https://github.com/your-username/file-aggregator/releases)
2. Rename to `file-aggregator.exe`
3. Run from command prompt or double-click for GUI

**macOS:**
```bash
# Download and install
curl -L https://github.com/your-username/file-aggregator/releases/latest/download/file-aggregator-macos -o file-aggregator
chmod +x file-aggregator
sudo mv file-aggregator /usr/local/bin/
```

**Linux:**
```bash
# Download and install
wget https://github.com/your-username/file-aggregator/releases/latest/download/file-aggregator-linux
chmod +x file-aggregator-linux
sudo mv file-aggregator-linux /usr/local/bin/file-aggregator
```

### Build from Source

See "Build Instructions" section below.

## Quick Start

### GUI Mode (Default)

Launch without arguments:

```bash
./file-aggregator
```

Drag and drop files into the window, then click "Save" or "Copy to Clipboard".

### CLI Mode

```bash
# Aggregate files and copy to clipboard
./file-aggregator add file1.rs file2.toml file3.md

# Save to file instead
./file-aggregator add --output result.md src/*.rs

# Interactive file picker (requires 'gum' installed)
./file-aggregator interactive
```

**Common Use Case:** Preparing code context for AI assistants
```bash
# Copy all source files to clipboard for ChatGPT/Claude
./file-aggregator add src/**/*.rs Cargo.toml

# Save for later review
./file-aggregator add --output context.md src/**/*.rs
```

## Usage

### CLI Commands

**`file-aggregator add <files...> [--output <path>]`**
- Aggregate specified files
- Copies to clipboard by default
- Use `--output` to save to file instead

**`file-aggregator interactive`**
- Launch interactive file picker (requires [gum](https://github.com/charmbracelet/gum))
- Select multiple files with space, confirm with enter

**`file-aggregator gui` or `file-aggregator` (no arguments)**
- Launch GUI application
- Drag and drop files into window

### Output Format

```markdown
rs path=/absolute/path/to/file.rs

\`\`\`rs
fn main() {
    println!("File content here");
}
\`\`\`

---

toml path=/absolute/path/to/config.toml

\`\`\`toml
[package]
name = "example"
\`\`\`

---
```

Binary files are base64-encoded with a `[BINARY CONTENT - BASE64 ENCODED]` marker.

## Security Warning

⚠️ **Output is plain text format.** The aggregated markdown contains file contents as-is. Before piping output to a shell or executing any commands:

1. Review the aggregated content manually
2. Verify no unexpected executables were included
3. Check for malicious content in base64-encoded binaries
4. Use `--allow-executables` only when you trust all input files

**Never** blindly execute: `./file-aggregator * | sh` — always inspect output first.

This tool validates input paths and applies security controls, but the aggregated text output is meant for **reading and review**, not automatic execution.

## Performance Benchmarks

File Aggregator uses parallel I/O (stdlib threads) to maximize throughput when aggregating multiple files.

### What the Benchmarks Measure

Standard test case: **20 files × 10MB each (200MB total)**

Metrics:
- Total aggregation time (file reading + formatting)
- Throughput (MB/s)
- Speedup vs sequential I/O

### Running Benchmarks

**Option 1: PowerShell (Windows)**
```powershell
.\benchmark_test.ps1
```

**Option 2: Bash (Linux/macOS)**
```bash
./benchmark_test.sh
```

**Option 3: Rust Integration Test**
```bash
cargo test --test benchmark -- --ignored --nocapture
```

Each script:
- Auto-generates 20 test files (10MB each)
- Runs 5 iterations
- Reports average time, min/max, throughput
- Auto-cleanup

### Expected Performance

**Baseline (Modern SSD):** 400-500ms for 200MB aggregation

**Performance varies by:**
- Storage type: NVMe SSD (best) > SATA SSD > HDD
- CPU cores: More cores = better parallelization
- File count: 10-50 files is optimal range
- System load: Background processes affect results

**Good indicators:**
- Throughput > 300 MB/s on SSD
- Consistent times across iterations

### Real-World Results

**Test System:** Intel i7-12700F (12C/20T), 32GB RAM, NVMe SSD (MSI M480 PRO), Windows 11

**Expected results based on parallel I/O implementation:**
- **Time:** 400-500ms for 200MB (20 files × 10MB)
- **Throughput:** 400-500 MB/s
- **Speedup:** 4-6x vs sequential I/O

**Theoretical analysis:**
- NVMe sequential read: ~3,000 MB/s
- With 20 parallel threads on 12-core CPU: Expected ~400-500 MB/s aggregate throughput
- Bottleneck: Thread scheduling + mutex contention on result collection

*Real benchmark results will replace this section once binary is available.*

### How to Interpret Your Results

Compare your throughput to expected baseline:
- **>400 MB/s**: Excellent (modern NVMe SSD)
- **200-400 MB/s**: Good (SATA SSD)
- **<100 MB/s**: Check for bottlenecks (antivirus, HDD, high system load)

See `BENCHMARKS.md` for detailed methodology and troubleshooting.

## License

Licensed under the [MIT License](LICENSE).

## Support This Project

This tool is free and open source. If you find it useful, consider [supporting development](https://github.com/sponsors/your-username) or contributing improvements via pull requests.

## Contributing

Contributions welcome! Please:
- Follow existing code style (run `cargo fmt`)
- Add tests for new security-sensitive features
- Update this README for user-facing changes

Run the test suite before submitting:

```bash
cargo test
cargo clippy -- -D warnings
```
