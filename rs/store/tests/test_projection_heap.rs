//! B-09 (a): `SessionProjection::heap_bytes` counts what it holds
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! The projection cache budgets by `heap_bytes`; if that number is a
//! fraction of the real heap, the budget bounds nothing. This binary
//! installs a counting allocator and measures, independently of the
//! projection's own arithmetic, how many bytes stay allocated after a
//! projection is built from a realistic session. `heap_bytes` must land
//! within 20 % of that.
//!
//! Run with: cargo test -p open-story-store --test test_projection_heap

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

use open_story_store::projection::SessionProjection;
use serde_json::{json, Value};

/// Counts live bytes: every allocation adds its requested size, every free
/// subtracts it. Independent of any estimate inside the crate.
struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        System.alloc(layout)
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        System.dealloc(ptr, layout)
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        System.realloc(ptr, layout, new_size)
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}

// ── A session shaped like the translator's claude-code output ──────────

const SESSION: &str = "heap-session-1";

fn envelope(seq: u64, subtype: &str, raw: Value, payload: Value) -> Value {
    let mut ap = json!({"_variant": "claude-code", "meta": {"agent": "claude-code"},
        "cwd": "/home/bm/projects/proj-1", "git_branch": "main", "is_sidechain": false,
        "uuid": format!("00000000-0000-4000-8000-{seq:012}"), "version": "2.1.0",
        "timestamp": format!("2026-09-01T10:{:02}:{:02}.000Z", seq / 60, seq % 60)});
    for (k, v) in payload.as_object().cloned().unwrap_or_default() {
        ap[k] = v;
    }
    let mut raw_full = json!({"cwd": "/home/bm/projects/proj-1", "gitBranch": "main",
        "isSidechain": false, "sessionId": SESSION, "type": "user",
        "uuid": format!("00000000-0000-4000-8000-{seq:012}"), "version": "2.1.0",
        "parentUuid": if seq == 1 { Value::Null } else { json!(format!("00000000-0000-4000-8000-{:012}", seq - 1)) }});
    for (k, v) in raw.as_object().cloned().unwrap_or_default() {
        raw_full[k] = v;
    }
    json!({
        "specversion": "1.0",
        "id": format!("evt-{SESSION}-{seq}"),
        "source": format!("arc://transcript/{SESSION}"),
        "type": "io.arc.event",
        "subtype": subtype,
        "time": format!("2026-09-01T10:{:02}:{:02}.000Z", seq / 60, seq % 60),
        "datacontenttype": "application/json",
        "agent": "claude-code",
        "data": {"seq": seq, "session_id": SESSION, "raw": raw_full, "agent_payload": ap},
    })
}

fn usage() -> Value {
    json!({"input_tokens": 12, "output_tokens": 340, "cache_creation_input_tokens": 900,
           "cache_read_input_tokens": 48000, "service_tier": "standard"})
}

fn assistant(seq: u64, subtype: &str, content: Value, payload: Value) -> Value {
    let mut p = json!({"message_id": format!("msg_{seq:024}"), "model": "claude-synthetic-1",
        "stop_reason": "end_turn", "token_usage": usage(),
        "parent_uuid": format!("00000000-0000-4000-8000-{:012}", seq - 1)});
    for (k, v) in payload.as_object().cloned().unwrap_or_default() {
        p[k] = v;
    }
    envelope(
        seq,
        subtype,
        json!({"type": "assistant", "requestId": format!("req_{seq:024}"),
            "message": {"id": format!("msg_{seq:024}"), "type": "message", "role": "assistant",
                "model": "claude-synthetic-1", "content": content, "stop_reason": "end_turn",
                "usage": usage()}}),
        p,
    )
}

fn words(seed: u64, n: usize) -> String {
    let syl = [
        "ka", "to", "mi", "re", "sol", "ven", "dor", "lin", "ash", "quo",
    ];
    (0..n)
        .map(|i| syl[((seed as usize) * 7 + i * 3) % syl.len()])
        .collect::<Vec<_>>()
        .join(" ")
}

fn output_text(seed: u64, bytes: usize) -> String {
    let mut s = String::new();
    let mut i = 0;
    while s.len() < bytes {
        s.push_str(&format!(
            "{i:6}  src/module_{}.rs:{}: {}\n",
            i % 97,
            (seed + i as u64) % 900,
            words(seed + i as u64, 6)
        ));
        i += 1;
    }
    s.truncate(bytes);
    s
}

/// 20 turns of prompt / thinking / text / tool_use / tool_result / text /
/// turn.complete — 140 events, ~5 KB tool outputs, like the harness fixture.
fn session_events() -> Vec<Value> {
    let mut out = Vec::new();
    let mut seq = 1u64;
    for turn in 0..20u64 {
        let text = words(turn, 14);
        out.push(envelope(
            seq,
            "message.user.prompt",
            json!({"message": {"role": "user", "content": text}, "promptId": format!("p-{turn}")}),
            json!({"text": text, "user_type": "external"}),
        ));
        seq += 1;
        out.push(assistant(
            seq,
            "message.assistant.thinking",
            json!([{"type": "thinking", "thinking": words(seq, 40), "signature": "s".repeat(200)}]),
            json!({"content_types": ["thinking"]}),
        ));
        seq += 1;
        out.push(assistant(
            seq,
            "message.assistant.text",
            json!([{"type": "text", "text": words(seq, 60)}]),
            json!({"content_types": ["text"]}),
        ));
        seq += 1;
        let cmd = format!("cargo test -p crate-{turn} -- {}", words(seq, 3));
        out.push(assistant(
            seq,
            "message.assistant.tool_use",
            json!([{"type": "tool_use", "id": format!("toolu_{seq:024}"), "name": "Bash",
                    "input": {"command": cmd, "description": words(seq, 5)}}]),
            json!({"content_types": ["tool_use"], "tool": "Bash",
                   "args": {"command": cmd, "description": words(seq, 5)}}),
        ));
        seq += 1;
        let outp = output_text(seq, 5000);
        out.push(envelope(seq, "message.user.tool_result",
            json!({"message": {"role": "user", "content": [{"type": "tool_result",
                   "tool_use_id": format!("toolu_{:024}", seq - 1), "content": outp, "is_error": false}]},
                   "toolUseResult": {"stdout": outp, "stderr": "", "interrupted": false, "isImage": false}}),
            json!({"text": outp, "parent_uuid": format!("00000000-0000-4000-8000-{:012}", seq - 1),
                   "tool_outcome": {"type": "bash", "command": "cargo test", "succeeded": true},
                   "user_type": "external"})));
        seq += 1;
        out.push(assistant(
            seq,
            "message.assistant.text",
            json!([{"type": "text", "text": words(seq, 60)}]),
            json!({"content_types": ["text"]}),
        ));
        seq += 1;
        out.push(envelope(
            seq,
            "system.turn.complete",
            json!({"subtype": "turn_complete"}),
            json!({}),
        ));
        seq += 1;
    }
    out
}

mod when_a_projection_is_built_from_a_real_shaped_session {
    use super::*;

    #[test]
    fn it_reports_heap_bytes_within_twenty_percent_of_the_allocator() {
        let events = session_events();
        assert_eq!(events.len(), 140);

        // Warm every code path once so lazily-initialised statics (serde,
        // regexes, thread-locals) are not counted against the projection.
        {
            let mut warm = SessionProjection::new("warm");
            for e in &events {
                warm.append(e);
            }
            assert!(warm.event_count() == 140);
        }

        let before = live();
        let mut p = SessionProjection::new(SESSION);
        for e in &events {
            let _ = p.append(e);
        }
        let measured = live() - before;
        let reported = p.heap_bytes();
        assert_eq!(p.event_count(), 140);
        assert!(
            measured > 100_000,
            "a 140-event session holds more than 100 KB: {measured}"
        );

        let ratio = reported as f64 / measured as f64;
        assert!(
            (0.8..=1.2).contains(&ratio),
            "heap_bytes reports {reported} B, the allocator measured {measured} B live \
             (ratio {ratio:.2}); it must be within 20 %"
        );
        drop(p);
    }
}
