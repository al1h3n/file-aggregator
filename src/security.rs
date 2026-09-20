use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::fs;
use base64::Engine;

/// Validate and canonicalize a path, detecting traversal attempts
pub fn validate_path(path: &Path) -> Result<PathBuf> {
    // Reject empty paths
    if path.as_os_str().is_empty() {
        anyhow::bail!("Empty path provided");
    }
    
    // Convert to string for inspection
    let path_str = path.to_string_lossy();
    
    // Check for traversal sequences
    if path_str.contains("..") {
        anyhow::bail!("Path traversal detected: {}", path_str);
    }
    
    // Check for NULL bytes
    if path_str.contains('\0') {
        anyhow::bail!("NULL byte in path: {}", path_str);
    }
    
    // Windows-specific checks
    #[cfg(windows)]
    {
        // Check for device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
        let upper = path_str.to_uppercase();
        let dangerous_names = [
            "CON", "PRN", "AUX", "NUL",
            "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
            "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ];
        
        for name in &dangerous_names {
            // Check for standalone device names (word boundaries)
            let upper_str = upper.as_str();
            if upper_str == *name 
                || upper_str.starts_with(&format!("{}.", name))
                || upper_str.starts_with(&format!("{}:", name))
                || upper_str.contains(&format!("\\{}.", name))
                || upper_str.contains(&format!("\\{}", name)) && upper_str.ends_with(name)
                || upper_str.contains(&format!("/{}.", name))
                || upper_str.contains(&format!("/{}", name)) && upper_str.ends_with(name)
            {
                anyhow::bail!("Windows device name detected: {}", path_str);
            }
        }
        
        // Check for UNC paths (block \\?\UNC\ but allow \\?\C:\ extended-length local paths)
        // After canonicalize(), Windows adds \\?\ prefix for long paths
        if path_str.starts_with(r"\\?\UNC\") || path_str.starts_with(r"\?\UNC\")
            || (path_str.starts_with(r"\\") && !path_str.starts_with(r"\\?\") && !path_str.starts_with(r"\?\")) {
            anyhow::bail!("UNC paths not allowed: {}", path_str);
        }
        
        // Check for alternate data streams (C:\path\file.txt:stream)
        // Allow drive letter colon (C:) and extended path prefix (\\?\C:)
        let drive_letter_positions: Vec<usize> = path_str.match_indices(':')
            .map(|(i, _)| i)
            .collect();
        
        for pos in drive_letter_positions {
            // Valid: C:, \\?\C:, \?\C:
            let is_drive_letter = (pos == 1) // C:
                || (pos == 5 && path_str.starts_with(r"\\?\")) // \\?\C:
                || (pos == 3 && path_str.starts_with(r"\?\"));  // \?\C:
            
            if !is_drive_letter {
                anyhow::bail!("Alternate data stream detected: {}", path_str);
            }
        }
    }
    
    // Canonicalize path (resolves symlinks, makes absolute)
    let canonical = path.canonicalize()
        .with_context(|| format!("Failed to canonicalize path: {}", path_str))?;
    
    // Verify it's a file (not directory or special device)
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("Failed to read metadata: {}", canonical.display()))?;
    
    if !metadata.is_file() {
        anyhow::bail!("Not a regular file: {}", canonical.display());
    }
    
    // Check file size (50MB limit per file)
    const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;
    if metadata.len() > MAX_FILE_SIZE {
        anyhow::bail!(
            "File too large: {} bytes (max: {} MB)",
            metadata.len(),
            MAX_FILE_SIZE / 1024 / 1024
        );
    }
    
    Ok(canonical)
}

/// Detect if a file is binary using magic bytes
pub fn detect_binary(path: &Path) -> Result<bool> {
    // Read first 8192 bytes for magic byte detection
    let kind = infer::get_from_path(path)
        .context("Failed to read file for binary detection")?;
    
    Ok(kind.is_some())
}

/// Read file safely, encoding binaries as Base64
pub fn read_file_safe(path: &Path) -> Result<String> {
    let is_binary = detect_binary(path)?;
    let (is_exec, exec_type) = is_executable_file(path)?;
    
    if is_binary {
        // Check size limit for executables
        if is_exec {
            const MAX_EXEC_SIZE: u64 = 50 * 1024 * 1024; // 50MB (raised from 10MB for archives)
            let metadata = fs::metadata(path)?;
            if metadata.len() > MAX_EXEC_SIZE {
                anyhow::bail!(
                    "Executable file too large: {} MB (max: 50 MB)",
                    metadata.len() / 1024 / 1024
                );
            }
        }
        
        // Read and Base64 encode
        let bytes = fs::read(path)
            .with_context(|| format!("Failed to read binary file: {}", path.display()))?;
        
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        
        if is_exec {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Ok(format!(
                "# WARNING: Executable file content below\n# Type: {}\n# Added: {}\n```base64\n{}\n```",
                exec_type, timestamp, encoded
            ))
        } else {
            Ok(format!("```base64\n{}\n```", encoded))
        }
    } else {
        // Read as text, strip NULL bytes
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read text file: {}", path.display()))?;
        
        let clean_content = content.replace('\0', "");
        
        if is_exec {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            Ok(format!(
                "# WARNING: Executable file content below\n# Type: {}\n# Added: {}\n{}",
                exec_type, timestamp, clean_content
            ))
        } else {
            Ok(clean_content)
        }
    }
}

/// Strip Windows extended-length path prefix for display
pub fn strip_windows_prefix(path: &str) -> String {
    if path.starts_with(r"\\?\") {
        path[4..].to_string()
    } else {
        path.to_string()
    }
}

/// Check if file is executable (both extension and magic bytes)
pub fn is_executable_file(path: &Path) -> Result<(bool, String)> {
    // Check extension first
    let ext = path.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    
    let executable_extensions = [
        // Windows
        "exe", "dll", "bat", "cmd", "ps1", "msi", "scr", "com", "pif", 
        "vbs", "js", "wsf", "hta", "cpl", "jar",
        // Unix/Linux
        "sh", "run", "bin", "elf", "deb", "rpm", "appimage", "out",
        // macOS
        "app", "dmg", "pkg", "command",
        // Scripts
        "py", "rb", "pl", "php", "lua",
        // Archives (may contain executables)
        "zip", "tar", "gz", "7z", "rar", "bz2", "xz",
    ];
    
    let is_exec_ext = executable_extensions.contains(&ext.as_str());
    
    // Check magic bytes
    let is_exec_magic = is_executable(path)?;
    
    if is_exec_ext || is_exec_magic {
        let file_type = if is_exec_magic {
            detect_executable_type(path)?
        } else {
            format!(".{} file", ext)
        };
        Ok((true, file_type))
    } else {
        Ok((false, String::new()))
    }
}

/// Detect executable type from magic bytes
fn detect_executable_type(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path).context("Failed to open file")?;
    use std::io::Read;
    let mut header = [0u8; 4];
    file.read_exact(&mut header).ok();
    
    let file_type = match &header {
        [0x4D, 0x5A, _, _] => "PE executable (Windows .exe/.dll)",
        [0x7F, 0x45, 0x4C, 0x46] => "ELF executable (Linux)",
        [0xFE, 0xED, 0xFA, 0xCE] | [0xCE, 0xFA, 0xED, 0xFE] => "Mach-O executable (macOS 32-bit)",
        [0xFE, 0xED, 0xFA, 0xCF] | [0xCF, 0xFA, 0xED, 0xFE] => "Mach-O executable (macOS 64-bit)",
        [0x23, 0x21, _, _] => "Script with shebang",
        _ => {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                &format!(".{} file", ext)
            } else {
                "Unknown executable"
            }
        }
    };
    
    Ok(file_type.to_string())
}

/// Detect if file is executable based on magic bytes
fn is_executable(path: &Path) -> Result<bool> {
    let mut file = fs::File::open(path)
        .context("Failed to open file")?;
    
    use std::io::Read;
    let mut header = [0u8; 4];
    file.read_exact(&mut header).ok();
    
    // Check for common executable magic bytes
    let is_exec = matches!(
        &header,
        // PE (Windows .exe/.dll)
        [0x4D, 0x5A, _, _] |
        // ELF (Linux)
        [0x7F, 0x45, 0x4C, 0x46] |
        // Mach-O (macOS) 32-bit
        [0xFE, 0xED, 0xFA, 0xCE] |
        [0xCE, 0xFA, 0xED, 0xFE] |
        // Mach-O 64-bit
        [0xFE, 0xED, 0xFA, 0xCF] |
        [0xCF, 0xFA, 0xED, 0xFE] |
        // Shebang scripts
        [0x23, 0x21, _, _]
    );
    
    Ok(is_exec)
}

/// Strip control characters for safe clipboard/output
pub fn strip_control_chars(s: &str) -> String {
    s.chars()
        .filter(|&c| {
            // Keep printable chars, newline, tab
            if c == '\n' || c == '\t' {
                return true;
            }
            
            // Remove control chars (0x00-0x1F, 0x7F-0x9F)
            if c <= '\x1F' || (c >= '\x7F' && c <= '\u{9F}') {
                return false;
            }
            
            // Remove RLO/LRO (used for filename spoofing)
            if c == '\u{202E}' || c == '\u{202D}' {
                return false;
            }
            
            true
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    
    
    #[test]
    fn test_strip_control_chars() {
        assert_eq!(strip_control_chars("hello\x00world"), "helloworld");
        assert_eq!(strip_control_chars("test\x1B[31mred\x1B[0m"), "test[31mred[0m");
        assert_eq!(strip_control_chars("normal\ntext\ttab"), "normal\ntext\ttab");
        assert_eq!(strip_control_chars("test\u{202E}fake.txt"), "testfake.txt");
    }
    
    #[test]
    fn test_validate_path_traversal() {
        let result = validate_path(Path::new("../etc/passwd"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }
    
    #[test]
    fn test_validate_path_null_byte() {
        let result = validate_path(Path::new("test\0.txt"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NULL"));
    }
}
