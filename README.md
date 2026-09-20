# File Aggregator

Secure command-line and GUI tool for aggregating multiple files into a single markdown document. Built with Rust for cross-platform reliability and memory safety.

## What It Does

File Aggregator takes multiple files from your filesystem and combines them into a single markdown file with proper formatting, security controls, and optional clipboard output. Each file's content is preserved in fenced code blocks with syntax highlighting support.

## Features

- **Dual Interface**: GUI with drag-and-drop support + full-featured CLI
- **Cross-Platform**: macOS and Linux support, single binary with no runtime dependencies
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

Expected binary size: 5-8 MB per platform.

## Usage

### GUI Mode

Launch without arguments to start the graphical interface:

```bash
./file-aggregator
```

Drag and drop files into the window. The tool will validate paths, check sizes, and aggregate content. Use the "Save" button to export or "Copy" to send to clipboard.

### CLI Mode

```bash
# Aggregate specific files
./file-aggregator file1.rs file2.toml file3.md

# With output file
./file-aggregator --output result.md src/*.rs

# Allow executable files without prompting
./file-aggregator --allow-executables script.sh binary.exe

# Interactive file picker (requires 'gum' installed)
./file-aggregator --interactive
```

#### CLI Options

- `--output <path>`, `-o <path>`: Write to file instead of stdout
- `--allow-executables`: Skip confirmation prompts for executable files
- `--interactive`, `-i`: Use interactive file picker (requires [gum](https://github.com/charmbracelet/gum))
- `--help`: Show all available options

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

## License

Free to use under [MIT License](LICENSE) (or Apache-2.0 — see LICENSE file). 

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
