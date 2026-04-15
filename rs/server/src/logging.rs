//! Logging helpers (metadata only — never log message content).

use open_story_core::cloud_event::CloudEvent;
use chrono::Local;

/// Short session ID for display (first 8 chars). UTF-8-safe.
///
/// `id[..id.len().min(8)]` panics when byte 8 falls in the middle of a
/// multi-byte char. Session IDs from agents are typically UUIDs (ASCII),
/// but custom session ids and project ids can be unicode. Bug found
/// 2026-04-15 during audit walk #9. See
/// docs/research/architecture-audit/API_WALK.md F-1.
pub fn short_id(id: &str) -> &str {
    open_story_core::strings::truncate_at_char_boundary(id, 8)
}

/// Re-export for callers that import via this module.
pub use open_story_core::strings::truncate_at_char_boundary;

/// Format a log line with timestamp, category label, and message.
pub fn log_event(category: &str, message: &str) {
    let now = Local::now().format("%H:%M:%S");
    eprintln!(
        "\x1b[2m{now}\x1b[0m \x1b[36m{category:>5}\x1b[0m {message}"
    );
}

/// Summarize a batch of CloudEvents as a compact subtype list.
/// e.g. "message.user.prompt, progress.bash"
pub fn event_type_summary(events: &[CloudEvent]) -> String {
    let types: Vec<&str> = events
        .iter()
        .map(|e| {
            e.subtype
                .as_deref()
                .unwrap_or(&e.event_type)
        })
        .collect();
    if types.is_empty() {
        return String::new();
    }
    // Deduplicate while preserving order, show counts for repeated types
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for t in &types {
        if let Some(entry) = seen.iter_mut().find(|(name, _)| name == t) {
            entry.1 += 1;
        } else {
            seen.push((t, 1));
        }
    }
    seen.iter()
        .map(|(name, count)| {
            if *count > 1 {
                format!("{name} x{count}")
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── short_id / truncate_at_char_boundary (audit walk #9) ──────────

    #[test]
    fn short_id_truncates_ascii_to_8_chars() {
        assert_eq!(short_id("0123456789abcdef"), "01234567");
    }

    #[test]
    fn short_id_returns_full_string_when_shorter_than_8() {
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id(""), "");
    }

    #[test]
    fn short_id_does_not_panic_on_multibyte_at_byte_8() {
        // BUG that this fix closes: previously `&id[..id.len().min(8)]`
        // panicked when byte 8 fell inside a multi-byte character.
        //
        // Fixture: "abc日日" — "abc" = 3 bytes, each 日 = 3 bytes.
        //   Total: 9 bytes.
        //   Byte 8 lands inside the SECOND 日 (which spans bytes 6..9).
        //   Old impl: panic with "byte index 8 is not a char boundary".
        //   New impl: backs up to char boundary (byte 6) and returns "abc日".
        //
        // I confirmed the panic with a standalone rustc binary before
        // committing the fix.
        let id = "abc日日";
        assert_eq!(id.len(), 9);
        let truncated = short_id(id);
        // Returns "abc日" — 6 bytes, the largest valid prefix ≤ 8 bytes.
        assert_eq!(truncated, "abc日");
    }

    #[test]
    fn truncate_handles_japanese_at_byte_50() {
        // 日 = 3 bytes. 17 of them = 51 bytes. Asking for 50 lands
        // mid-char; helper must back up to a boundary.
        let s = "日".repeat(17);
        assert_eq!(s.len(), 51);
        let truncated = truncate_at_char_boundary(&s, 50);
        assert!(truncated.len() <= 50);
        assert_eq!(truncated.len() % 3, 0, "must be a multiple of 3 bytes (whole 日 chars)");
        // Should be 16 日 chars = 48 bytes
        assert_eq!(truncated.chars().count(), 16);
    }

    #[test]
    fn truncate_zero_max_bytes_returns_empty() {
        assert_eq!(truncate_at_char_boundary("hello", 0), "");
    }

    #[test]
    fn truncate_max_bytes_exactly_at_boundary() {
        // 4-byte emoji, ask for exactly 4 bytes — should return the emoji
        let s = "🦀tail";
        let truncated = truncate_at_char_boundary(s, 4);
        assert_eq!(truncated, "🦀");
    }

    // ── event_type_summary basic coverage (was untested) ──────────────

    #[test]
    fn event_type_summary_empty_input_returns_empty_string() {
        assert_eq!(event_type_summary(&[]), "");
    }
}
