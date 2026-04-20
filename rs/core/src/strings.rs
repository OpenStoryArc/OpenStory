//! UTF-8-safe string helpers shared across crates.
//!
//! Extracted 2026-04-15 during audit walks #9 (api) and #10 (sentence)
//! after finding two identical `truncate_str` / `truncate_at_char_boundary`
//! implementations in `rs/server/src/logging.rs` and `rs/patterns/src/
//! sentence.rs`. The duplicates were both correct (used `is_char_boundary`
//! to back up to a valid prefix) — but two copies of the same logic in
//! two crates is the same drift pattern documented in
//! IS_EPHEMERAL_DIVERGENCE.md and the subagent-detection consolidation.

/// Truncate a string slice at the largest char boundary ≤ `max_bytes`.
///
/// `&s[..max_bytes]` panics when `max_bytes` lands inside a multi-byte
/// UTF-8 character. This helper backs up to a valid char boundary. ASCII
/// inputs are unchanged. Empty input or `max_bytes == 0` returns `""`.
pub fn truncate_at_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shorter_than_max_returns_input_unchanged() {
        assert_eq!(truncate_at_char_boundary("abc", 10), "abc");
        assert_eq!(truncate_at_char_boundary("", 10), "");
    }

    #[test]
    fn ascii_truncates_at_max() {
        assert_eq!(truncate_at_char_boundary("abcdefghij", 5), "abcde");
    }

    #[test]
    fn multibyte_backs_up_to_char_boundary() {
        // "abc日日" = 9 bytes. byte 8 is mid-second-日 (bytes 6..9).
        // Truncating to 8 bytes should back up to byte 6 → "abc日".
        let s = "abc日日";
        assert_eq!(s.len(), 9);
        let truncated = truncate_at_char_boundary(s, 8);
        assert_eq!(truncated, "abc日");
        assert_eq!(truncated.len(), 6);
    }

    #[test]
    fn zero_max_returns_empty() {
        assert_eq!(truncate_at_char_boundary("hello", 0), "");
    }

    #[test]
    fn exact_boundary_returns_full_char() {
        // 4-byte emoji + tail. Asking for exactly 4 bytes.
        let s = "🦀tail";
        assert_eq!(truncate_at_char_boundary(s, 4), "🦀");
    }

    #[test]
    fn does_not_panic_on_emoji_at_boundary() {
        // Earlier bug surfaced in audit walk #9 — verify we never panic.
        let s = "🦀🦀x"; // 9 bytes
        let _ = truncate_at_char_boundary(s, 8); // [..8] is a boundary
        let _ = truncate_at_char_boundary(s, 7); // backs up to 4
        let _ = truncate_at_char_boundary(s, 1); // backs up to 0
    }
}
