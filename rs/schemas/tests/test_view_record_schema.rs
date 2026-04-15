//! Schema tests for ViewRecord and WireRecord.
//!
//! WireRecord flattens ViewRecord flattens RecordBody — three nested
//! flattens plus an internally-tagged enum. SC-1 from SCHEMA_MAP.md.
//! The shape-B `wire_record_rejects_missing_record_type` test is the
//! litmus: if that tag doesn't propagate, the schema is subtly wrong.

use open_story_schemas::{canonicalize, generate, load_schema};
use open_story_views::unified::{
    AssistantMessage, ContentBlock, MessageContent, RecordBody, ToolCall, UserMessage,
};
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::WireRecord;
use serde_json::{json, Value};

// ── drift ──────────────────────────────────────────────────────────────

#[test]
fn view_record_schema_is_up_to_date() {
    let regen = generate::<ViewRecord>();
    let committed = load_schema("view_record.schema.json").expect("schema");
    assert_eq!(canonicalize(&regen), canonicalize(&committed));
}

#[test]
fn wire_record_schema_is_up_to_date() {
    let regen = generate::<WireRecord>();
    let committed = load_schema("wire_record.schema.json").expect("schema");
    assert_eq!(canonicalize(&regen), canonicalize(&committed));
}

// ── validators ─────────────────────────────────────────────────────────

fn vr_validator() -> jsonschema::Validator {
    let s = load_schema("view_record.schema.json").unwrap();
    jsonschema::validator_for(&s).unwrap()
}

fn wr_validator() -> jsonschema::Validator {
    let s = load_schema("wire_record.schema.json").unwrap();
    jsonschema::validator_for(&s).unwrap()
}

// ── fixtures ───────────────────────────────────────────────────────────

fn vr_user_message() -> ViewRecord {
    ViewRecord {
        id: "evt-user-1".into(),
        seq: 1,
        session_id: "sess".into(),
        timestamp: "2026-04-15T10:00:00Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::UserMessage(UserMessage {
            content: MessageContent::Text("hello".into()),
            images: vec![],
        }),
    }
}

fn vr_assistant_with_blocks() -> ViewRecord {
    ViewRecord {
        id: "evt-asst-1".into(),
        seq: 2,
        session_id: "sess".into(),
        timestamp: "2026-04-15T10:00:01Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::AssistantMessage(Box::new(AssistantMessage {
            model: "claude-opus-4-6".into(),
            content: vec![ContentBlock::Text { text: "hi".into() }],
            stop_reason: Some("end_turn".into()),
            end_turn: None,
            phase: None,
        })),
    }
}

fn vr_tool_call() -> ViewRecord {
    ViewRecord {
        id: "evt-tool-1".into(),
        seq: 3,
        session_id: "sess".into(),
        timestamp: "2026-04-15T10:00:02Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::ToolCall(Box::new(ToolCall {
            call_id: "toolu_x".into(),
            name: "Read".into(),
            input: json!({"file_path": "/x.rs"}),
            raw_input: json!({"file_path": "/x.rs"}),
            typed_input: None,
            status: None,
        })),
    }
}

// ── Shape B: ViewRecord good fixtures ──────────────────────────────────

#[test]
fn view_record_accepts_user_message() {
    let v = vr_validator();
    let json = serde_json::to_value(vr_user_message()).unwrap();
    assert!(v.is_valid(&json), "user_message view record must validate");
}

#[test]
fn view_record_accepts_assistant_with_content_blocks() {
    let v = vr_validator();
    let json = serde_json::to_value(vr_assistant_with_blocks()).unwrap();
    assert!(v.is_valid(&json), "assistant_message must validate");
}

#[test]
fn view_record_accepts_tool_call() {
    let v = vr_validator();
    let json = serde_json::to_value(vr_tool_call()).unwrap();
    assert!(v.is_valid(&json), "tool_call must validate");
}

#[test]
fn view_record_accepts_message_content_as_blocks() {
    // SC-2 from SCHEMA_MAP — the untagged MessageContent is the
    // highest-risk spot in the schema. Lock the Blocks arm too.
    let v = vr_validator();
    let vr = ViewRecord {
        id: "evt-u-blocks".into(),
        seq: 4,
        session_id: "sess".into(),
        timestamp: "2026-04-15T10:00:03Z".into(),
        agent_id: None,
        is_sidechain: false,
        body: RecordBody::UserMessage(UserMessage {
            content: MessageContent::Blocks(vec![ContentBlock::Text { text: "hi".into() }]),
            images: vec![],
        }),
    };
    let json = serde_json::to_value(vr).unwrap();
    assert!(v.is_valid(&json), "user_message with Blocks must validate");
}

// ── Shape C: ViewRecord bad fixtures ───────────────────────────────────

#[test]
fn view_record_rejects_unknown_record_type() {
    let v = vr_validator();
    let bad = json!({
        "id": "x",
        "seq": 1,
        "session_id": "s",
        "timestamp": "2026-04-15T00:00:00Z",
        "record_type": "oopsie_daisy",
        "payload": {}
    });
    assert!(!v.is_valid(&bad));
}

#[test]
fn view_record_rejects_content_block_with_unknown_type() {
    let v = vr_validator();
    let bad = json!({
        "id": "x",
        "seq": 1,
        "session_id": "s",
        "timestamp": "2026-04-15T00:00:00Z",
        "record_type": "assistant_message",
        "payload": {
            "model": "c",
            "content": [{"type": "weird_block", "text": "x"}]
        }
    });
    assert!(!v.is_valid(&bad), "unknown content block type must reject");
}

// ── Shape B: WireRecord (flatten composition) ──────────────────────────

fn wr_from_vr(vr: ViewRecord, depth: u16, parent_uuid: Option<&str>) -> Value {
    let wr = WireRecord {
        record: vr,
        depth,
        parent_uuid: parent_uuid.map(|s| s.to_string()),
        truncated: false,
        payload_bytes: 0,
    };
    serde_json::to_value(wr).unwrap()
}

#[test]
fn wire_record_accepts_flattened_view_record_plus_tree_meta() {
    let v = wr_validator();
    let wr = wr_from_vr(vr_assistant_with_blocks(), 2, Some("parent-1"));
    assert!(v.is_valid(&wr), "flattened wire record must validate");
    // Confirm the flatten actually landed the record_type at the top.
    assert_eq!(wr["record_type"], "assistant_message");
    assert_eq!(wr["depth"], 2);
}

#[test]
fn wire_record_accepts_root_with_null_parent_uuid() {
    let v = wr_validator();
    let wr = wr_from_vr(vr_user_message(), 0, None);
    assert!(v.is_valid(&wr));
    assert!(wr["parent_uuid"].is_null());
}

// ── Shape C: WireRecord (SC-1 — flatten composition survived) ──────────

#[test]
fn wire_record_rejects_missing_record_type() {
    // The litmus test for SC-1: if schemars didn't carry the tagged
    // RecordBody discriminator through the double flatten, this would
    // pass (permissive). It needs to reject.
    let v = wr_validator();
    let bad = json!({
        "id": "x",
        "seq": 1,
        "session_id": "s",
        "timestamp": "2026-04-15T00:00:00Z",
        "depth": 0,
        "parent_uuid": null,
        "truncated": false,
        "payload_bytes": 0
        // missing record_type + payload
    });
    assert!(!v.is_valid(&bad), "missing record_type must reject — proves flatten+tag survived generation");
}
