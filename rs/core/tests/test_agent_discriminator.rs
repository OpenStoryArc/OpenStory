//! Cowork sessions must be distinguishable from CLI Claude Code sessions.
//!
//! Cowork (Claude Desktop's code tab) runs Claude Code inside a sandbox VM
//! and writes byte-identical JSONL — the *format* is claude-code, but the
//! *source platform* is different (sandboxed VM, different MCP surface,
//! routes to a different model). The CloudEvent `agent` extension attribute
//! carries the platform identity; the payload's `meta.agent` keeps tagging
//! the format. Views fall through to claude-code parsing for any agent
//! value that isn't pi-mono/hermes, so the discriminator never changes how
//! records render — only how they're labeled.

use open_story_core::paths::agent_label_from_path;
use open_story_core::translate::{translate_line, TranscriptState};
use serde_json::json;
use std::path::PathBuf;

fn user_line(uuid: &str) -> serde_json::Value {
    json!({
        "type": "user",
        "uuid": uuid,
        "timestamp": "2026-06-10T00:00:00Z",
        "sessionId": "sess-cowork",
        "message": {"role": "user", "content": [{"type": "text", "text": "hi"}]}
    })
}

// ── when the transcript lives under local-agent-mode-sessions ───────

#[test]
fn it_should_label_cowork_paths() {
    let path = PathBuf::from(
        "/Users/me/Library/Application Support/Claude/local-agent-mode-sessions/\
         acct-uuid/task-uuid/local_sess-uuid/.claude/projects/encoded/inner.jsonl",
    );
    assert_eq!(agent_label_from_path(&path), Some("claude-code-cowork"));
}

#[test]
fn it_should_not_label_regular_claude_projects_paths() {
    let path = PathBuf::from("/Users/me/.claude/projects/-Users-me-repo/sess.jsonl");
    assert_eq!(agent_label_from_path(&path), None);
}

#[test]
fn it_should_not_label_paths_that_merely_contain_the_substring() {
    // A user directory *named like* the marker must not trigger — only an
    // exact path component counts.
    let path = PathBuf::from("/Users/me/my-local-agent-mode-sessions-notes/sess.jsonl");
    assert_eq!(agent_label_from_path(&path), None);
}

// ── translate stamps the label on every CloudEvent ──────────────────

#[test]
fn it_should_stamp_cowork_agent_when_state_carries_the_label() {
    let mut state = TranscriptState::new("sess-cowork".into())
        .with_agent_label("claude-code-cowork");
    let events = translate_line(&user_line("evt-cw-1"), &mut state);

    assert!(!events.is_empty(), "translator must emit at least one event");
    for ce in &events {
        assert_eq!(
            ce.agent.as_deref(),
            Some("claude-code-cowork"),
            "CloudEvent.agent must carry the platform discriminator"
        );
    }
}

#[test]
fn it_should_default_to_claude_code_without_a_label() {
    let mut state = TranscriptState::new("sess-cli".into());
    let events = translate_line(&user_line("evt-cli-1"), &mut state);

    assert!(!events.is_empty());
    for ce in &events {
        assert_eq!(ce.agent.as_deref(), Some("claude-code"));
    }
}

// ── format tag is unchanged: payload meta.agent stays claude-code ───

#[test]
fn it_should_keep_payload_format_tag_as_claude_code() {
    let mut state = TranscriptState::new("sess-cowork".into())
        .with_agent_label("claude-code-cowork");
    let events = translate_line(&user_line("evt-cw-2"), &mut state);

    for ce in &events {
        let ap = ce
            .data
            .agent_payload
            .as_ref()
            .expect("claude-code events carry a typed payload");
        assert_eq!(
            ap.agent(),
            "claude-code",
            "payload meta.agent tags the *format* and must not change — \
             views dispatch typed parsing on it"
        );
    }
}
