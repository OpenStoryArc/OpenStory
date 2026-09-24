//! E-06: an unknown agent tag survives deserialization with its raw JSON
//! intact instead of dropping the whole batch (OpenActor emits
//! `agent: "openactor"` and the strict enum threw its events away), and
//! translate rejections are counted per file by reason. Scoreboard:
//! docs/research/openstory-as-node/REQUIREMENTS.md

use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use open_story_core::reader::read_new_lines;
use open_story_core::translate::TranscriptState;

fn known_event() -> CloudEvent {
    let mut payload = ClaudeCodePayload::new();
    payload.text = Some("hello".to_string());
    let data = EventData::with_payload(
        serde_json::json!({"k": 1}),
        0,
        "s".to_string(),
        AgentPayload::ClaudeCode(payload),
    );
    CloudEvent::new(
        "arc://test/s".into(),
        "io.arc.event".into(),
        data,
        Some("message.user.prompt".into()),
        Some("e1".into()),
        Some("2026-09-23T00:00:00Z".into()),
        None,
        None,
        Some("claude-code".into()),
    )
}

/// Find and rewrite the payload's variant tag anywhere in the serialized event.
fn retag(v: &mut serde_json::Value, agent: &str) -> bool {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("_variant") {
                map.insert("_variant".into(), serde_json::json!(agent));
                map.insert("step".into(), serde_json::json!("observe"));
                if let Some(meta) = map.get_mut("meta").and_then(|m| m.as_object_mut()) {
                    meta.insert("agent".into(), serde_json::json!(agent));
                }
                return true;
            }
            map.values_mut().any(|c| retag(c, agent))
        }
        serde_json::Value::Array(a) => a.iter_mut().any(|c| retag(c, agent)),
        _ => false,
    }
}

mod when_agent_is_unknown {
    use super::*;

    #[test]
    fn it_keeps_raw_and_counts_rejection() {
        // 1. Tolerance: an event tagged with an agent this build has never
        //    heard of still deserializes, names its agent, and round-trips
        //    byte for byte.
        let mut wire = serde_json::to_value(known_event()).unwrap();
        assert!(retag(&mut wire, "openactor"), "fixture carries a variant tag");
        let event: CloudEvent = serde_json::from_value(wire.clone()).expect("unknown agent is tolerated");
        let payload = event.data.agent_payload.as_ref().expect("payload kept");
        assert!(matches!(payload, AgentPayload::Unknown(_)), "{payload:?}");
        assert_eq!(payload.agent(), "openactor");
        assert_eq!(serde_json::to_value(&event).unwrap(), wire, "raw survives a round trip untouched");

        // 2. Counting: a transcript with a malformed line is read to the end,
        //    the good lines translate, and the rejection is counted by reason.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("t.jsonl");
        let fixture = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/synth_global.jsonl"),
        )
        .unwrap();
        let mut lines: Vec<&str> = fixture.lines().take(2).collect();
        lines.push("{this is not json");
        lines.push(lines[1]);
        std::fs::write(&path, lines.join("\n") + "\n").unwrap();

        let mut state = TranscriptState::new("t".into());
        let events = read_new_lines(&path, &mut state).unwrap();
        assert!(!events.is_empty(), "good lines still translate");
        assert_eq!(state.rejections.get("invalid_json"), Some(&1), "{:?}", state.rejections);
        assert_eq!(state.rejections.values().sum::<u64>(), 1, "nothing else was rejected");
    }
}
