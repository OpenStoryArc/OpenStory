//! CloudEvents 1.0 compliant struct and builder.

use chrono::Utc;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::event_data::EventData;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CloudEvent {
    pub specversion: String,
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub time: String,
    pub datacontenttype: String,
    pub data: EventData,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataschema: Option<String>,
    /// Agent platform that produced this event (e.g., "claude-code", "pi-mono").
    /// CloudEvent extension attribute — not part of the spec, but allowed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Host where the translator ran (normalized `gethostname()` or override).
    /// Stamped at event creation via [`CloudEvent::with_host`] — survives
    /// NATS replication so origin identity is preserved across leaves/hub.
    /// CloudEvent extension attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    /// Human user who owns this OpenStory instance (normalized self-identifier).
    /// Stamped at event creation via [`CloudEvent::with_user`] — orthogonal
    /// to `host`: a single user runs OpenStory on multiple machines, and a
    /// shared machine may have multiple users. Both fields together let
    /// the hub distinguish "Katie's laptop" from "Katie's desktop" from
    /// "Max's laptop". CloudEvent extension attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Person who owns this event in OpenStory's identity model — a stable
    /// UUID from the `[person]` config section, distinct from the OS-level
    /// `user` field. The person represents a sovereign human who owns a
    /// fleet of principals (devices, agents). Stamped at event creation via
    /// [`CloudEvent::with_person_id`] by the source-aware caller (watcher
    /// or hook handler) after consulting the principal resolver.
    /// CloudEvent extension attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub person_id: Option<String>,
    /// Principal that produced this event — a stable UUID identifying which
    /// device or agent in the person's fleet did the work (e.g. "MacBook
    /// running Claude Code", "Hetzner running Bobby"). Stamped at event
    /// creation via [`CloudEvent::with_principal_id`] alongside `person_id`.
    /// CloudEvent extension attribute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub principal_id: Option<String>,
}

impl CloudEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: String,
        event_type: String,
        data: EventData,
        subtype: Option<String>,
        event_id: Option<String>,
        time: Option<String>,
        subject: Option<String>,
        dataschema: Option<String>,
        agent: Option<String>,
    ) -> Self {
        Self {
            specversion: "1.0".to_string(),
            id: event_id.unwrap_or_else(|| Uuid::new_v4().to_string()),
            source,
            event_type,
            time: time
                .unwrap_or_else(|| Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            datacontenttype: "application/json".to_string(),
            data,
            subtype,
            subject,
            dataschema,
            agent,
            host: None,
            user: None,
            person_id: None,
            principal_id: None,
        }
    }

    /// Stamp the originating host onto this event. Chainable.
    ///
    /// Translators call this right after `new()` with the value from
    /// [`crate::host::host()`]. Later calls override earlier ones, which is
    /// convenient in test fixtures.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = Some(host.into());
        self
    }

    /// Stamp the originating user onto this event. Chainable.
    ///
    /// Translators call this alongside [`Self::with_host`] using the value
    /// from [`crate::user::user()`]. Same fixture-override semantics as
    /// `with_host`.
    pub fn with_user(mut self, user: impl Into<String>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Stamp the owning person's id onto this event. Chainable.
    ///
    /// Source-aware callers (watcher, hook handler) call this after
    /// translation using the `(person_id, principal_id)` returned by the
    /// principal resolver. Distinct from `with_user` (which records the
    /// OS-level user); `person_id` is OpenStory's identity-model handle.
    pub fn with_person_id(mut self, person_id: impl Into<String>) -> Self {
        self.person_id = Some(person_id.into());
        self
    }

    /// Stamp the producing principal's id onto this event. Chainable.
    ///
    /// Paired with [`Self::with_person_id`] — both come from the resolver.
    /// Identifies which device or agent in the person's fleet produced this
    /// event (e.g. "MacBook running Claude Code", "Hetzner running Bobby").
    pub fn with_principal_id(mut self, principal_id: impl Into<String>) -> Self {
        self.principal_id = Some(principal_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn test_data() -> EventData {
        EventData::new(json!({"key": "value"}), 1, "test-session".to_string())
    }

    #[test]
    fn test_new_defaults_boundary_table() {
        type DefaultsCase = (
            &'static str,
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
            Option<&'static str>,
        );
        let cases: Vec<DefaultsCase> = vec![
            (
                "all None → auto-generated id + time",
                None,
                None,
                None,
                None,
            ),
            ("explicit id", Some("custom-id"), None, None, None),
            (
                "explicit time",
                None,
                Some("2026-01-01T00:00:00Z"),
                None,
                None,
            ),
            ("explicit subject", None, None, Some("test-subject"), None),
            (
                "explicit schema",
                None,
                None,
                None,
                Some("https://example.com/schema"),
            ),
            (
                "all explicit",
                Some("id-1"),
                Some("2026-01-01T00:00:00Z"),
                Some("subj"),
                Some("schema"),
            ),
        ];

        for (desc, eid, time, subject, schema) in cases {
            let ce = CloudEvent::new(
                "test-source".into(),
                "io.arc.event".into(),
                test_data(),
                Some("test.subtype".into()),
                eid.map(|s| s.into()),
                time.map(|s| s.into()),
                subject.map(|s| s.into()),
                schema.map(|s| s.into()),
                None,
            );

            assert_eq!(ce.specversion, "1.0", "{desc}: specversion");
            assert_eq!(
                ce.datacontenttype, "application/json",
                "{desc}: content type"
            );
            assert_eq!(ce.source, "test-source", "{desc}: source");
            assert_eq!(ce.event_type, "io.arc.event", "{desc}: type");
            assert_eq!(
                ce.subtype,
                Some("test.subtype".to_string()),
                "{desc}: subtype"
            );

            if let Some(expected_id) = eid {
                assert_eq!(ce.id, expected_id, "{desc}: explicit id");
            } else {
                assert!(
                    !ce.id.is_empty(),
                    "{desc}: auto-generated id should not be empty"
                );
                assert!(
                    ce.id.len() > 10,
                    "{desc}: auto-generated id should be UUID-like"
                );
            }

            if let Some(expected_time) = time {
                assert_eq!(ce.time, expected_time, "{desc}: explicit time");
            } else {
                assert!(
                    !ce.time.is_empty(),
                    "{desc}: auto-generated time should not be empty"
                );
            }

            assert_eq!(ce.subject.as_deref(), subject, "{desc}: subject");
            assert_eq!(ce.dataschema.as_deref(), schema, "{desc}: dataschema");
        }
    }

    #[test]
    fn test_serialization_skips_none_fields() {
        let ce = CloudEvent::new(
            "src".into(),
            "io.arc.event".into(),
            EventData::new(json!({}), 0, "s".to_string()),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let serialized = serde_json::to_string(&ce).unwrap();
        assert!(
            !serialized.contains("subtype"),
            "subtype=None should be skipped"
        );
        assert!(
            !serialized.contains("subject"),
            "subject=None should be skipped"
        );
        assert!(
            !serialized.contains("dataschema"),
            "dataschema=None should be skipped"
        );
    }

    #[test]
    fn test_serialization_includes_present_fields() {
        let ce = CloudEvent::new(
            "src".into(),
            "io.arc.event".into(),
            EventData::new(json!({}), 0, "s".to_string()),
            Some("test.sub".into()),
            None,
            None,
            Some("test-subject".into()),
            Some("https://schema".into()),
            None,
        );
        let serialized = serde_json::to_string(&ce).unwrap();
        assert!(serialized.contains("\"subtype\":\"test.sub\""));
        assert!(serialized.contains("\"subject\":\"test-subject\""));
        assert!(serialized.contains("\"dataschema\":\"https://schema\""));
    }

    #[test]
    fn test_auto_generated_ids_are_unique() {
        let d = || EventData::new(json!({}), 0, "s".to_string());
        let ce1 = CloudEvent::new(
            "s".into(),
            "t".into(),
            d(),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        let ce2 = CloudEvent::new(
            "s".into(),
            "t".into(),
            d(),
            None,
            None,
            None,
            None,
            None,
            None,
        );
        assert_ne!(ce1.id, ce2.id, "auto-generated IDs should be unique");
    }

    // ── host stamping ──────────────────────────────────────────────────

    fn minimal_ce() -> CloudEvent {
        CloudEvent::new(
            "arc://test".into(),
            "io.arc.event".into(),
            test_data(),
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[test]
    fn host_defaults_to_none_from_new() {
        // New() must not set host — keeps the 52 existing call sites
        // untouched by this refactor. Only translators opt in via with_host().
        let ce = minimal_ce();
        assert!(ce.host.is_none(), "new() must default host to None");
    }

    #[test]
    fn with_host_sets_field() {
        let ce = minimal_ce().with_host("Maxs-Air");
        assert_eq!(ce.host.as_deref(), Some("Maxs-Air"));
    }

    #[test]
    fn with_host_accepts_string_and_str() {
        // impl Into<String> — both &str and String should compile.
        let _ = minimal_ce().with_host("literal");
        let owned: String = "owned".to_string();
        let _ = minimal_ce().with_host(owned);
    }

    #[test]
    fn with_host_is_chainable_and_overrides() {
        // Later call wins — useful for test fixtures.
        let ce = minimal_ce().with_host("first").with_host("second");
        assert_eq!(ce.host.as_deref(), Some("second"));
    }

    #[test]
    fn serialization_skips_host_when_none() {
        let ce = minimal_ce();
        let json = serde_json::to_string(&ce).unwrap();
        assert!(
            !json.contains("\"host\""),
            "host=None must be absent from JSON, got: {json}"
        );
    }

    #[test]
    fn serialization_includes_host_when_set() {
        let ce = minimal_ce().with_host("Maxs-Air");
        let json = serde_json::to_string(&ce).unwrap();
        assert!(
            json.contains("\"host\":\"Maxs-Air\""),
            "serialized JSON must include host field, got: {json}"
        );
    }

    #[test]
    fn host_round_trips_through_serde() {
        let ce = minimal_ce().with_host("debian-16gb-ash-1");
        let json = serde_json::to_string(&ce).unwrap();
        let round: CloudEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(round.host.as_deref(), Some("debian-16gb-ash-1"));
    }

    #[test]
    fn host_deserializes_as_none_when_absent() {
        // Pre-refactor events on disk have no host key. They must
        // deserialize cleanly with host: None.
        let json = r#"{
            "specversion":"1.0",
            "id":"evt-1",
            "source":"arc://test",
            "type":"io.arc.event",
            "time":"2026-04-21T00:00:00Z",
            "datacontenttype":"application/json",
            "data":{"raw":{},"seq":1,"session_id":"s"}
        }"#;
        let ce: CloudEvent = serde_json::from_str(json).unwrap();
        assert!(ce.host.is_none());
    }

    // ── person_id / principal_id stamping ──────────────────────────────

    #[test]
    fn person_and_principal_default_to_none_from_new() {
        // Mirror host: new() must not set them — only source-aware
        // callers opt in via the builders. Existing call sites (52+)
        // remain untouched.
        let ce = minimal_ce();
        assert!(ce.person_id.is_none(), "new() must default person_id to None");
        assert!(ce.principal_id.is_none(), "new() must default principal_id to None");
    }

    #[test]
    fn with_person_id_sets_field() {
        let ce = minimal_ce().with_person_id("p-uuid-123");
        assert_eq!(ce.person_id.as_deref(), Some("p-uuid-123"));
    }

    #[test]
    fn with_principal_id_sets_field() {
        let ce = minimal_ce().with_principal_id("k-uuid-456");
        assert_eq!(ce.principal_id.as_deref(), Some("k-uuid-456"));
    }

    #[test]
    fn person_and_principal_builders_accept_string_and_str() {
        let _ = minimal_ce().with_person_id("literal").with_principal_id("literal");
        let owned: String = "owned".to_string();
        let _ = minimal_ce()
            .with_person_id(owned.clone())
            .with_principal_id(owned);
    }

    #[test]
    fn person_and_principal_builders_chain_and_compose() {
        let ce = minimal_ce()
            .with_person_id("max-uuid")
            .with_principal_id("macbook-uuid")
            .with_host("Maxs-Air")
            .with_user("max");
        assert_eq!(ce.person_id.as_deref(), Some("max-uuid"));
        assert_eq!(ce.principal_id.as_deref(), Some("macbook-uuid"));
        assert_eq!(ce.host.as_deref(), Some("Maxs-Air"));
        assert_eq!(ce.user.as_deref(), Some("max"));
    }

    #[test]
    fn with_person_id_overrides_on_repeat() {
        let ce = minimal_ce().with_person_id("first").with_person_id("second");
        assert_eq!(ce.person_id.as_deref(), Some("second"));
    }

    #[test]
    fn serialization_skips_person_and_principal_when_none() {
        let ce = minimal_ce();
        let json = serde_json::to_string(&ce).unwrap();
        assert!(
            !json.contains("\"person_id\""),
            "person_id=None must be absent from JSON, got: {json}"
        );
        assert!(
            !json.contains("\"principal_id\""),
            "principal_id=None must be absent from JSON, got: {json}"
        );
    }

    #[test]
    fn serialization_includes_person_and_principal_when_set() {
        let ce = minimal_ce()
            .with_person_id("max-uuid")
            .with_principal_id("macbook-uuid");
        let json = serde_json::to_string(&ce).unwrap();
        assert!(json.contains("\"person_id\":\"max-uuid\""), "got: {json}");
        assert!(json.contains("\"principal_id\":\"macbook-uuid\""), "got: {json}");
    }

    #[test]
    fn person_and_principal_round_trip_through_serde() {
        let ce = minimal_ce()
            .with_person_id("max-uuid")
            .with_principal_id("macbook-uuid");
        let json = serde_json::to_string(&ce).unwrap();
        let round: CloudEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(round.person_id.as_deref(), Some("max-uuid"));
        assert_eq!(round.principal_id.as_deref(), Some("macbook-uuid"));
    }

    #[test]
    fn person_and_principal_deserialize_as_none_when_absent() {
        // Existing events on disk have neither key. They must deserialize
        // cleanly with both fields None — this is the backward-compat
        // contract for v1 of the personhood model.
        let json = r#"{
            "specversion":"1.0",
            "id":"evt-1",
            "source":"arc://test",
            "type":"io.arc.event",
            "time":"2026-04-21T00:00:00Z",
            "datacontenttype":"application/json",
            "data":{"raw":{},"seq":1,"session_id":"s"}
        }"#;
        let ce: CloudEvent = serde_json::from_str(json).unwrap();
        assert!(ce.person_id.is_none());
        assert!(ce.principal_id.is_none());
    }
}
