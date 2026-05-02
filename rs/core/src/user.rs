//! User identity resolver for CloudEvent origin stamping.
//!
//! Mirror of [`crate::host`] but for the *human* who owns this OpenStory
//! instance — a self-identifier that lets one person's events be
//! distinguished from another's when leaf nodes aggregate at a hub.
//!
//! `host` answers "which machine produced this event?". `user` answers
//! "which human did the work?". A single human runs OpenStory on multiple
//! machines (laptop + desktop + VPS) and wants their work attributed to
//! them across all of them; conversely, a single machine in a shared
//! deployment may have multiple users. The two fields are orthogonal.
//!
//! One resolution per process, cached in a `OnceLock`. The translator
//! calls [`user()`] on every event; the actual resolution + normalization
//! is paid once at first use.
//!
//! # Resolution order
//!
//! 1. `$OPEN_STORY_USER` (explicit override — always wins)
//! 2. `/etc/openstory/user` (file, if present and non-empty)
//! 3. `$USER` (env — the OS-level fallback; inside Docker this returns
//!    the container's user, which is rarely meaningful, so most users
//!    will want the override above)
//! 4. `"unknown"` (last-resort fallback; logs a warning via `eprintln!`)
//!
//! # Normalization
//!
//! Same rules as [`crate::host::normalize`]: strip a trailing `.local`,
//! replace `.`, space, and `/` with `-`, truncate to 64 bytes. Keeps the
//! value safe to compose into a NATS subject token without surprise.

use std::sync::OnceLock;

static USER: OnceLock<String> = OnceLock::new();

/// Resolved, normalized user identity for this process.
///
/// Cached after the first call — subsequent calls are a pointer deref.
pub fn user() -> &'static str {
    USER.get_or_init(|| {
        resolve_from(
            std::env::var("OPEN_STORY_USER").ok(),
            std::fs::read_to_string("/etc/openstory/user").ok(),
            std::env::var("USER").ok(),
        )
    })
}

/// Pure resolution function — no I/O, testable. Returns normalized user.
pub(crate) fn resolve_from(
    env_override: Option<String>,
    file: Option<String>,
    os_user: Option<String>,
) -> String {
    let pick = env_override
        .and_then(non_empty)
        .or_else(|| file.and_then(non_empty))
        .or_else(|| os_user.and_then(non_empty));

    match pick {
        Some(raw) => normalize(&raw),
        None => {
            eprintln!("[open-story-core::user] resolution failed; using 'unknown'");
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

/// Make a raw user identifier safe as a NATS subject token.
///
/// Reuses the [`crate::host::normalize`] rules so user and host fields
/// share a single sanitization story:
/// - Strips a single trailing `.local` (just in case — uncommon for
///   usernames but cheap to share the rule).
/// - Replaces `.`, space, and `/` with `-`.
/// - Truncates to 64 bytes (ASCII-safe).
pub(crate) fn normalize(raw: &str) -> String {
    crate::host::normalize(raw)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── normalize ──────────────────────────────────────────────────────
    // Most normalization cases are exercised in `host::tests::normalize_*`.
    // These tests pin the user-side surface so future refactors don't
    // accidentally diverge the two stories.

    #[test]
    fn normalize_passes_through_clean_username() {
        assert_eq!(normalize("katie"), "katie");
        assert_eq!(normalize("max"), "max");
    }

    #[test]
    fn normalize_replaces_spaces_in_human_name() {
        assert_eq!(normalize("Katie Loughran"), "Katie-Loughran");
    }

    #[test]
    fn normalize_replaces_email_dots() {
        assert_eq!(normalize("katie.loughran"), "katie-loughran");
    }

    #[test]
    fn normalize_handles_empty_string() {
        assert_eq!(normalize(""), "");
    }

    // ── resolve_from ───────────────────────────────────────────────────

    #[test]
    fn resolve_prefers_env_override_over_file_and_os() {
        assert_eq!(
            resolve_from(
                Some("env-user".into()),
                Some("file-user".into()),
                Some("os-user".into()),
            ),
            "env-user"
        );
    }

    #[test]
    fn resolve_prefers_file_over_os_when_env_absent() {
        assert_eq!(
            resolve_from(None, Some("file-user".into()), Some("os-user".into())),
            "file-user"
        );
    }

    #[test]
    fn resolve_falls_back_to_os_user_when_env_and_file_absent() {
        assert_eq!(resolve_from(None, None, Some("os-user".into())), "os-user");
    }

    #[test]
    fn resolve_returns_unknown_when_all_sources_absent() {
        assert_eq!(resolve_from(None, None, None), "unknown");
    }

    #[test]
    fn resolve_treats_empty_string_as_absent() {
        // Empty env falls through to file.
        assert_eq!(
            resolve_from(Some("".into()), Some("file-user".into()), None),
            "file-user"
        );
    }

    #[test]
    fn resolve_treats_whitespace_only_as_absent() {
        assert_eq!(
            resolve_from(Some("   ".into()), None, Some("os-user".into())),
            "os-user"
        );
    }

    #[test]
    fn resolve_trims_env_value() {
        assert_eq!(
            resolve_from(Some("  katie  ".into()), None, None),
            "katie"
        );
    }

    #[test]
    fn resolve_normalizes_env_value() {
        assert_eq!(
            resolve_from(Some("Katie Loughran".into()), None, None),
            "Katie-Loughran"
        );
    }

    // ── cached user() ──────────────────────────────────────────────────

    #[test]
    fn user_returns_nonempty_value() {
        let u = user();
        assert!(!u.is_empty(), "user() must never return empty");
    }

    #[test]
    fn user_is_cached_across_calls() {
        let a = user();
        let b = user();
        assert!(
            std::ptr::eq(a, b),
            "user() must return the same &'static str on every call"
        );
    }

    #[test]
    fn user_hot_path_is_cheap() {
        // Translator calls user() once per event. A million hits must
        // complete well under 50ms in debug builds (≈ 50ns/call budget)
        // so the translator stays on its performance path.
        let _ = user(); // warm the OnceLock
        let start = std::time::Instant::now();
        let mut len_accumulator = 0usize;
        for _ in 0..1_000_000 {
            len_accumulator = len_accumulator.wrapping_add(user().len());
        }
        let elapsed = start.elapsed();
        assert!(len_accumulator > 0);
        assert!(
            elapsed < std::time::Duration::from_millis(50),
            "user() hot path took {elapsed:?} for 1M calls — expected <50ms"
        );
    }
}
