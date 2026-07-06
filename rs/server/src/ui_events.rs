//! Subjects for the AUTHORED ui-event namespace on the bus.
//!
//! Sovereignty invariant: authored events (interactions, control, annotations —
//! the user or an agent acting ON THE MIRROR) publish under `ui.*`, strictly
//! separate from the OBSERVED, read-only `events.*` namespace (watcher-sourced
//! agent activity). This module owns that partition so it is unit-tested by
//! construction: a ui subject ALWAYS starts with `ui.` and NEVER `events.`.
//! Nothing here can write to the observed stream — it only names ui subjects.

use open_story_bus::IngestBatch;
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

/// Wrap an authored CloudEvent in a single-event IngestBatch for TYPED publish
/// on the `ui` stream — the same envelope observed events use, so the MCP
/// consumes `ui.*` through the identical typed pump (no raw-bytes special-case).
pub fn ui_batch(ce: CloudEvent) -> IngestBatch {
    IngestBatch {
        session_id: UI_SOURCE.to_string(),
        project_id: UI_SOURCE.to_string(),
        events: vec![ce],
    }
}

/// Below this idle gap (ms) the user is considered actively engaged.
pub const IDLE_THRESHOLD_MS: i64 = 8_000;

/// Attention-aware pacing rollup — mirrors ui/src/lib/tempo-profile.ts. So an
/// agent can act in the user's RESTS: poll this, drive only when `active_now` is
/// false. Serialized under `tempo` on GET /api/ui-state.
#[derive(Debug, serde::Serialize)]
pub struct Tempo {
    /// True when the last interaction is within IDLE_THRESHOLD_MS of now.
    pub active_now: bool,
    /// Epoch ms of the most recent interaction (None if none).
    pub last_activity_ms: Option<i64>,
    /// How long the user has rested (now - last); None if no activity.
    pub rest_ms: Option<i64>,
    /// Median inter-interaction gap (the rhythm); None if fewer than two.
    pub cadence_ms: Option<i64>,
}

/// Build the tempo profile from interaction event times (epoch ms) at `now_ms`.
/// Pure + order-independent so it's tested without a store.
pub fn tempo_profile(mut times_ms: Vec<i64>, now_ms: i64) -> Tempo {
    times_ms.sort_unstable();
    let Some(&last) = times_ms.last() else {
        return Tempo { active_now: false, last_activity_ms: None, rest_ms: None, cadence_ms: None };
    };
    let rest = now_ms - last;
    let cadence = if times_ms.len() >= 2 {
        let mut gaps: Vec<i64> = times_ms.windows(2).map(|w| w[1] - w[0]).collect();
        gaps.sort_unstable();
        let mid = gaps.len() / 2;
        Some(if gaps.len() % 2 == 1 { gaps[mid] } else { (gaps[mid - 1] + gaps[mid]) / 2 })
    } else {
        None
    };
    Tempo { active_now: rest < IDLE_THRESHOLD_MS, last_activity_ms: Some(last), rest_ms: Some(rest), cadence_ms: cadence }
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

    // Phase 3: attention-aware pacing rollup.
    #[test]
    fn tempo_empty_is_inactive_with_no_activity() {
        let t = tempo_profile(vec![], 1_000_000);
        assert!(!t.active_now);
        assert_eq!(t.last_activity_ms, None);
        assert_eq!(t.rest_ms, None);
        assert_eq!(t.cadence_ms, None);
    }

    #[test]
    fn tempo_recent_activity_is_active() {
        let now = 1_000_000;
        let t = tempo_profile(vec![now - 30_000, now - 2_000], now);
        assert!(t.active_now); // 2s < 8s threshold
        assert_eq!(t.rest_ms, Some(2_000));
        assert_eq!(t.last_activity_ms, Some(now - 2_000));
    }

    #[test]
    fn tempo_long_gap_is_resting() {
        let now = 1_000_000;
        let t = tempo_profile(vec![now - 20_000], now);
        assert!(!t.active_now); // 20s > 8s threshold
        assert_eq!(t.rest_ms, Some(20_000));
    }

    #[test]
    fn tempo_cadence_is_median_gap() {
        let now = 1_000_000;
        // times with gaps 1000, 3000, 1000 → median 1000
        let t = tempo_profile(vec![now - 6_000, now - 5_000, now - 2_000, now - 1_000], now);
        assert_eq!(t.cadence_ms, Some(1_000));
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
