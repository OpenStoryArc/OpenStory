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
            time: time.unwrap_or_else(|| Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)),
            datacontenttype: "application/json".to_string(),
            data,
            subtype,
            subject,
            dataschema,
            agent,
            host: None,
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
        let cases: Vec<(&str, Option<&str>, Option<&str>, Option<&str>, Option<&str>)> = vec![
            ("all None → auto-generated id + time", None, None, None, None),
            ("explicit id",    Some("custom-id"), None, None, None),
            ("explicit time",  None, Some("2026-01-01T00:00:00Z"), None, None),
            ("explicit subject", None, None, Some("test-subject"), None),
            ("explicit schema", None, None, None, Some("https://example.com/schema")),
            ("all explicit",   Some("id-1"), Some("2026-01-01T00:00:00Z"), Some("subj"), Some("schema")),
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
            assert_eq!(ce.datacontenttype, "application/json", "{desc}: content type");
            assert_eq!(ce.source, "test-source", "{desc}: source");
            assert_eq!(ce.event_type, "io.arc.event", "{desc}: type");
            assert_eq!(ce.subtype, Some("test.subtype".to_string()), "{desc}: subtype");

            if let Some(expected_id) = eid {
                assert_eq!(ce.id, expected_id, "{desc}: explicit id");
            } else {
                assert!(!ce.id.is_empty(), "{desc}: auto-generated id should not be empty");
                assert!(ce.id.len() > 10, "{desc}: auto-generated id should be UUID-like");
            }

            if let Some(expected_time) = time {
                assert_eq!(ce.time, expected_time, "{desc}: explicit time");
            } else {
                assert!(!ce.time.is_empty(), "{desc}: auto-generated time should not be empty");
            }

            assert_eq!(ce.subject.as_deref(), subject, "{desc}: subject");
            assert_eq!(ce.dataschema.as_deref(), schema, "{desc}: dataschema");
        }
    }

    #[test]
    fn test_serialization_skips_none_fields() {
        let ce = CloudEvent::new(
            "src".into(), "io.arc.event".into(),
            EventData::new(json!({}), 0, "s".to_string()),
            None, None, None, None, None, None,
        );
        let serialized = serde_json::to_string(&ce).unwrap();
        assert!(!serialized.contains("subtype"), "subtype=None should be skipped");
        assert!(!serialized.contains("subject"), "subject=None should be skipped");
        assert!(!serialized.contains("dataschema"), "dataschema=None should be skipped");
    }

    #[test]
    fn test_serialization_includes_present_fields() {
        let ce = CloudEvent::new(
            "src".into(), "io.arc.event".into(),
            EventData::new(json!({}), 0, "s".to_string()),
            Some("test.sub".into()), None, None,
            Some("test-subject".into()), Some("https://schema".into()),
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
        let ce1 = CloudEvent::new("s".into(), "t".into(), d(), None, None, None, None, None, None);
        let ce2 = CloudEvent::new("s".into(), "t".into(), d(), None, None, None, None, None, None);
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
}
