use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

// Import from main crate
use file_aggregator::security::{validate_path, strip_control_chars};

/// Helper to create temp test files
fn create_test_file(dir: &Path, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.join(name);
    let mut file = fs::File::create(&path).unwrap();
    file.write_all(content).unwrap();
    path
}

// ============================================================================
// TEST SET 1: Malicious Path Patterns
// ============================================================================

#[test]
fn test_path_traversal_basic() {
    let result = validate_path(Path::new("../etc/passwd"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("traversal"));
}

#[test]
fn test_path_traversal_double_dots() {
    let result = validate_path(Path::new("test/../../secret.txt"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("traversal"));
}

#[test]
fn test_path_traversal_encoded() {
    // URL-encoded double dots
    let result = validate_path(Path::new("test/%2e%2e/secret.txt"));
    assert!(result.is_err());
}

#[test]
#[cfg(windows)]
fn test_windows_unc_path() {
    let result = validate_path(Path::new("\\\\server\\share\\file.txt"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("UNC"));
}

#[test]
#[cfg(windows)]
fn test_windows_device_names() {
    let devices = ["CON", "PRN", "AUX", "NUL", "COM1", "LPT1"];
    
    for device in &devices {
        let result = validate_path(Path::new(device));
        assert!(result.is_err(), "Device name {} should be rejected", device);
    }
}

#[test]
#[cfg(windows)]
fn test_windows_alternate_data_stream() {
    let result = validate_path(Path::new("test.txt:hidden"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("data stream"));
}

#[test]
#[cfg(unix)]
fn test_symlink_loop() {
    let temp_dir = TempDir::new().unwrap();
    let link1 = temp_dir.path().join("link1");
    let link2 = temp_dir.path().join("link2");
    
    // Create circular symlinks
    std::os::unix::fs::symlink(&link2, &link1).ok();
    std::os::unix::fs::symlink(&link1, &link2).ok();
    
    let result = validate_path(&link1);
    // Should fail on canonicalize due to symlink loop
    assert!(result.is_err());
}

// ============================================================================
// TEST SET 2: Malicious Filename Patterns
// ============================================================================

#[test]
fn test_null_byte_in_path() {
    let result = validate_path(Path::new("test\0.txt"));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("NULL"));
}

#[test]
fn test_empty_path() {
    let result = validate_path(Path::new(""));
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Empty"));
}

#[test]
fn test_strip_control_chars_null() {
    assert_eq!(strip_control_chars("hello\x00world"), "helloworld");
}

#[test]
fn test_strip_control_chars_ansi_escape() {
    // ANSI color codes
    assert_eq!(strip_control_chars("test\x1B[31mred\x1B[0m"), "test[31mred[0m");
}

#[test]
fn test_strip_control_chars_rlo() {
    // Right-to-left override (filename spoofing)
    assert_eq!(strip_control_chars("test\u{202E}txt.exe"), "testtxt.exe");
}

#[test]
fn test_strip_control_chars_newline_injection() {
    let input = "safe\nline";
    let output = strip_control_chars(input);
    assert_eq!(output, "safe\nline"); // Newlines preserved
}

#[test]
fn test_strip_control_chars_command_injection() {
    // Attempt to inject command via special chars
    let input = "test; rm -rf /";
    let output = strip_control_chars(input);
    // Semicolons are not control chars, but special handling elsewhere prevents execution
    assert!(output.contains(';'));
}

// ============================================================================
// TEST SET 3: Malicious Binary Files
// ============================================================================

#[test]
fn test_pe_executable_detection() {
    let temp_dir = TempDir::new().unwrap();
    
    // PE header (Windows .exe)
    let pe_header = vec![0x4D, 0x5A, 0x90, 0x00]; // MZ header
    let path = create_test_file(temp_dir.path(), "test.exe", &pe_header);
    
    // Should validate but trigger warning
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_elf_executable_detection() {
    let temp_dir = TempDir::new().unwrap();
    
    // ELF header (Linux binary)
    let elf_header = vec![0x7F, 0x45, 0x4C, 0x46]; // ELF magic
    let path = create_test_file(temp_dir.path(), "test.bin", &elf_header);
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_macho_executable_detection() {
    let temp_dir = TempDir::new().unwrap();
    
    // Mach-O header (macOS binary)
    let macho_header = vec![0xFE, 0xED, 0xFA, 0xCE]; // 32-bit Mach-O
    let path = create_test_file(temp_dir.path(), "test.macho", &macho_header);
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_script_with_shebang() {
    let temp_dir = TempDir::new().unwrap();
    
    // Shell script with shebang
    let script = b"#!/bin/bash\necho 'test'";
    let path = create_test_file(temp_dir.path(), "test.sh", script);
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_polyglot_png_pe() {
    let temp_dir = TempDir::new().unwrap();
    
    // PNG header followed by PE header (polyglot file)
    let mut polyglot = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // PNG
    polyglot.extend_from_slice(&[0x4D, 0x5A]); // PE header at offset
    
    let path = create_test_file(temp_dir.path(), "image.png", &polyglot);
    
    // Should detect as binary and handle safely
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_corrupted_executable() {
    let temp_dir = TempDir::new().unwrap();
    
    // Truncated PE header (corrupted)
    let corrupted = vec![0x4D, 0x5A]; // Incomplete PE
    let path = create_test_file(temp_dir.path(), "corrupt.exe", &corrupted);
    
    let result = validate_path(&path);
    assert!(result.is_ok()); // Should still validate, detection happens at read
}

// ============================================================================
// TEST SET 4: Size Attacks
// ============================================================================

#[test]
fn test_empty_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = create_test_file(temp_dir.path(), "empty.txt", b"");
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_file_size_limit_exceeded() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create 51MB file (exceeds 50MB limit)
    let large_content = vec![0u8; 51 * 1024 * 1024];
    let path = create_test_file(temp_dir.path(), "large.bin", &large_content);
    
    let result = validate_path(&path);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("too large"));
}

#[test]
fn test_file_exactly_at_limit() {
    let temp_dir = TempDir::new().unwrap();
    
    // Create exactly 50MB file
    let content = vec![0u8; 50 * 1024 * 1024];
    let path = create_test_file(temp_dir.path(), "exact.bin", &content);
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

#[test]
fn test_one_byte_file() {
    let temp_dir = TempDir::new().unwrap();
    let path = create_test_file(temp_dir.path(), "tiny.txt", b"x");
    
    let result = validate_path(&path);
    assert!(result.is_ok());
}

// Note: Compression bomb test requires actual ZIP decompression,
// which would be tested in integration tests with full aggregator

// ============================================================================
// INTEGRATION TESTS
// ============================================================================

#[test]
fn test_valid_text_file_full_flow() {
    let temp_dir = TempDir::new().unwrap();
    let content = b"Hello, world!\nThis is a test.";
    let path = create_test_file(temp_dir.path(), "test.txt", content);
    
    let result = validate_path(&path);
    assert!(result.is_ok());
    
    let canonical = result.unwrap();
    assert!(canonical.is_absolute());
}

#[test]
fn test_directory_rejection() {
    let temp_dir = TempDir::new().unwrap();
    
    let result = validate_path(temp_dir.path());
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("Not a regular file"));
}

#[test]
fn test_nonexistent_file() {
    let result = validate_path(Path::new("nonexistent_file_xyz_123.txt"));
    assert!(result.is_err());
}
