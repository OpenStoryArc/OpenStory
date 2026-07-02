//! Subjects for the AUTHORED ui-event namespace on the bus.
//!
//! Sovereignty invariant: authored events (interactions, control, annotations —
//! the user or an agent acting ON THE MIRROR) publish under `ui.*`, strictly
//! separate from the OBSERVED, read-only `events.*` namespace (watcher-sourced
//! agent activity). This module owns that partition so it is unit-tested by
//! construction: a ui subject ALWAYS starts with `ui.` and NEVER `events.`.
//! Nothing here can write to the observed stream — it only names ui subjects.

/// Sanitize a free-form token (an issuer/principal) into a NATS-subject-safe
/// segment: lowercase alnum / `-` / `_`, everything else → `_`, empty → `anon`.
/// Prevents a stray `.` or space from injecting extra subject levels (which is
/// how an authored token could otherwise escape the `ui.` prefix).
pub fn sanitize_token(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c.to_ascii_lowercase() } else { '_' })
        .collect();
    if t.is_empty() { "anon".to_string() } else { t }
}

/// Build the subject for an authored ui event: `ui.{principal}.{class}.{kind}`
/// (e.g. `ui.max.interaction.navigate`). NEVER `events.*` — that namespace is
/// the observed, read-only source. Every segment is sanitized so no free-form
/// input can break the partition.
pub fn ui_subject(class: &str, kind: &str, principal: Option<&str>) -> String {
    let p = principal.map(sanitize_token).unwrap_or_else(|| "anon".to_string());
    format!("ui.{}.{}.{}", p, sanitize_token(class), sanitize_token(kind))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_is_ui_namespaced_never_events() {
        let s = ui_subject("interaction", "navigate", Some("max"));
        assert_eq!(s, "ui.max.interaction.navigate");
        assert!(s.starts_with("ui."));
        assert!(!s.starts_with("events."));
    }

    #[test]
    fn missing_or_blank_principal_is_anon() {
        assert_eq!(ui_subject("interaction", "navigate", None), "ui.anon.interaction.navigate");
        assert_eq!(ui_subject("interaction", "navigate", Some("")), "ui.anon.interaction.navigate");
    }

    #[test]
    fn free_form_tokens_are_sanitized_so_they_cannot_escape_the_prefix() {
        // dots/spaces/@ would otherwise inject subject levels or break publish
        let s = ui_subject("interaction", "navigate", Some("Max Glassie@a1"));
        assert_eq!(s, "ui.max_glassie_a1.interaction.navigate");
        assert!(!s.contains(' ') && !s.contains('@'));
        // even a principal literally named "events" stays under ui.
        assert!(ui_subject("interaction", "x", Some("events")).starts_with("ui.events."));
    }

    #[test]
    fn every_authored_class_stays_ui_namespaced() {
        for class in ["interaction", "control", "annotation"] {
            let s = ui_subject(class, "x", Some("p"));
            assert!(s.starts_with("ui."), "{s} should be ui-namespaced");
            assert!(!s.starts_with("events."), "{s} must never be in the observed namespace");
        }
    }

    // Phase 1f wiring contracts: control + annotation authored surfaces publish on
    // exactly these subjects. Lock the shape so a rename can't silently drift.
    #[test]
    fn control_subject_is_keyed_by_action() {
        assert_eq!(ui_subject("control", "open_view", Some("max")), "ui.max.control.open_view");
        assert_eq!(ui_subject("control", "focus_event", None), "ui.anon.control.focus_event");
    }

    #[test]
    fn annotation_subject_is_keyed_by_add_or_remove() {
        assert_eq!(ui_subject("annotation", "add", Some("max")), "ui.max.annotation.add");
        assert_eq!(ui_subject("annotation", "remove", Some("max")), "ui.max.annotation.remove");
    }
}
