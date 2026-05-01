//! Host identity resolver for CloudEvent origin stamping.
//!
//! One resolution per process, cached in a `OnceLock`. The translator calls
//! [`host()`] on every event; the actual resolution + normalization is paid
//! once at first use.
//!
//! # Resolution order
//!
//! 1. `$OPEN_STORY_HOST` (explicit override — always wins)
//! 2. `/etc/openstory/host` (file, if present and non-empty)
//! 3. `hostname::get()` (the standard `gethostname(2)` call)
//! 4. `"unknown"` (last-resort fallback; logs a warning via `eprintln!`)
//!
//! # Normalization
//!
//! The resolved value is normalized so it can safely become a NATS subject
//! token later: strip a trailing `.local`, replace `.`, space, and `/` with
//! `-`, and truncate to 64 bytes. See [`normalize`].

use std::sync::OnceLock;

static HOST: OnceLock<String> = OnceLock::new();

/// Resolved, normalized host name for this process.
///
/// Cached after the first call — subsequent calls are a pointer deref.
pub fn host() -> &'static str {
    HOST.get_or_init(|| {
        resolve_from(
            std::env::var("OPEN_STORY_HOST").ok(),
            std::fs::read_to_string("/etc/openstory/host").ok(),
            hostname::get().ok().and_then(|h| h.into_string().ok()),
        )
    })
}

/// Pure resolution function — no I/O, testable. Returns normalized host.
pub(crate) fn resolve_from(
    env: Option<String>,
    file: Option<String>,
    sys_hostname: Option<String>,
) -> String {
    let pick = env
        .and_then(non_empty)
        .or_else(|| file.and_then(non_empty))
        .or_else(|| sys_hostname.and_then(non_empty));

    match pick {
        Some(raw) => normalize(&raw),
        None => {
            eprintln!("[open-story-core::host] resolution failed; using 'unknown'");
            "unknown".to_string()
        }
    }
}

fn non_empty(s: String) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Make a raw hostname safe as a NATS subject token.
///
/// - Strips a single trailing `.local` (macOS mDNS noise).
/// - Replaces `.`, space, and `/` with `-`.
/// - Truncates to 64 bytes (ASCII-safe).
pub(crate) fn normalize(raw: &str) -> String {
    let trimmed = raw.strip_suffix(".local").unwrap_or(raw);
    let replaced: String = trimmed
        .chars()
        .map(|c| if c == '.' || c == ' ' || c == '/' { '-' } else { c })
        .collect();
    if replaced.len() <= 64 {
        replaced
    } else {
        replaced.chars().take(64).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize ──────────────────────────────────────────────────────

    #[test]
    fn normalize_passes_through_clean_name() {
        assert_eq!(normalize("Maxs-Air"), "Maxs-Air");
        assert_eq!(normalize("debian-16gb-ash-1"), "debian-16gb-ash-1");
    }

    #[test]
    fn normalize_strips_trailing_local() {
        assert_eq!(normalize("Maxs-MacBook-Pro.local"), "Maxs-MacBook-Pro");
    }

    #[test]
    fn normalize_strips_only_trailing_local_not_middle() {
        // '.local' that isn't the very tail must not be stripped — it's
        // the dot-replacement rule's job to make it subject-safe.
        assert_eq!(normalize("local.foo"), "local-foo");
        assert_eq!(normalize("a.local.b"), "a-local-b");
    }

    #[test]
    fn normalize_replaces_dots_inside() {
        assert_eq!(normalize("host.with.dots"), "host-with-dots");
    }

    #[test]
    fn normalize_replaces_spaces() {
        assert_eq!(normalize("Maxs MacBook"), "Maxs-MacBook");
    }

    #[test]
    fn normalize_replaces_slashes() {
        assert_eq!(normalize("weird/name"), "weird-name");
    }

    #[test]
    fn normalize_truncates_to_64_bytes() {
        let long = "a".repeat(200);
        let out = normalize(&long);
        assert_eq!(out.len(), 64);
        assert!(out.chars().all(|c| c == 'a'));
    }

    #[test]
    fn normalize_handles_empty_string() {
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn normalize_combined_cases() {
        // All rules at once.
        assert_eq!(
            normalize("Max.MacBook Pro/work.local"),
            "Max-MacBook-Pro-work"
        );
    }

    // ── resolve_from ───────────────────────────────────────────────────

    #[test]
    fn resolve_prefers_env_over_file_and_hostname() {
        assert_eq!(
            resolve_from(
                Some("env-host".into()),
                Some("file-host".into()),
                Some("sys-host".into()),
            ),
            "env-host"
        );
    }

    #[test]
    fn resolve_prefers_file_over_hostname_when_env_absent() {
        assert_eq!(
            resolve_from(None, Some("file-host".into()), Some("sys-host".into())),
            "file-host"
        );
    }

    #[test]
    fn resolve_falls_back_to_hostname_when_env_and_file_absent() {
        assert_eq!(
            resolve_from(None, None, Some("sys-host".into())),
            "sys-host"
        );
    }

    #[test]
    fn resolve_returns_unknown_when_all_sources_absent() {
        assert_eq!(resolve_from(None, None, None), "unknown");
    }

    #[test]
    fn resolve_treats_empty_string_as_absent() {
        // Empty env falls through to file.
        assert_eq!(
            resolve_from(Some("".into()), Some("file-host".into()), None),
            "file-host"
        );
    }

    #[test]
    fn resolve_treats_whitespace_only_as_absent() {
        // Whitespace-only env falls through to hostname.
        assert_eq!(
            resolve_from(Some("   ".into()), None, Some("sys-host".into())),
            "sys-host"
        );
    }

    #[test]
    fn resolve_trims_env_value() {
        // Padding around a real value is stripped.
        assert_eq!(
            resolve_from(Some("  env-host  ".into()), None, None),
            "env-host"
        );
    }

    #[test]
    fn resolve_normalizes_env_value() {
        assert_eq!(
            resolve_from(Some("Some.Machine.local".into()), None, None),
            "Some-Machine"
        );
    }

    #[test]
    fn resolve_normalizes_hostname_value() {
        assert_eq!(
            resolve_from(None, None, Some("Maxs-MacBook-Pro.local".into())),
            "Maxs-MacBook-Pro"
        );
    }

    // ── cached host() ──────────────────────────────────────────────────

    #[test]
    fn host_returns_nonempty_value() {
        let h = host();
        assert!(!h.is_empty(), "host() must never return empty");
    }

    #[test]
    fn host_is_cached_across_calls() {
        let a = host();
        let b = host();
        assert!(
            std::ptr::eq(a, b),
            "host() must return the same &'static str on every call"
        );
    }

    #[test]
    fn host_hot_path_is_cheap() {
        // Translator calls host() once per event. A million hits must
        // complete well under 50ms in debug builds (≈ 50ns/call budget)
        // so the translator stays on its performance path.
        let _ = host(); // warm the OnceLock
        let start = std::time::Instant::now();
        let mut len_accumulator = 0usize;
        for _ in 0..1_000_000 {
            len_accumulator = len_accumulator.wrapping_add(host().len());
        }
        let elapsed = start.elapsed();
        // Prevent the optimizer from eliding the loop.
        assert!(len_accumulator > 0);
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "host() hot path took {elapsed:?} for 1M calls — expected <50ms"
        );
    }
}
