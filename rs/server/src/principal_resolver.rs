//! Principal resolver — maps a source signal to (person_id, principal_id).
//!
//! Used by source-aware callers (watcher, hook handler) to determine which
//! principal in a person's fleet should own an incoming event. First match
//! wins across `Person::principals` (config order is precedence).
//!
//! Falls back to a synthetic `unattributed:{person_id}` principal id if no
//! matchers match — defense against silent unattributed events. Operators
//! can grep for `unattributed:` in the store to find events that need a
//! principal definition.

use regex::Regex;

use open_story_core::cloud_event::CloudEvent;

use crate::config::{Person, PrincipalMatchers};

/// Source signal for an incoming event. Built by source-aware callers
/// (watcher knows the file path; hook handler knows the headers). All
/// fields are optional — `None` means the caller doesn't have that info.
#[derive(Debug, Clone, Default)]
pub struct SourceContext {
    pub agent: Option<String>,
    pub host: Option<String>,
    pub user: Option<String>,
    pub source_path: Option<String>,
}

/// Resolved identity to stamp on an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedIdentity {
    pub person_id: String,
    pub principal_id: String,
}

/// Resolve the (person_id, principal_id) for an incoming event.
///
/// First match wins across `person.principals`. If no principal matches,
/// returns the person's id paired with an `unattributed:{person_id}`
/// principal id — events still get tagged so we can find them later.
pub fn resolve(person: &Person, ctx: &SourceContext) -> ResolvedIdentity {
    for principal in &person.principals {
        if matches(&principal.matchers, ctx) {
            return ResolvedIdentity {
                person_id: person.id.clone(),
                principal_id: principal.id.clone(),
            };
        }
    }
    ResolvedIdentity {
        person_id: person.id.clone(),
        principal_id: format!("unattributed:{}", person.id),
    }
}

/// Resolve the batch's identity once and stamp every event with it.
///
/// Convenience wrapper for callers that already have a Vec of events from
/// a single source (the watcher closures call this). The first event's
/// `agent`/`host`/`user` provides the [`SourceContext`] for the whole
/// batch — events from one file share these fields by construction.
///
/// `source_path` is supplied by the caller (typically the watch dir) and
/// fed to any `watch_dir_pattern` matchers.
///
/// No-op when `person` is `None` (config not yet bootstrapped) or when
/// `events` is empty.
pub fn stamp_events(
    person: Option<&Person>,
    source_path: Option<String>,
    events: &mut [CloudEvent],
) {
    let Some(person) = person else { return };
    let Some(first) = events.first() else { return };
    let ctx = SourceContext {
        agent: first.agent.clone(),
        host: first.host.clone(),
        user: first.user.clone(),
        source_path,
    };
    let identity = resolve(person, &ctx);
    for ev in events.iter_mut() {
        ev.person_id = Some(identity.person_id.clone());
        ev.principal_id = Some(identity.principal_id.clone());
    }
}

/// Every `Some` matcher field must equal the corresponding ctx field.
/// `None` matchers are wildcards. An entirely-empty matcher always matches.
fn matches(matchers: &PrincipalMatchers, ctx: &SourceContext) -> bool {
    if let Some(agent) = &matchers.agent {
        if ctx.agent.as_ref() != Some(agent) {
            return false;
        }
    }
    if let Some(host) = &matchers.host {
        if ctx.host.as_ref() != Some(host) {
            return false;
        }
    }
    if let Some(user) = &matchers.user {
        if ctx.user.as_ref() != Some(user) {
            return false;
        }
    }
    if let Some(pattern) = &matchers.watch_dir_pattern {
        let Some(path) = &ctx.source_path else {
            return false;
        };
        // Bad regex must not panic — silently fails the match.
        let Ok(re) = Regex::new(pattern) else {
            return false;
        };
        if !re.is_match(path) {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Principal;

    fn person_with(principals: Vec<Principal>) -> Person {
        Person {
            id: "person-1".to_string(),
            display_name: "Test".into(),
            email: String::new(),
            principals,
        }
    }

    fn principal(id: &str, matchers: PrincipalMatchers) -> Principal {
        Principal {
            id: id.into(),
            display_name: id.into(),
            matchers,
        }
    }

    #[test]
    fn empty_matchers_match_anything() {
        let person = person_with(vec![principal("k-default", PrincipalMatchers::default())]);
        let ctx = SourceContext::default();
        let r = resolve(&person, &ctx);
        assert_eq!(r.person_id, "person-1");
        assert_eq!(r.principal_id, "k-default");
    }

    #[test]
    fn host_matcher_matches_exact_host() {
        let person = person_with(vec![principal(
            "k-laptop",
            PrincipalMatchers {
                host: Some("Maxs-Air".into()),
                ..Default::default()
            },
        )]);
        let ctx = SourceContext {
            host: Some("Maxs-Air".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&person, &ctx).principal_id, "k-laptop");
    }

    #[test]
    fn host_matcher_rejects_different_host() {
        let person = person_with(vec![principal(
            "k-laptop",
            PrincipalMatchers {
                host: Some("Maxs-Air".into()),
                ..Default::default()
            },
        )]);
        let ctx = SourceContext {
            host: Some("Other-Host".into()),
            ..Default::default()
        };
        let r = resolve(&person, &ctx);
        assert!(
            r.principal_id.starts_with("unattributed:"),
            "no match should fall back to unattributed, got: {}",
            r.principal_id
        );
    }

    #[test]
    fn first_matching_principal_wins() {
        // Both could match; the catch-all is second, so the specific wins.
        let person = person_with(vec![
            principal(
                "k-specific",
                PrincipalMatchers {
                    host: Some("H".into()),
                    ..Default::default()
                },
            ),
            principal("k-catch-all", PrincipalMatchers::default()),
        ]);
        let ctx = SourceContext {
            host: Some("H".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&person, &ctx).principal_id, "k-specific");
    }

    #[test]
    fn catch_all_falls_through_after_non_matching_specific() {
        let person = person_with(vec![
            principal(
                "k-specific",
                PrincipalMatchers {
                    host: Some("OnlyThisHost".into()),
                    ..Default::default()
                },
            ),
            principal("k-catch-all", PrincipalMatchers::default()),
        ]);
        let ctx = SourceContext {
            host: Some("DifferentHost".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&person, &ctx).principal_id, "k-catch-all");
    }

    #[test]
    fn all_fields_must_match_for_compound_matcher() {
        let person = person_with(vec![principal(
            "k-strict",
            PrincipalMatchers {
                host: Some("H".into()),
                user: Some("U".into()),
                agent: Some("claude-code".into()),
                watch_dir_pattern: None,
            },
        )]);
        let full = SourceContext {
            host: Some("H".into()),
            user: Some("U".into()),
            agent: Some("claude-code".into()),
            source_path: None,
        };
        assert_eq!(resolve(&person, &full).principal_id, "k-strict");

        // Drop one — should not match.
        let partial = SourceContext {
            agent: None,
            ..full
        };
        assert!(resolve(&person, &partial)
            .principal_id
            .starts_with("unattributed:"));
    }

    #[test]
    fn watch_dir_pattern_matches_regex() {
        let person = person_with(vec![principal(
            "k-claude",
            PrincipalMatchers {
                watch_dir_pattern: Some(r".*\.claude/projects/.*".into()),
                ..Default::default()
            },
        )]);
        let ctx_match = SourceContext {
            source_path: Some("/Users/max/.claude/projects/foo/session.jsonl".into()),
            ..Default::default()
        };
        assert_eq!(resolve(&person, &ctx_match).principal_id, "k-claude");

        let ctx_miss = SourceContext {
            source_path: Some("/Users/max/.pi/agent/sessions/x.jsonl".into()),
            ..Default::default()
        };
        assert!(resolve(&person, &ctx_miss)
            .principal_id
            .starts_with("unattributed:"));
    }

    #[test]
    fn watch_dir_pattern_with_no_source_path_does_not_match() {
        let person = person_with(vec![principal(
            "k-claude",
            PrincipalMatchers {
                watch_dir_pattern: Some(r".*".into()),
                ..Default::default()
            },
        )]);
        let ctx = SourceContext::default(); // no source_path
        assert!(resolve(&person, &ctx)
            .principal_id
            .starts_with("unattributed:"));
    }

    #[test]
    fn invalid_regex_does_not_panic_does_not_match() {
        let person = person_with(vec![principal(
            "k-bad",
            PrincipalMatchers {
                watch_dir_pattern: Some("[".into()), // malformed
                ..Default::default()
            },
        )]);
        let ctx = SourceContext {
            source_path: Some("anything".into()),
            ..Default::default()
        };
        let r = resolve(&person, &ctx);
        assert!(
            r.principal_id.starts_with("unattributed:"),
            "bad regex must not match, got: {}",
            r.principal_id
        );
    }

    #[test]
    fn unattributed_fallback_uses_person_id_in_principal() {
        let person = person_with(vec![]); // no principals at all
        let ctx = SourceContext::default();
        let r = resolve(&person, &ctx);
        assert_eq!(r.person_id, "person-1");
        assert_eq!(r.principal_id, "unattributed:person-1");
    }

    // ── stamp_events ────────────────────────────────────────────────────

    use open_story_core::cloud_event::CloudEvent;
    use open_story_core::event_data::EventData;

    fn ce_with(agent: Option<&str>, host: Option<&str>, user: Option<&str>) -> CloudEvent {
        let mut ce = CloudEvent::new(
            "src".into(),
            "io.arc.event".into(),
            EventData::new(serde_json::json!({}), 0, "s".to_string()),
            None,
            None,
            None,
            None,
            None,
            agent.map(|s| s.to_string()),
        );
        if let Some(h) = host {
            ce = ce.with_host(h);
        }
        if let Some(u) = user {
            ce = ce.with_user(u);
        }
        ce
    }

    #[test]
    fn stamp_events_writes_person_and_principal_to_every_event() {
        let person = person_with(vec![principal(
            "k-laptop",
            PrincipalMatchers {
                host: Some("Maxs-Air".into()),
                ..Default::default()
            },
        )]);
        let mut events = vec![
            ce_with(Some("claude-code"), Some("Maxs-Air"), Some("max")),
            ce_with(Some("claude-code"), Some("Maxs-Air"), Some("max")),
        ];
        stamp_events(Some(&person), None, &mut events);
        for ev in &events {
            assert_eq!(ev.person_id.as_deref(), Some("person-1"));
            assert_eq!(ev.principal_id.as_deref(), Some("k-laptop"));
        }
    }

    #[test]
    fn stamp_events_no_op_when_person_is_none() {
        let mut events = vec![ce_with(Some("claude-code"), Some("H"), Some("U"))];
        stamp_events(None, None, &mut events);
        assert!(events[0].person_id.is_none());
        assert!(events[0].principal_id.is_none());
    }

    #[test]
    fn stamp_events_no_op_on_empty_batch() {
        let person = person_with(vec![principal("k", PrincipalMatchers::default())]);
        let mut events: Vec<CloudEvent> = Vec::new();
        stamp_events(Some(&person), None, &mut events); // must not panic
        assert!(events.is_empty());
    }

    #[test]
    fn stamp_events_uses_source_path_for_watch_dir_pattern() {
        let person = person_with(vec![principal(
            "k-claude",
            PrincipalMatchers {
                watch_dir_pattern: Some(r".*\.claude.*".into()),
                ..Default::default()
            },
        )]);
        let mut events = vec![ce_with(Some("claude-code"), Some("h"), Some("u"))];
        stamp_events(
            Some(&person),
            Some("/Users/max/.claude/projects".into()),
            &mut events,
        );
        assert_eq!(events[0].principal_id.as_deref(), Some("k-claude"));
    }
}
