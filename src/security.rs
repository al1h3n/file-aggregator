//! Input validation and safe file reading.
//!
//! Everything that touches a user-supplied path goes through here:
//! - [`validate_path`] rejects traversal, NUL bytes, Windows device names / UNC /
//!   alternate data streams, non-regular files and files over 50 MB.
//! - [`read_file_safe`] reads a file **once** (bounded, so a file that grows after
//!   validation still can't exceed the limit) and turns it into text: UTF-8 stays text,
//!   anything else becomes Base64. Executables and scripts get a warning header.
//! - [`strip_control_chars`] removes terminal escapes, C1 controls and the
//!   right-to-left override characters used for filename spoofing.

use anyhow::{Context, Result};
use base64::Engine;
use std::borrow::Cow;
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};

/// Largest single file we accept (50 MB).
pub const MAX_FILE_SIZE: u64 = 50 * 1024 * 1024;

/// How many leading bytes `infer` needs to recognise a file type (same as `infer::get_from_path`).
const SNIFF_LEN: usize = 8192;

/// Extensions treated as executable or "may contain executables" even without magic bytes.
const EXECUTABLE_EXTENSIONS: &[&str] = &[
    // Windows
    "exe", "dll", "bat", "cmd", "ps1", "msi", "scr", "com", "pif", "vbs", "js", "wsf", "hta", "cpl", "jar", // Unix/Linux
    "sh", "run", "bin", "elf", "deb", "rpm", "appimage", "out", // macOS
    "app", "dmg", "pkg", "command", // Scripts
    "py", "rb", "pl", "php", "lua", // Archives (may contain executables)
    "zip", "tar", "gz", "7z", "rar", "bz2", "xz",
];

/// Validate and canonicalize a path, detecting traversal attempts.
pub fn validate_path(path: &Path) -> Result<PathBuf> {
    validate_path_with_size(path).map(|(canonical, _)| canonical)
}

/// Same checks as [`validate_path`], but also returns the file size in bytes, so callers
/// that need it (size limits, the GUI's size column) don't have to stat the file again.
pub fn validate_path_with_size(path: &Path) -> Result<(PathBuf, u64)> {
    // Reject empty paths
    if path.as_os_str().is_empty() {
        anyhow::bail!("Empty path provided");
    }

    let path_str = path.to_string_lossy();

    // A `..` *component* is traversal. Checking components (not the raw string) keeps
    // legitimate names such as `notes..md` or `v1..v2.txt` working.
    if path.components().any(|c| c == Component::ParentDir) {
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
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6",
            "LPT7", "LPT8", "LPT9",
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
        if path_str.starts_with(r"\\?\UNC\")
            || path_str.starts_with(r"\?\UNC\")
            || (path_str.starts_with(r"\\") && !path_str.starts_with(r"\\?\") && !path_str.starts_with(r"\?\"))
        {
            anyhow::bail!("UNC paths not allowed: {}", path_str);
        }

        // Check for alternate data streams (C:\path\file.txt:stream)
        // Allow drive letter colon (C:) and extended path prefix (\\?\C:)
        for (pos, _) in path_str.match_indices(':') {
            // Valid: C:, \\?\C:, \?\C:
            let is_drive_letter = (pos == 1) // C:
                || (pos == 5 && path_str.starts_with(r"\\?\")) // \\?\C:
                || (pos == 3 && path_str.starts_with(r"\?\")); // \?\C:

            if !is_drive_letter {
                anyhow::bail!("Alternate data stream detected: {}", path_str);
            }
        }
    }

    // Canonicalize path (resolves symlinks, makes absolute)
    let canonical = path.canonicalize().with_context(|| format!("Failed to canonicalize path: {}", path_str))?;

    // Verify it's a file (not directory or special device)
    let metadata = fs::metadata(&canonical).with_context(|| format!("Failed to read metadata: {}", canonical.display()))?;

    if !metadata.is_file() {
        anyhow::bail!("Not a regular file: {}", canonical.display());
    }

    // Check file size (50MB limit per file)
    if metadata.len() > MAX_FILE_SIZE {
        anyhow::bail!("File too large: {} bytes (max: {} MB)", metadata.len(), MAX_FILE_SIZE / 1024 / 1024);
    }

    Ok((canonical, metadata.len()))
}

/// Detect if a file is binary using magic bytes
pub fn detect_binary(path: &Path) -> Result<bool> {
    // Read first 8192 bytes for magic byte detection
    let kind = infer::get_from_path(path).context("Failed to read file for binary detection")?;

    Ok(kind.is_some_and(is_binary_kind))
}

/// `infer` also recognises a few *text* formats (HTML, XML, shell scripts). Those stay
/// text; before, they were Base64-encoded like images, which made them unreadable.
fn is_binary_kind(kind: infer::Type) -> bool {
    kind.matcher_type() != infer::MatcherType::Text
}

fn sniff_binary(head: &[u8]) -> bool {
    infer::get(head).is_some_and(is_binary_kind)
}

/// What a quick look at the start of a file says about it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Inspection {
    /// `Some(description)` when the file is an executable, script or archive.
    pub executable: Option<String>,
    /// Recognised binary format (it will be Base64-encoded in the output).
    pub binary: bool,
}

/// Looks at the first 8 KB of a file once: executable or not, binary or not.
/// Used by the GUI when files are added (warning dialog, token estimate).
pub fn inspect_file(path: &Path) -> Result<Inspection> {
    let mut head = Vec::with_capacity(SNIFF_LEN);
    fs::File::open(path).context("Failed to open file")?.take(SNIFF_LEN as u64).read_to_end(&mut head).context("Failed to read file")?;
    Ok(Inspection { executable: executable_kind(path, &head), binary: sniff_binary(&head) })
}

/// Read file safely, encoding binaries as Base64.
///
/// The file is opened and read exactly once; type detection runs on the bytes in memory.
pub fn read_file_safe(path: &Path) -> Result<String> {
    let bytes = read_bounded(path, MAX_FILE_SIZE)?;
    let sniff = &bytes[..bytes.len().min(SNIFF_LEN)];
    let exec = executable_kind(path, sniff);

    // Recognised binary formats, and anything that isn't valid UTF-8, become Base64.
    // (Before, non-UTF-8 text failed the whole aggregation.)
    let text = if sniff_binary(sniff) { Err(bytes) } else { String::from_utf8(bytes).map_err(|e| e.into_bytes()) };

    let warning = |kind: &str| {
        let timestamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
        format!("# WARNING: Executable file content below\n# Type: {}\n# Added: {}\n", kind, timestamp)
    };

    Ok(match text {
        Ok(content) => {
            // Strip NUL bytes; `contains` is a fast memchr scan, so clean files aren't copied.
            let clean = if content.contains('\0') { content.replace('\0', "") } else { content };
            match exec {
                Some(kind) => warning(&kind) + &clean,
                None => clean,
            }
        }
        Err(bytes) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
            let body = format!("```base64\n{}\n```", encoded);
            match exec {
                Some(kind) => warning(&kind) + &body,
                None => body,
            }
        }
    })
}

/// Reads at most `limit` bytes; a file that is larger *now* (even if it was smaller when
/// validated) is an error rather than a silent truncation.
fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let file = fs::File::open(path).with_context(|| format!("Failed to open file: {}", path.display()))?;
    // Allocate once: the size hint plus one byte, so `read_to_end` never has to grow the buffer.
    let hint = file.metadata().map(|m| m.len()).unwrap_or(0).min(limit);
    let mut bytes = Vec::with_capacity(hint as usize + 1);
    file.take(limit + 1).read_to_end(&mut bytes).with_context(|| format!("Failed to read file: {}", path.display()))?;
    if bytes.len() as u64 > limit {
        anyhow::bail!("File too large: more than {} MB", limit / 1024 / 1024);
    }
    Ok(bytes)
}

/// Writes `content` to `path`, readable only by the current user on Unix (`0o600`).
///
/// New files are *created* with that mode, so they're never briefly world-readable;
/// existing files get their permissions tightened as well.
pub fn write_private(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(path).context("Failed to create output file")?;
    // Only regular files: `-o /dev/null` (or another device) must never be chmod-ed, which
    // as root would lock every other user out of it.
    #[cfg(unix)]
    if file.metadata().is_ok_and(|m| m.is_file()) {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600)).context("Failed to set file permissions")?;
    }
    file.write_all(content.as_bytes()).context("Failed to write output file")
}

/// Strip Windows extended-length path prefix for display
pub fn strip_windows_prefix(path: &str) -> String {
    path.strip_prefix(r"\\?\").unwrap_or(path).to_string()
}

/// Check if file is executable (both extension and magic bytes)
pub fn is_executable_file(path: &Path) -> Result<(bool, String)> {
    let mut header = Vec::with_capacity(4);
    fs::File::open(path).context("Failed to open file")?.take(4).read_to_end(&mut header).context("Failed to read file")?;
    Ok(match executable_kind(path, &header) {
        Some(kind) => (true, kind),
        None => (false, String::new()),
    })
}

/// `Some(description)` when the extension or the first bytes say "executable".
/// `header` may be shorter than 4 bytes (tiny files); patterns only need what they check.
fn executable_kind(path: &Path, header: &[u8]) -> Option<String> {
    let magic = match header {
        [0x4D, 0x5A, ..] => Some("PE executable (Windows .exe/.dll)"),
        [0x7F, 0x45, 0x4C, 0x46, ..] => Some("ELF executable (Linux)"),
        [0xFE, 0xED, 0xFA, 0xCE, ..] | [0xCE, 0xFA, 0xED, 0xFE, ..] => Some("Mach-O executable (macOS 32-bit)"),
        [0xFE, 0xED, 0xFA, 0xCF, ..] | [0xCF, 0xFA, 0xED, 0xFE, ..] => Some("Mach-O executable (macOS 64-bit)"),
        // `#!` starts a shebang line, unless it's a Rust inner attribute (`#![...]`), which
        // rustc itself never treats as a shebang.
        [0x23, 0x21, rest @ ..] if rest.iter().find(|b| !matches!(b, b' ' | b'\t')) != Some(&b'[') => Some("Script with shebang"),
        _ => None,
    };
    if let Some(kind) = magic {
        return Some(kind.to_string());
    }
    let ext = path.extension().and_then(|e| e.to_str()).map(str::to_lowercase).unwrap_or_default();
    EXECUTABLE_EXTENSIONS.contains(&ext.as_str()).then(|| format!(".{} file", ext))
}

/// Characters removed from output: C0 controls except `\n` and `\t`, DEL, C1 controls,
/// and the left-to-right / right-to-left override characters.
#[inline]
fn is_stripped(c: char) -> bool {
    matches!(c, '\0'..='\x08' | '\x0B'..='\x1F' | '\x7F'..='\u{9F}' | '\u{202D}' | '\u{202E}')
}

/// Byte offset of the first character [`is_stripped`] would remove, if any.
///
/// Scans 32-byte chunks with a branch-free "is every byte printable ASCII, `\n` or `\t`"
/// test that the compiler vectorizes; only chunks that fail it are checked exactly. The
/// exact check can work on bytes because every stripped character starts with a byte
/// that never appears inside another UTF-8 character: C0/DEL are ASCII, C1 controls are
/// `C2 80..=9F`, and U+202D/U+202E are `E2 80 AD/AE`.
fn first_stripped(s: &str) -> Option<usize> {
    const CHUNK: usize = 32;
    let bytes = s.as_bytes();
    let exact = |i: usize| match bytes[i] {
        b'\n' | b'\t' => false,
        0x00..=0x1F | 0x7F => true,
        0xC2 => matches!(bytes.get(i + 1), Some(0x80..=0x9F)),
        0xE2 => bytes.get(i + 1) == Some(&0x80) && matches!(bytes.get(i + 2), Some(0xAD | 0xAE)),
        _ => false,
    };
    for (n, chunk) in bytes.chunks(CHUNK).enumerate() {
        let plain = chunk.iter().fold(true, |all, &b| all & (matches!(b, 0x20..=0x7E) | (b == b'\n') | (b == b'\t')));
        if !plain {
            let start = n * CHUNK;
            if let Some(i) = (start..start + chunk.len()).find(|&i| exact(i)) {
                return Some(i);
            }
        }
    }
    None
}

/// [`strip_control_chars`] without the copy when there is nothing to strip (the usual case).
pub fn strip_control_chars_cow(s: &str) -> Cow<'_, str> {
    match first_stripped(s) {
        None => Cow::Borrowed(s),
        Some(first) => {
            let mut out = String::with_capacity(s.len());
            out.push_str(&s[..first]);
            out.extend(s[first..].chars().filter(|&c| !is_stripped(c)));
            Cow::Owned(out)
        }
    }
}

/// Strip control characters for safe clipboard/output
pub fn strip_control_chars(s: &str) -> String {
    strip_control_chars_cow(s).into_owned()
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

    /// The original character-by-character filter, kept as the reference.
    fn reference_strip(s: &str) -> String {
        s.chars()
            .filter(|&c| {
                if c == '\n' || c == '\t' {
                    return true;
                }
                if c <= '\x1F' || ('\x7F'..='\u{9F}').contains(&c) {
                    return false;
                }
                c != '\u{202E}' && c != '\u{202D}'
            })
            .collect()
    }

    #[test]
    fn strip_matches_reference_on_random_text() {
        // Deterministic xorshift so failures reproduce. The alphabet mixes plain ASCII with
        // every boundary case: C0/C1 controls, DEL, the overrides, their UTF-8 neighbours
        // (U+00A0, U+202C, U+202F, U+2080) and 2-, 3- and 4-byte characters.
        let alphabet = [
            'a', 'Z', ' ', '\n', '\t', '\r', '\0', '\x1B', '\x1F', '\x7F', '\u{80}', '\u{9F}', '\u{A0}', '£', 'ж', '\u{202C}', '\u{202D}', '\u{202E}',
            '\u{202F}', '\u{2080}', '—', '日', '🦀', '`',
        ];
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = || {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state
        };
        for _ in 0..2000 {
            let len = (next() % 200) as usize;
            // Mostly plain text, like real files, so the vector fast path is exercised too.
            let s: String = (0..len).map(|_| if next() % 4 == 0 { alphabet[(next() % alphabet.len() as u64) as usize] } else { 'x' }).collect();
            assert_eq!(strip_control_chars(&s), reference_strip(&s), "input: {s:?}");
        }
    }

    #[test]
    fn clean_text_is_not_copied() {
        let s = "fn main() {\n\tprintln!(\"héllo — 日本\");\n}\n".repeat(10);
        assert!(matches!(strip_control_chars_cow(&s), Cow::Borrowed(_)));
    }

    #[test]
    fn test_validate_path_traversal() {
        let result = validate_path(Path::new("../etc/passwd"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn double_dots_inside_a_name_are_not_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes..v2.md");
        fs::write(&path, "ok").unwrap();
        assert!(validate_path(&path).is_ok());
    }

    #[test]
    fn test_validate_path_null_byte() {
        let result = validate_path(Path::new("test\0.txt"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("NULL"));
    }

    #[test]
    fn read_file_safe_formats() {
        let dir = tempfile::tempdir().unwrap();
        let text = dir.path().join("a.txt");
        fs::write(&text, "a\0b").unwrap();
        assert_eq!(read_file_safe(&text).unwrap(), "ab");

        // PNG magic -> Base64 block.
        let png = dir.path().join("a.png");
        fs::write(&png, [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]).unwrap();
        assert_eq!(read_file_safe(&png).unwrap(), "```base64\niVBORw0KGgo=\n```");

        // Invalid UTF-8 without known magic used to fail; now it's Base64 too.
        let latin1 = dir.path().join("b.txt");
        fs::write(&latin1, [b'c', 0xE9, b'!']).unwrap();
        assert_eq!(read_file_safe(&latin1).unwrap(), "```base64\nY+kh\n```");

        // HTML is recognised by `infer` but is text, so it stays readable.
        let html = dir.path().join("index.html");
        fs::write(&html, "<!DOCTYPE html><html></html>").unwrap();
        assert_eq!(read_file_safe(&html).unwrap(), "<!DOCTYPE html><html></html>");
        assert!(!detect_binary(&html).unwrap() && detect_binary(&png).unwrap());

        // Scripts keep the warning header.
        let script = dir.path().join("run.sh");
        fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
        let out = read_file_safe(&script).unwrap();
        assert!(out.starts_with("# WARNING: Executable file content below\n# Type: Script with shebang\n# Added: "));
        assert!(out.ends_with("\n#!/bin/sh\necho hi\n"));
    }

    #[test]
    #[cfg(unix)]
    fn write_private_sets_0600_but_leaves_devices_alone() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.md");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        write_private(&path, "new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);

        let null = Path::new("/dev/null");
        let before = fs::metadata(null).unwrap().permissions().mode();
        write_private(null, "discarded").unwrap();
        assert_eq!(fs::metadata(null).unwrap().permissions().mode(), before);
    }

    #[test]
    fn executable_detection_matches_old_rules() {
        let dir = tempfile::tempdir().unwrap();
        let cases: &[(&str, &[u8], Option<&str>)] = &[
            ("corrupt.exe", &[0x4D, 0x5A], Some("PE executable (Windows .exe/.dll)")),
            ("x.bin", &[0x7F, 0x45, 0x4C, 0x46], Some("ELF executable (Linux)")),
            ("x.macho", &[0xCF, 0xFA, 0xED, 0xFE], Some("Mach-O executable (macOS 64-bit)")),
            ("tool.PY", b"print(1)", Some(".py file")),
            ("main.rs", b"#![windows_subsystem = \"windows\"]", None),
            ("env.txt", b"#! /usr/bin/env python3", Some("Script with shebang")),
            ("notes.md", b"# hi", None),
            ("empty.txt", b"", None),
        ];
        for (name, bytes, expected) in cases {
            let path = dir.path().join(name);
            fs::write(&path, bytes).unwrap();
            let (is_exec, kind) = is_executable_file(&path).unwrap();
            assert_eq!(is_exec, expected.is_some(), "{name}");
            assert_eq!(kind, expected.unwrap_or(""), "{name}");
            assert_eq!(inspect_file(&path).unwrap().executable.as_deref(), *expected, "{name}");
        }
    }
}
