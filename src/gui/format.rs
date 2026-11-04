//! Small text helpers for the interface. Pure functions, so they're unit-tested.

/// `512 B`, `12.4 KB`, `3.2 MB` (1 KB = 1024 B, like the size limits).
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if bytes < 1024 {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{:.1} MB", b / (KB * KB))
    }
}

/// Rough token count for AI prompts: ~4 bytes per token for text. Binary files are
/// Base64-encoded first (4/3 larger) and Base64 tokenizes worse (~3 characters per token).
pub fn estimate_tokens(bytes: u64, binary: bool) -> u64 {
    if binary { bytes.div_ceil(3) * 4 / 3 } else { bytes.div_ceil(4) }
}

/// `950`, `1.2k`, `48k`, `1.3M`.
pub fn compact_count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..10_000 => format!("{:.1}k", n as f64 / 1e3),
        10_000..1_000_000 => format!("{}k", (n as f64 / 1e3).round() as u64),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// Keeps the *end* of `s` (the part of a folder path that tells folders apart):
/// `/home/me/projects/app/src` -> `…/app/src`. Counts characters, never splits one.
pub fn elide_start(s: &str, max_chars: usize) -> String {
    let count = s.chars().count();
    if count <= max_chars {
        return s.to_string();
    }
    if max_chars == 0 {
        return String::new();
    }
    let keep = max_chars - 1; // room for the ellipsis
    let tail: String = s.chars().skip(count - keep).collect();
    // Prefer cutting at a separator so the visible part starts with a whole folder name.
    match tail.find(['/', '\\']) {
        Some(i) if i + 1 < tail.len() && i < keep / 2 => format!("…{}", &tail[i..]),
        _ => format!("…{tail}"),
    }
}

/// Short type label for the badge in front of a file name: `RS`, `TOML`, `MD`.
pub fn badge(extension: Option<&str>) -> String {
    match extension {
        Some(ext) if !ext.is_empty() => ext.chars().take(4).flat_map(char::to_uppercase).collect(),
        _ => "TXT".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(1023), "1023 B");
        assert_eq!(human_size(1024), "1.0 KB");
        assert_eq!(human_size(12_700), "12.4 KB");
        assert_eq!(human_size(50 * 1024 * 1024), "50.0 MB");
    }

    #[test]
    fn tokens() {
        assert_eq!(estimate_tokens(0, false), 0);
        assert_eq!(estimate_tokens(4, false), 1);
        assert_eq!(estimate_tokens(5, false), 2);
        assert_eq!(estimate_tokens(3000, true), 1333);
        assert_eq!(compact_count(950), "950");
        assert_eq!(compact_count(1234), "1.2k");
        assert_eq!(compact_count(48_400), "48k");
        assert_eq!(compact_count(1_250_000), "1.2M");
    }

    #[test]
    fn elision_is_char_safe_and_keeps_the_end() {
        assert_eq!(elide_start("/a/b", 10), "/a/b");
        assert_eq!(elide_start("/home/me/projects/app/src", 12), "…/app/src");
        // Multi-byte names: never panics, never splits a character.
        let cyrillic = "/home/пользователь/документы/проект";
        let short = elide_start(cyrillic, 12);
        assert!(short.starts_with('…') && short.chars().count() <= 12, "{short}");
        assert!(cyrillic.ends_with(short.trim_start_matches('…')));
        assert_eq!(elide_start("日本語のフォルダ名", 4), "…ルダ名");
        assert_eq!(elide_start("abc", 0), "");
    }

    #[test]
    fn badges() {
        assert_eq!(badge(Some("rs")), "RS");
        assert_eq!(badge(Some("toml")), "TOML");
        assert_eq!(badge(Some("markdown")), "MARK");
        assert_eq!(badge(None), "TXT");
    }
}
