//! Subjects for the AUTHORED ui-event namespace on the bus.
//!
//! Sovereignty invariant: authored events (interactions, control, annotations —
//! the user or an agent acting ON THE MIRROR) publish under `ui.*`, strictly
//! separate from the OBSERVED, read-only `events.*` namespace (watcher-sourced
//! agent activity). This module owns that partition so it is unit-tested by
//! construction: a ui subject ALWAYS starts with `ui.` and NEVER `events.`.
//! Nothing here can write to the observed stream — it only names ui subjects.

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use serde_json::Value;

/// The synthetic session that owns authored ui events (the human's overlay-of-self).
pub const UI_SOURCE: &str = "openstory-ui";

/// Build a spec-compliant CloudEvent for an authored ui event. The free-form
/// interaction/control/annotation body rides in `EventData.raw`, so an authored
/// event is a first-class CloudEvent — the SAME `io.arc.event` type as observed
/// events; only the subtype (`interaction.*`/`control.*`/`annotation.*`) and the
/// `ui.*` subject mark it as authored. This is the typed shape the bus + MCP pump
/// consume, replacing per-call-site ad-hoc `json!` payloads.
pub fn ui_cloud_event(class: &str, kind: &str, session_id: &str, raw: Value) -> CloudEvent {
    let data = EventData::new(raw, 0, session_id.to_string());
    CloudEvent::new(
        UI_SOURCE.to_string(),
        "io.arc.event".to_string(),
        data,
        Some(format!("{class}.{kind}")),
        None,
        None,
        None,
        None,
        Some(UI_SOURCE.to_string()),
    )
}

/// The authored body carried by a ui event's `data`. Tolerant of both the proper
/// CloudEvent shape (`data.raw` = the body) and the legacy flat shape (`data` IS
/// the body), so reads survive the CloudEvent migration on mixed data.
pub fn ui_body(data: &Value) -> Value {
    match data.get("raw") {
        Some(raw) if !raw.is_null() => raw.clone(),
        _ => data.clone(),
    }
}

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

    // Phase 1g: authored events are proper CloudEvents (typed EventData envelope).
    #[test]
    fn ui_cloud_event_is_a_spec_compliant_cloudevent() {
        let body = serde_json::json!({ "kind": "navigate", "view": "canvas" });
        let ce = ui_cloud_event("interaction", "navigate", "openstory-ui", body.clone());
        let v = serde_json::to_value(&ce).unwrap();
        assert_eq!(v["specversion"], "1.0");
        assert_eq!(v["type"], "io.arc.event");
        assert_eq!(v["subtype"], "interaction.navigate");
        assert_eq!(v["datacontenttype"], "application/json");
        assert_eq!(v["agent"], "openstory-ui");
        // the authored body rides in EventData.raw, untouched
        assert_eq!(v["data"]["raw"], body);
        assert_eq!(v["data"]["session_id"], "openstory-ui");
        assert!(v["id"].is_string() && v["time"].is_string());
    }

    #[test]
    fn ui_body_unwraps_cloudevent_raw_but_tolerates_flat_legacy() {
        // proper CloudEvent shape → the body is under data.raw
        let nested = serde_json::json!({ "raw": { "kind": "select", "view": "canvas" }, "seq": 0, "session_id": "openstory-ui" });
        assert_eq!(ui_body(&nested), serde_json::json!({ "kind": "select", "view": "canvas" }));
        // legacy flat shape (data IS the body) still works
        let flat = serde_json::json!({ "kind": "navigate", "view": "story" });
        assert_eq!(ui_body(&flat), flat);
    }
}
