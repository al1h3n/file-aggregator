//! Turns a list of files into one prompt document.
//!
//! Output layout (unchanged from earlier versions, so existing prompts keep working):
//!
//! ````text
//! BEGINNING
//! Read following file and answer question given:
//! <custom prompt, or "QUESTION">
//!
//! ```rs path=/abs/path/main.rs
//! <file content>
//! ```
//!
//! END
//! ````
//!
//! How it runs:
//! 1. **Validate every path and add up the sizes first.** The 200 MB total limit fails
//!    fast, before a single byte of content is read.
//! 2. **Read in parallel** on rayon's thread pool (one thread per CPU core, not one per
//!    file). Each worker reads its file once and sanitizes it, so the expensive scanning
//!    is spread over all cores. Results keep the input order.
//! 3. **Assemble** into one `String` allocated at its final size, so no re-copying.
//!
//! The result is already sanitized ([`strip_control_chars`](crate::security::strip_control_chars)):
//! callers can put it on the clipboard or in a file as is.

use anyhow::Result;
use rayon::prelude::*;
use std::borrow::Cow;
use std::path::{Path, PathBuf};

use crate::security::{read_file_safe, strip_control_chars_cow, validate_path_with_size};

/// Total size limit for one aggregation: 200 MB.
pub const MAX_TOTAL_SIZE: u64 = 200 * 1024 * 1024;

const HEADER: &str = "BEGINNING\nRead following file and answer question given:\n";
const FOOTER: &str = "END";

/// One file, read and ready to be written out.
struct Entry {
    /// Extension used as the code-fence language (`txt` when there is none).
    extension: String,
    /// Canonical path for the `path=` attribute, sanitized.
    path: String,
    content: String,
    /// Backticks in the fence: one more than the longest run inside the content, at least 3.
    fence: usize,
}

/// Aggregate multiple files into prompt format
pub fn aggregate_files(paths: &[PathBuf]) -> Result<String> {
    aggregate_files_with_prompt(paths, None)
}

/// Aggregate multiple files with optional custom prompt
pub fn aggregate_files_with_prompt(paths: &[PathBuf], custom_prompt: Option<&str>) -> Result<String> {
    // Phase 1: validate (defense in depth: callers validate too) and check the total size.
    // Sequential on purpose: it's only a few syscalls per file, and the first bad path
    // is then always the one reported.
    let mut total_size: u64 = 0;
    let mut canonical = Vec::with_capacity(paths.len());
    for path in paths {
        let (path, size) = validate_path_with_size(path)?;
        total_size += size;
        if total_size > MAX_TOTAL_SIZE {
            anyhow::bail!("Total size exceeds limit: {} MB (max: 200 MB)", total_size / 1024 / 1024);
        }
        canonical.push(path);
    }

    // Phase 2: read + sanitize in parallel; `collect` keeps the original order.
    let entries: Vec<Entry> = canonical.par_iter().map(|path| read_entry(path)).collect::<Result<_>>()?;

    // Phase 3: assemble into a buffer allocated once at its final size.
    let prompt = custom_prompt.map(strip_control_chars_cow);
    let capacity = HEADER.len()
        + prompt.as_deref().map_or("QUESTION\n\n".len(), |p| p.len() + 2)
        + entries.iter().map(|e| e.content.len() + e.path.len() + e.extension.len() + 2 * e.fence + 10).sum::<usize>()
        + FOOTER.len();
    let mut output = String::with_capacity(capacity);
    output.push_str(HEADER);
    match prompt.as_deref() {
        // A blank custom prompt adds nothing; no prompt at all adds the QUESTION placeholder.
        Some(prompt) if !prompt.trim().is_empty() => {
            output.push_str(prompt);
            output.push_str("\n\n");
        }
        Some(_) => {}
        None => output.push_str("QUESTION\n\n"),
    }
    for entry in &entries {
        // Format: ```extension path=FULL_PATH
        push_fence(&mut output, entry.fence);
        output.push_str(&entry.extension);
        output.push_str(" path=");
        output.push_str(&entry.path);
        output.push('\n');
        output.push_str(&entry.content);
        output.push('\n');
        push_fence(&mut output, entry.fence);
        output.push_str("\n\n");
    }
    output.push_str(FOOTER);
    debug_assert!(output.len() <= capacity, "capacity estimate too small");
    Ok(output)
}

/// Reads one validated file and prepares it for output (runs on a worker thread).
fn read_entry(path: &Path) -> Result<Entry> {
    let content = sanitized(read_file_safe(path)?);
    let extension = sanitized(path.extension().and_then(|e| e.to_str()).unwrap_or("txt").to_string());
    let path = sanitized(path.display().to_string());
    let fence = (longest_backtick_run(&content) + 1).max(3);
    Ok(Entry { extension, path, content, fence })
}

/// Strips control characters, reusing the string when it's already clean.
fn sanitized(s: String) -> String {
    match strip_control_chars_cow(&s) {
        Cow::Borrowed(_) => s,
        Cow::Owned(clean) => clean,
    }
}

/// Longest run of consecutive backticks. A fence must be longer than any run inside it,
/// otherwise a Markdown file with its own ``` blocks would end the fence early.
fn longest_backtick_run(s: &str) -> usize {
    let (mut longest, mut run) = (0, 0);
    for &b in s.as_bytes() {
        run = if b == b'`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    longest
}

fn push_fence(output: &mut String, len: usize) {
    output.extend(std::iter::repeat_n('`', len));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn test_aggregate_empty() {
        let result = aggregate_files(&[]);
        assert!(result.is_ok());
        let content = result.unwrap();
        assert!(content.contains("BEGINNING"));
        assert!(content.contains("END"));
    }

    #[test]
    fn layout_is_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("main.rs");
        let b = dir.path().join("Makefile");
        fs::write(&a, "fn main() {}\n").unwrap();
        fs::write(&b, "all:\n\techo hi").unwrap();
        let (ca, cb) = (a.canonicalize().unwrap(), b.canonicalize().unwrap());

        let out = aggregate_files(&[a.clone(), b.clone()]).unwrap();
        let expected = format!(
            "BEGINNING\nRead following file and answer question given:\nQUESTION\n\n```rs path={}\nfn main() {{}}\n\n```\n\n```txt path={}\nall:\n\techo hi\n```\n\nEND",
            ca.display(),
            cb.display()
        );
        assert_eq!(out, expected);

        let with_prompt = aggregate_files_with_prompt(std::slice::from_ref(&a), Some("Explain\x1B[31m")).unwrap();
        assert!(with_prompt.starts_with("BEGINNING\nRead following file and answer question given:\nExplain[31m\n\n```rs path="));
        let blank_prompt = aggregate_files_with_prompt(&[a], Some("  ")).unwrap();
        assert!(blank_prompt.starts_with("BEGINNING\nRead following file and answer question given:\n```rs path="));
    }

    #[test]
    fn fence_grows_past_backticks_in_content() {
        let dir = tempfile::tempdir().unwrap();
        let md = dir.path().join("README.md");
        fs::write(&md, "Example:\n```sh\nls\n```\n").unwrap();
        let out = aggregate_files(&[md]).unwrap();
        assert!(out.contains("\n````md path="), "{out}");
        assert!(out.contains("```sh\nls\n```\n\n````\n\nEND"), "{out}");
        assert_eq!(longest_backtick_run("a ``` b ````` c"), 5);
    }

    #[test]
    fn total_limit_fails_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        // Sparse 45 MB files: 5 of them exceed 200 MB without writing 200 MB to disk.
        let paths: Vec<PathBuf> = (0..5)
            .map(|i| {
                let p = dir.path().join(format!("big{i}.txt"));
                fs::File::create(&p).unwrap().set_len(45 * 1024 * 1024).unwrap();
                p
            })
            .collect();
        let err = aggregate_files(&paths).unwrap_err().to_string();
        assert_eq!(err, "Total size exceeds limit: 225 MB (max: 200 MB)");
    }

    #[test]
    fn output_is_sanitized_and_ordered() {
        let dir = tempfile::tempdir().unwrap();
        let paths: Vec<PathBuf> = (0..40)
            .map(|i| {
                let p = dir.path().join(format!("f{i:02}.txt"));
                fs::write(&p, format!("file {i}\x1B[0m\u{202E}")).unwrap();
                p
            })
            .collect();
        let out = aggregate_files(&paths).unwrap();
        assert!(!out.contains('\x1B') && !out.contains('\u{202E}'));
        let positions: Vec<usize> = (0..40).map(|i| out.find(&format!("file {i}[0m\n")).unwrap()).collect();
        assert!(positions.windows(2).all(|w| w[0] < w[1]), "order not preserved");
    }
}
