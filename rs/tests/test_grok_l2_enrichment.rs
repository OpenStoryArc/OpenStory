//! BDD: Grok L2 enrichment — session artifacts become CloudEvents that
//! match what Grok wrote on disk (observe, never invent).
//!
//! Spec shape: `describe("when X") / it("should Y")` with correctness
//! assertions on values, not mere presence.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use open_story::paths::session_id_from_path;
use open_story::translate::TranscriptState;
use open_story::watcher;
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::AgentPayload;
use open_story_core::translate_grok::translate_grok_line;
use open_story_core::translate_grok_l2::{
    translate_hunk_record, translate_summary_json, translate_terminal_log,
};
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use serde_json::json;

const SESSION: &str = "019f6d6e-ada9-7240-b26c-036ec49af2a7";

/// Build a minimal Grok session tree under a temp watch root.
///
/// ```text
/// {watch}/%2Fproj/{SESSION}/
///   updates.jsonl
///   hunk_records.jsonl
///   summary.json
///   terminal/call-shell-1.log
/// ```
fn seed_session_tree(watch: &Path, log_body: &str) -> PathBuf {
    let sess = watch
        .join("%2FUsers%2Fmaxglassie%2Fprojects%2FOpenStory")
        .join(SESSION);
    fs::create_dir_all(sess.join("terminal")).unwrap();

    let updates = json!({
        "timestamp": 1784247414,
        "method": "session/update",
        "params": {
            "sessionId": SESSION,
            "update": {
                "sessionUpdate": "user_message_chunk",
                "content": {"type": "text", "text": "run a command"}
            },
            "_meta": {"eventId": format!("{SESSION}-user-1")}
        }
    });
    fs::write(
        sess.join("updates.jsonl"),
        format!("{}\n", updates),
    )
    .unwrap();

    let hunk = json!({
        "hunkId": "hunk-bdd-1",
        "filePath": "/Users/maxglassie/projects/OpenStory/rs/core/src/translate_grok_l2.rs",
        "hunkStart": 1,
        "hunkEnd": 40,
        "linesAdded": 40,
        "linesRemoved": 0,
        "timestamp": "2026-07-17T12:00:00Z"
    });
    fs::write(sess.join("hunk_records.jsonl"), format!("{}\n", hunk)).unwrap();

    fs::write(
        sess.join("summary.json"),
        json!({
            "session_summary": "Grok L2 BDD session",
            "current_model_id": "grok-4.5",
            "num_messages": 4,
            "updated_at": "2026-07-17T12:00:00Z"
        })
        .to_string(),
    )
    .unwrap();

    fs::write(sess.join("terminal").join("call-shell-1.log"), log_body).unwrap();

    sess
}

fn grok_text(ev: &CloudEvent) -> &str {
    ev.data
        .agent_payload
        .as_ref()
        .and_then(|p| p.text())
        .unwrap_or("")
}

fn is_subtype(ev: &CloudEvent, subtype: &str) -> bool {
    ev.subtype.as_deref() == Some(subtype)
}

// ── describe("when Grok Bash ACP tool_result completes with byte-array rawOutput") ──

mod when_bash_acp_tool_result_completes {
    use super::*;

    #[test]
    fn it_should_emit_readable_text_matching_the_shell_output_not_a_byte_array_dump() {
        let mut st = TranscriptState::new(SESSION.into());
        let call = json!({
            "timestamp": 1,
            "method": "session/update",
            "params": {
                "sessionId": SESSION,
                "update": {
                    "sessionUpdate": "tool_call",
                    "toolCallId": "call-shell-1",
                    "rawInput": {"command": "echo hi"},
                    "_meta": {"x.ai/tool": {"name": "run_terminal_command"}}
                },
                "_meta": {"eventId": "c1"}
            }
        });
        let done = json!({
            "timestamp": 2,
            "method": "session/update",
            "params": {
                "sessionId": SESSION,
                "update": {
                    "sessionUpdate": "tool_call_update",
                    "toolCallId": "call-shell-1",
                    "status": "completed",
                    "content": [{
                        "type": "content",
                        "content": {"type": "text", "text": "hi\n"}
                    }],
                    "rawOutput": {
                        "type": "Bash",
                        "output": [104, 105, 10],
                        "command": "echo hi"
                    }
                },
                "_meta": {"eventId": "d1"}
            }
        });
        let _ = translate_grok_line(&call, &mut st);
        let events = translate_grok_line(&done, &mut st);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].subtype.as_deref(), Some("message.user.tool_result"));
        assert_eq!(grok_text(&events[0]), "hi\n");
        assert!(
            !grok_text(&events[0]).contains("\"output\":["),
            "OpenStory event text must match the log, not rawOutput JSON"
        );
    }
}

// ── describe("when a Grok session directory is backfilled by the watcher") ──

mod when_grok_session_dir_is_backfilled {
    use super::*;

    #[test]
    fn it_should_emit_events_whose_text_matches_terminal_log_and_hunk_path() {
        let dir = tempfile::tempdir().unwrap();
        let log_body = "exit: 0\nhello from shell for BDD\n";
        let _sess = seed_session_tree(dir.path(), log_body);

        // Capture watcher output as CloudEvent JSONL — the public seam.
        let out = dir.path().join("emitted.jsonl");
        let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
        let n = watcher::backfill(dir.path(), &mut states, Some(&out), false).expect("backfill");
        assert!(
            n >= 4,
            "backfill should emit prompt + hunk + terminal + summary (≥4), got {n}"
        );
        assert!(out.exists(), "watcher must write events to output file");

        let collected: Vec<CloudEvent> = fs::read_to_string(&out)
            .unwrap()
            .lines()
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_str(l).expect("CloudEvent line"))
            .collect();

        assert_eq!(
            collected.len() as u64,
            n,
            "emitted file line count must match backfill return"
        );

        // --- correctness: every event is Grok-tagged for this session ---
        for ev in &collected {
            assert_eq!(
                ev.agent.as_deref(),
                Some("grok"),
                "subtype={:?} id={}",
                ev.subtype,
                ev.id
            );
            assert_eq!(ev.data.session_id, SESSION);
        }

        // --- user prompt from updates.jsonl ---
        let prompts: Vec<_> = collected
            .iter()
            .filter(|e| is_subtype(e, "message.user.prompt"))
            .collect();
        assert_eq!(prompts.len(), 1, "exactly one user prompt from updates.jsonl");
        assert_eq!(grok_text(prompts[0]), "run a command");

        // --- terminal log → tool_result text equals log file bytes ---
        let term_results: Vec<_> = collected
            .iter()
            .filter(|e| is_subtype(e, "message.user.tool_result"))
            .collect();
        assert!(
            !term_results.is_empty(),
            "expected terminal tool_result from watcher, subtypes: {:?}",
            collected
                .iter()
                .map(|e| e.subtype.as_deref())
                .collect::<Vec<_>>()
        );
        let term = term_results
            .iter()
            .find(|e| grok_text(e) == log_body)
            .expect("tool_result text must equal terminal log body byte-for-byte");
        match term.data.agent_payload.as_ref() {
            Some(AgentPayload::Grok(g)) => {
                assert_eq!(g.tool_call_id.as_deref(), Some("call-shell-1"));
            }
            _ => panic!("expected Grok payload"),
        }

        // --- hunk → file.hunk with exact path ---
        let hunks: Vec<_> = collected
            .iter()
            .filter(|e| is_subtype(e, "file.hunk"))
            .collect();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            grok_text(hunks[0]),
            "/Users/maxglassie/projects/OpenStory/rs/core/src/translate_grok_l2.rs"
        );

        // --- summary meta ---
        let summaries: Vec<_> = collected
            .iter()
            .filter(|e| is_subtype(e, "system.grok.summary"))
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(grok_text(summaries[0]), "Grok L2 BDD session");
    }

    #[test]
    fn it_should_resolve_session_id_from_l2_artifact_paths() {
        let dir = tempfile::tempdir().unwrap();
        let sess = seed_session_tree(dir.path(), "x\n");
        assert_eq!(
            session_id_from_path(&sess.join("updates.jsonl")),
            SESSION
        );
        assert_eq!(
            session_id_from_path(&sess.join("hunk_records.jsonl")),
            SESSION
        );
        assert_eq!(
            session_id_from_path(&sess.join("terminal").join("call-shell-1.log")),
            SESSION
        );
        assert_eq!(session_id_from_path(&sess.join("summary.json")), SESSION);
    }
}

// ── describe("when L2 CloudEvents reach the views layer") ──

mod when_l2_events_reach_views {
    use super::*;

    #[test]
    fn it_should_render_hunk_as_file_snapshot_with_path_and_line_counts() {
        let hunk = translate_hunk_record(
            &json!({
                "hunkId": "h1",
                "filePath": "/tmp/edited.rs",
                "hunkStart": 2,
                "hunkEnd": 8,
                "linesAdded": 3,
                "linesRemoved": 1
            }),
            SESSION,
            1,
        )
        .unwrap();
        let records = from_cloud_event(&hunk);
        assert_eq!(records.len(), 1);
        match &records[0].body {
            RecordBody::FileSnapshot(fs) => {
                let tracked = fs.tracked_files.as_ref().expect("tracked_files");
                assert_eq!(
                    tracked.get("file_path").and_then(|v| v.as_str()),
                    Some("/tmp/edited.rs")
                );
                assert_eq!(
                    tracked.get("lines_added").and_then(|v| v.as_i64()),
                    Some(3)
                );
                assert_eq!(
                    tracked.get("lines_removed").and_then(|v| v.as_i64()),
                    Some(1)
                );
            }
            other => panic!("expected FileSnapshot, got {other:?}"),
        }
    }

    #[test]
    fn it_should_render_terminal_tool_result_with_full_output_text() {
        let body = "line1\nline2\n";
        let ev = translate_terminal_log(
            SESSION,
            "call-9",
            Path::new("/tmp/s/terminal/call-9.log"),
            body,
            1,
        );
        let records = from_cloud_event(&ev);
        assert_eq!(records.len(), 1);
        match &records[0].body {
            RecordBody::ToolResult(tr) => {
                assert_eq!(tr.call_id, "call-9");
                assert_eq!(tr.output.as_deref(), Some(body));
                assert!(!tr.is_error);
            }
            other => panic!("expected ToolResult, got {other:?}"),
        }
    }
}

// ── describe("when L2 event ids are derived") ──

mod when_l2_event_ids_are_derived {
    use super::*;

    #[test]
    fn it_should_keep_hunk_and_terminal_ids_distinct_even_if_keys_collide() {
        let hunk = translate_hunk_record(
            &json!({
                "hunkId": "same-key",
                "filePath": "/a.rs",
                "linesAdded": 1,
                "linesRemoved": 0
            }),
            "sess",
            1,
        )
        .unwrap();
        let term = translate_terminal_log(
            "sess",
            "same-key",
            Path::new("/tmp/terminal/same-key.log"),
            "out",
            2,
        );
        assert_ne!(
            hunk.id, term.id,
            "different L2 kinds must not share event ids"
        );
    }

    #[test]
    fn it_should_dedupe_summary_on_identical_content() {
        let v = json!({"session_summary": "x", "num_messages": 3});
        let a = translate_summary_json("sess", &v, 1);
        let b = translate_summary_json("sess", &v, 99);
        assert_eq!(a.id, b.id);
    }
}
