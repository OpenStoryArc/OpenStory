//! CloudEvents 1.0 compliant struct and builder.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudEvent {
    pub specversion: String,
    pub id: String,
    pub source: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub time: String,
    pub datacontenttype: String,
    pub data: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dataschema: Option<String>,
}

impl CloudEvent {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        source: String,
        event_type: String,
        data: serde_json::Value,
        subtype: Option<String>,
        event_id: Option<String>,
        time: Option<String>,
        subject: Option<String>,
        dataschema: Option<String>,
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
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_new_defaults_boundary_table() {
        // Test that optional fields get correct defaults
        let cases: Vec<(&str, Option<&str>, Option<&str>, Option<&str>, Option<&str>)> = vec![
            // (description, event_id, time, subject, dataschema)
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
                json!({"key": "value"}),
                Some("test.subtype".into()),
                eid.map(|s| s.into()),
                time.map(|s| s.into()),
                subject.map(|s| s.into()),
                schema.map(|s| s.into()),
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
            "src".into(), "io.arc.event".into(), json!({}),
            None, None, None, None, None,
        );
        let serialized = serde_json::to_string(&ce).unwrap();
        assert!(!serialized.contains("subtype"), "subtype=None should be skipped");
        assert!(!serialized.contains("subject"), "subject=None should be skipped");
        assert!(!serialized.contains("dataschema"), "dataschema=None should be skipped");
    }

    #[test]
    fn test_serialization_includes_present_fields() {
        let ce = CloudEvent::new(
            "src".into(), "io.arc.event".into(), json!({}),
            Some("test.sub".into()), None, None,
            Some("test-subject".into()), Some("https://schema".into()),
        );
        let serialized = serde_json::to_string(&ce).unwrap();
        assert!(serialized.contains("\"subtype\":\"test.sub\""));
        assert!(serialized.contains("\"subject\":\"test-subject\""));
        assert!(serialized.contains("\"dataschema\":\"https://schema\""));
    }

    #[test]
    fn test_auto_generated_ids_are_unique() {
        let ce1 = CloudEvent::new("s".into(), "t".into(), json!({}), None, None, None, None, None);
        let ce2 = CloudEvent::new("s".into(), "t".into(), json!({}), None, None, None, None, None);
        assert_ne!(ce1.id, ce2.id, "auto-generated IDs should be unique");
    }
}
