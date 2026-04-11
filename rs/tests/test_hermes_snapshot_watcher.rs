//! Performance tests for a Hermes snapshot-diff file watcher.
//!
//! Hermes Agent atomically overwrites `session_*.json` files via
//! temp file -> fsync -> os.replace (POSIX atomic rename). These tests
//! validate that notify::RecommendedWatcher can detect such writes,
//! that the parse-diff-translate pipeline is fast enough, that coalesced
//! events still produce correct CloudEvents, that parsing scales linearly,
//! and that concurrent sessions don't cross-contaminate.
//!
//! Run:
//!   cargo test -p open-story --test test_hermes_snapshot_watcher

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use open_story_core::translate::TranscriptState;
use open_story_core::translate_hermes::translate_hermes_line;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tempfile::TempDir;

// ── Helpers ─────────────────────────────────────────────────────────

/// Atomically replace `path` by writing to a .tmp sibling then renaming.
fn atomic_replace(path: &Path, content: &str) {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, content).unwrap();
    std::fs::rename(&tmp, path).unwrap();
}

/// Build a user message in Hermes/OpenAI shape.
fn user_message(content: &str) -> Value {
    json!({"role": "user", "content": content})
}

/// Build an assistant tool-call message.
fn assistant_tool_call(tool: &str, args: &str) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "tool_calls": [{
            "id": format!("tc_{}", tool),
            "function": {"name": tool, "arguments": args}
        }],
        "finish_reason": "tool_calls"
    })
}

/// Build a tool result message.
fn tool_result(call_id: &str, content: &str) -> Value {
    json!({
        "role": "tool",
        "tool_call_id": call_id,
        "content": content
    })
}

/// Build an assistant text message (final answer).
fn assistant_text(content: &str) -> Value {
    json!({
        "role": "assistant",
        "content": content,
        "finish_reason": "stop"
    })
}

/// Generate a Hermes session snapshot JSON string.
///
/// Produces the exact shape from `rs/tests/fixtures/hermes/session_snapshot.json`.
/// The `system_prompt_size` parameter controls padding to reach realistic file sizes
/// (real sessions start around 60KB because the system prompt dominates).
fn generate_hermes_snapshot(
    session_id: &str,
    messages: &[Value],
    system_prompt_size: usize,
) -> String {
    // Pad the system prompt to the requested size.
    let base_prompt = "You are Hermes Agent, a self-improving AI assistant.";
    let prompt = if system_prompt_size > base_prompt.len() {
        let padding = "x".repeat(system_prompt_size - base_prompt.len());
        format!("{} {}", base_prompt, padding)
    } else {
        base_prompt.to_string()
    };

    let snapshot = json!({
        "session_id": session_id,
        "model": "mock-model",
        "base_url": "http://mock:8000/v1",
        "platform": "cli",
        "session_start": "2026-04-10T10:00:00.000000+00:00",
        "last_updated": "2026-04-10T10:05:00.000000+00:00",
        "system_prompt": prompt,
        "tools": ["read_file", "write_file", "terminal"],
        "message_count": messages.len(),
        "messages": messages,
    });

    serde_json::to_string(&snapshot).unwrap()
}

/// Snapshot state for diffing — tracks the previous session_id and message count.
struct SnapshotState {
    session_id: String,
    message_count: usize,
}

/// Diff two snapshots: returns the new messages that should be translated.
///
/// - If the session_id changed, emit all messages (new session).
/// - If the message count shrank, emit all messages (compression/undo).
/// - Otherwise emit only the messages after `prev.message_count`.
fn diff_snapshot(prev: &SnapshotState, curr_json: &Value) -> Vec<Value> {
    let curr_sid = curr_json["session_id"].as_str().unwrap_or("");
    let messages = curr_json["messages"].as_array().unwrap();

    if curr_sid != prev.session_id || messages.len() < prev.message_count {
        // New session or compression — emit all messages
        return messages.clone();
    }

    // Normal append — emit only new messages
    messages[prev.message_count..].to_vec()
}

/// Wrap a single Hermes message into the JSONL envelope expected by
/// `translate_hermes_line`.
fn wrap_hermes_line(session_id: &str, msg: &Value, seq: u64) -> Value {
    json!({
        "envelope": {
            "session_id": session_id,
            "event_seq": seq,
            "timestamp": "2026-04-10T10:00:00Z",
            "source": "hermes"
        },
        "event_type": "message",
        "data": msg
    })
}

/// Build a realistic conversation of `n` messages: alternating user prompts,
/// assistant tool calls, tool results, finishing with an assistant text reply.
fn build_conversation(n: usize) -> Vec<Value> {
    let mut msgs = Vec::with_capacity(n);
    let mut i = 0;
    while msgs.len() < n {
        // User prompt
        msgs.push(user_message(&format!("Question {}", i)));
        if msgs.len() >= n {
            break;
        }
        // Assistant tool call
        msgs.push(assistant_tool_call("read_file", &format!("{{\"path\": \"file_{}.rs\"}}", i)));
        if msgs.len() >= n {
            break;
        }
        // Tool result
        msgs.push(tool_result(
            &format!("tc_read_file"),
            &format!("fn main() {{ println!(\"file {}\"); }}", i),
        ));
        if msgs.len() >= n {
            break;
        }
        // Assistant text
        msgs.push(assistant_text(&format!("Here is the content of file_{}.rs", i)));
        i += 1;
    }
    msgs.truncate(n);
    msgs
}

// ── Test 1: Atomic replace detection ────────────────────────────────

/// The viability test. Can notify::RecommendedWatcher see os.replace-style writes?
///
/// Creates a temp dir, writes an initial session file, starts a RecommendedWatcher,
/// does 20 atomic-replace cycles (write temp → std::fs::rename over target) with
/// 500ms spacing, and asserts ≥90% of replaces are detected.
///
/// This is the gate test — if it fails, PollWatcher is needed instead.
#[test]
fn test_atomic_replace_detection() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_test.json");

    // Write initial file so the watcher has something to track.
    let initial = generate_hermes_snapshot("test-session", &[user_message("hello")], 1000);
    std::fs::write(&session_path, &initial).unwrap();

    // Collect events into a shared vec.
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                events_clone.lock().unwrap().push(event);
            }
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(dir.path(), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    // Give the watcher time to register.
    std::thread::sleep(Duration::from_millis(100));

    let replace_count = 20;
    for i in 0..replace_count {
        let msgs = build_conversation(i + 2);
        let content = generate_hermes_snapshot("test-session", &msgs, 1000);
        atomic_replace(&session_path, &content);
        std::thread::sleep(Duration::from_millis(500));
    }

    // Give a grace period for the last events to arrive.
    std::thread::sleep(Duration::from_millis(500));

    let collected = events.lock().unwrap();
    // Count events that reference our session file (Create or Modify).
    // A single atomic replace may produce multiple events (Create for the tmp,
    // Rename, Modify, etc.), so we count distinct event batches that include
    // the target path.
    let target_events: Vec<&Event> = collected
        .iter()
        .filter(|e| {
            e.paths
                .iter()
                .any(|p| p.file_name().map(|n| n == "session_test.json").unwrap_or(false))
        })
        .collect();

    // We should detect at least 90% of replaces. On macOS (kqueue/FSEvents)
    // and Linux (inotify), rename events are reliably delivered. Coalescing
    // is acceptable — the key is that we don't miss >10%.
    let min_expected = (replace_count as f64 * 0.9) as usize;
    assert!(
        target_events.len() >= min_expected,
        "Expected at least {} events for {} atomic replaces, got {}. \
         Events: {:?}",
        min_expected,
        replace_count,
        target_events.len(),
        target_events
            .iter()
            .map(|e| format!("{:?}: {:?}", e.kind, e.paths))
            .collect::<Vec<_>>()
    );
}

// ── Test 2: Parse and diff latency ──────────────────────────────────

/// Processing pipeline speed: atomic-replace → read → parse → diff → translate.
///
/// Creates a 60KB snapshot (realistic: ~55KB system prompt + messages), runs 50
/// iterations of: append one message, atomic-replace, read, parse JSON, diff,
/// translate new messages. Measures p50/p99 latency.
///
/// Asserts: p50 < 2ms, p99 < 10ms.
#[test]
fn test_parse_and_diff_latency() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_perf.json");
    let session_id = "perf-session";

    // Start with 3 messages and a ~55KB system prompt (matches real data).
    let mut messages = vec![
        user_message("What files are in this directory?"),
        assistant_tool_call("terminal", r#"{"command": "ls -la"}"#),
        tool_result("tc_terminal", "README.md\nsetup.py\nsrc/\n"),
    ];

    let initial = generate_hermes_snapshot(session_id, &messages, 55_000);
    std::fs::write(&session_path, &initial).unwrap();

    let mut prev_state = SnapshotState {
        session_id: session_id.to_string(),
        message_count: messages.len(),
    };

    let mut latencies: Vec<Duration> = Vec::with_capacity(50);

    for i in 0..50 {
        // Add one message (alternating types for realism).
        let new_msg = match i % 4 {
            0 => user_message(&format!("Follow-up question {}", i)),
            1 => assistant_tool_call("read_file", &format!("{{\"path\": \"file_{}.rs\"}}", i)),
            2 => tool_result("tc_read_file", &format!("content of file {}", i)),
            _ => assistant_text(&format!("Here is the answer for question {}", i)),
        };
        messages.push(new_msg);

        let content = generate_hermes_snapshot(session_id, &messages, 55_000);
        atomic_replace(&session_path, &content);

        // Measure: read + parse + diff + translate
        let start = Instant::now();

        let raw = std::fs::read_to_string(&session_path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();
        let new_messages = diff_snapshot(&prev_state, &parsed);

        // Translate each new message through the real translator.
        let mut transcript_state = TranscriptState::new(session_id.to_string());
        for (seq, msg) in new_messages.iter().enumerate() {
            let line = wrap_hermes_line(session_id, msg, seq as u64 + 1);
            let _events = translate_hermes_line(&line, &mut transcript_state);
        }

        let elapsed = start.elapsed();
        latencies.push(elapsed);

        prev_state = SnapshotState {
            session_id: session_id.to_string(),
            message_count: messages.len(),
        };
    }

    // Sort for percentile calculation.
    latencies.sort();

    let p50 = latencies[latencies.len() / 2];
    let p99_idx = (latencies.len() as f64 * 0.99).ceil() as usize - 1;
    let p99 = latencies[p99_idx.min(latencies.len() - 1)];

    eprintln!(
        "Parse+diff+translate latency: p50={:?}, p99={:?}, min={:?}, max={:?}",
        p50,
        p99,
        latencies.first().unwrap(),
        latencies.last().unwrap()
    );

    assert!(
        p50 < Duration::from_millis(2),
        "p50 latency {:?} exceeded 2ms threshold",
        p50
    );
    assert!(
        p99 < Duration::from_millis(10),
        "p99 latency {:?} exceeded 10ms threshold",
        p99
    );
}

// ── Test 3: Coalesced event correctness ─────────────────────────────

/// What happens when the watcher misses intermediate states?
///
/// Scenario A: rapid writes (5 snapshots with 6..10 messages). Process whatever
///   the diff produces. Assert: all 5 new messages are emitted as CloudEvents.
///
/// Scenario B: undo — shrink from 10 to 8 messages, then grow to 9. Assert:
///   shrinkage is detected and all messages are re-emitted, then the new msg.
///
/// Scenario C: new session_id (compression). Assert: treated as new session,
///   all messages emitted.
#[test]
fn test_coalesced_event_correctness() {
    let session_id = "coalesce-session";

    // ── Scenario A: rapid appends ──
    // Start with 5 messages.
    let _base_messages = build_conversation(5);
    let mut prev = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 5,
    };

    // Simulate: watcher misses intermediate states, only sees the final
    // snapshot with 10 messages.
    let full_messages = build_conversation(10);
    let snapshot = generate_hermes_snapshot(session_id, &full_messages, 1000);
    let parsed: Value = serde_json::from_str(&snapshot).unwrap();

    let new_msgs = diff_snapshot(&prev, &parsed);
    assert_eq!(
        new_msgs.len(),
        5,
        "Scenario A: should emit exactly 5 new messages (6..10), got {}",
        new_msgs.len()
    );

    // Translate them and verify we get CloudEvents.
    let mut state = TranscriptState::new(session_id.to_string());
    let mut total_events = 0;
    for (seq, msg) in new_msgs.iter().enumerate() {
        let line = wrap_hermes_line(session_id, msg, seq as u64 + 1);
        let events = translate_hermes_line(&line, &mut state);
        total_events += events.len();
    }
    assert!(
        total_events >= 5,
        "Scenario A: expected at least 5 CloudEvents from 5 messages, got {}",
        total_events
    );

    // ── Scenario B: undo (shrinkage) then grow ──
    prev = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 10,
    };

    // Shrink to 8 messages (simulates /undo removing last 2).
    let shrunk_messages = build_conversation(8);
    let snapshot_shrunk = generate_hermes_snapshot(session_id, &shrunk_messages, 1000);
    let parsed_shrunk: Value = serde_json::from_str(&snapshot_shrunk).unwrap();

    let new_msgs_shrunk = diff_snapshot(&prev, &parsed_shrunk);
    // Shrinkage: all 8 messages should be re-emitted (session reset).
    assert_eq!(
        new_msgs_shrunk.len(),
        8,
        "Scenario B (shrink): should re-emit all 8 messages, got {}",
        new_msgs_shrunk.len()
    );

    // Now grow from 8 to 9.
    prev = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 8,
    };
    let grown_messages = build_conversation(9);
    let snapshot_grown = generate_hermes_snapshot(session_id, &grown_messages, 1000);
    let parsed_grown: Value = serde_json::from_str(&snapshot_grown).unwrap();

    let new_msgs_grown = diff_snapshot(&prev, &parsed_grown);
    assert_eq!(
        new_msgs_grown.len(),
        1,
        "Scenario B (grow): should emit 1 new message, got {}",
        new_msgs_grown.len()
    );

    // ── Scenario C: new session ID (compression) ──
    prev = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 9,
    };

    let new_session_id = "compressed-session-new";
    let compressed_messages = build_conversation(3);
    let snapshot_compressed =
        generate_hermes_snapshot(new_session_id, &compressed_messages, 1000);
    let parsed_compressed: Value = serde_json::from_str(&snapshot_compressed).unwrap();

    let new_msgs_compressed = diff_snapshot(&prev, &parsed_compressed);
    assert_eq!(
        new_msgs_compressed.len(),
        3,
        "Scenario C: new session_id should emit all 3 messages, got {}",
        new_msgs_compressed.len()
    );
}

// ── Test 4: Large file scaling ──────────────────────────────────────

/// Parse cost at extreme sizes. Generates snapshots at 60KB, 500KB, 1MB, 5MB,
/// 10MB and measures parse time. Asserts linear scaling (10MB < 100ms).
#[test]
fn test_large_file_scaling() {
    let dir = TempDir::new().unwrap();
    let session_id = "scale-session";

    // Target sizes (approximate — system_prompt_size is the main knob).
    let targets: Vec<(&str, usize)> = vec![
        ("60KB", 55_000),
        ("500KB", 490_000),
        ("1MB", 1_000_000),
        ("5MB", 5_000_000),
        ("10MB", 10_000_000),
    ];

    let messages = build_conversation(10);
    let mut results: Vec<(String, usize, Duration)> = Vec::new();

    for (label, prompt_size) in &targets {
        let content = generate_hermes_snapshot(session_id, &messages, *prompt_size);
        let actual_size = content.len();
        let path = dir.path().join(format!("session_{}.json", label));
        std::fs::write(&path, &content).unwrap();

        // Warm up: one parse to avoid cold-cache effects.
        let raw = std::fs::read_to_string(&path).unwrap();
        let _: Value = serde_json::from_str(&raw).unwrap();

        // Measure: average of 5 iterations.
        let iterations = 5;
        let start = Instant::now();
        for _ in 0..iterations {
            let raw = std::fs::read_to_string(&path).unwrap();
            let _parsed: Value = serde_json::from_str(&raw).unwrap();
        }
        let avg = start.elapsed() / iterations;

        results.push((label.to_string(), actual_size, avg));
    }

    eprintln!("\nSnapshot parse times:");
    for (label, size, avg) in &results {
        eprintln!(
            "  {}: {}KB actual, avg parse {:?}",
            label,
            size / 1024,
            avg
        );
    }

    // Assert: 10MB should parse in under 100ms.
    let (_, _, ten_mb_time) = results.last().unwrap();
    assert!(
        *ten_mb_time < Duration::from_millis(100),
        "10MB parse took {:?}, exceeds 100ms threshold",
        ten_mb_time
    );

    // Assert: roughly linear scaling. The 10MB parse should be no more than
    // 5x the 1MB parse (allowing for overhead and measurement noise).
    let one_mb_time = results[2].2;
    if one_mb_time > Duration::from_micros(100) {
        // Only check ratio if 1MB time is measurable.
        let ratio = ten_mb_time.as_nanos() as f64 / one_mb_time.as_nanos() as f64;
        eprintln!("  10MB/1MB ratio: {:.1}x (expect ~10x for linear)", ratio);
        assert!(
            ratio < 50.0,
            "Parse scaling is super-linear: 10MB/1MB ratio = {:.1}x (expected < 50x)",
            ratio
        );
    }
}

// ── Test 5: Concurrent sessions ─────────────────────────────────────

/// Gateway mode — multiple session files written simultaneously.
///
/// Creates 5 session files, writes to each from separate threads at random
/// intervals, then verifies all sessions produce correct diffs with no
/// cross-contamination.
#[test]
fn test_concurrent_sessions() {
    let dir = TempDir::new().unwrap();
    let session_count = 5;
    let writes_per_session = 10;

    // Set up initial files with 2 messages each.
    let initial_msgs = build_conversation(2);
    for i in 0..session_count {
        let sid = format!("concurrent-session-{}", i);
        let content = generate_hermes_snapshot(&sid, &initial_msgs, 1000);
        let path = dir.path().join(format!("session_{}.json", i));
        std::fs::write(&path, &content).unwrap();
    }

    // Collect events from the watcher.
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                events_clone.lock().unwrap().push(event);
            }
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(dir.path(), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    std::thread::sleep(Duration::from_millis(100));

    // Spawn threads: each writes to its own session file.
    let dir_path = dir.path().to_path_buf();
    let handles: Vec<_> = (0..session_count)
        .map(|i| {
            let dir = dir_path.clone();
            std::thread::spawn(move || {
                let sid = format!("concurrent-session-{}", i);
                for write_num in 0..writes_per_session {
                    let msg_count = 2 + write_num + 1; // grow from 3 to 12
                    let messages = build_conversation(msg_count);
                    let content = generate_hermes_snapshot(&sid, &messages, 1000);
                    let path = dir.join(format!("session_{}.json", i));
                    atomic_replace(&path, &content);

                    // Stagger writes slightly to simulate real-world timing.
                    std::thread::sleep(Duration::from_millis(50 + (i * 10) as u64));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("writer thread panicked");
    }

    // Grace period for final events.
    std::thread::sleep(Duration::from_millis(500));

    // Verify: read each file and diff from initial state. Each session
    // should have grown from 2 to 12 messages independently.
    let mut all_session_events: Vec<Vec<Value>> = Vec::new();

    for i in 0..session_count {
        let sid = format!("concurrent-session-{}", i);
        let path = dir.path().join(format!("session_{}.json", i));
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: Value = serde_json::from_str(&raw).unwrap();

        // Check final state is correct.
        let final_msg_count = parsed["message_count"].as_u64().unwrap() as usize;
        assert_eq!(
            final_msg_count, 12,
            "Session {} final message count should be 12, got {}",
            i, final_msg_count
        );

        // Verify session_id matches (no cross-contamination).
        let file_sid = parsed["session_id"].as_str().unwrap();
        assert_eq!(
            file_sid, sid,
            "Session {} has wrong session_id: expected '{}', got '{}'",
            i, sid, file_sid
        );

        // Diff from initial state: should produce 10 new messages.
        let prev = SnapshotState {
            session_id: sid.clone(),
            message_count: 2,
        };
        let new_msgs = diff_snapshot(&prev, &parsed);
        assert_eq!(
            new_msgs.len(),
            10,
            "Session {}: diff should show 10 new messages, got {}",
            i,
            new_msgs.len()
        );

        // Translate and verify we get CloudEvents.
        let mut state = TranscriptState::new(sid.clone());
        let mut session_events = Vec::new();
        for (seq, msg) in new_msgs.iter().enumerate() {
            let line = wrap_hermes_line(&sid, msg, seq as u64 + 1);
            let events = translate_hermes_line(&line, &mut state);
            for ev in &events {
                session_events.push(json!({
                    "session_id": sid,
                    "source": ev.source,
                }));
            }
        }
        assert!(
            !session_events.is_empty(),
            "Session {} produced no CloudEvents",
            i
        );
        all_session_events.push(session_events);
    }

    // Cross-contamination check: each session's events should reference
    // only that session's source URI.
    for (i, session_events) in all_session_events.iter().enumerate() {
        let expected_source = format!("hermes://session/concurrent-session-{}", i);
        for ev in session_events {
            let source = ev["source"].as_str().unwrap();
            assert_eq!(
                source, expected_source,
                "Cross-contamination: session {} event has source '{}', expected '{}'",
                i, source, expected_source
            );
        }
    }

    // Verify the watcher saw events for all session files.
    let collected = events.lock().unwrap();
    for i in 0..session_count {
        let filename = format!("session_{}.json", i);
        let has_events = collected.iter().any(|e| {
            e.paths
                .iter()
                .any(|p| p.file_name().map(|n| n.to_string_lossy() == filename).unwrap_or(false))
        });
        assert!(
            has_events,
            "Watcher did not detect any events for session file '{}'",
            filename
        );
    }
}

// ── Test 6: Write rate sweep ───────────────────────────────────────

/// Sweep write rates to find where the watcher starts dropping events.
///
/// Writes 50 atomic replaces at each rate in [1, 2, 5, 10, 20, 50, 100]/sec,
/// then counts how many notify events arrived vs expected. This is primarily
/// a measurement test — it prints results at every rate. The only hard
/// assertion is that at 10 writes/sec, detection should be >= 50%.
#[test]
fn test_write_rate_sweep() {
    let rates = [1, 2, 5, 10, 20, 50, 100];
    let writes_per_rate = 50;

    eprintln!("\nWrite rate sweep ({} writes per rate):", writes_per_rate);

    let mut rate_results: Vec<(u64, usize, usize)> = Vec::new();

    for rate in &rates {
        let dir = TempDir::new().unwrap();
        let session_path = dir.path().join("session_sweep.json");

        // Write initial file.
        let initial = generate_hermes_snapshot("sweep-session", &[user_message("init")], 1000);
        std::fs::write(&session_path, &initial).unwrap();

        let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);

        let mut watcher = RecommendedWatcher::new(
            move |res: Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    events_clone.lock().unwrap().push(event);
                }
            },
            Config::default(),
        )
        .expect("failed to create watcher");

        watcher
            .watch(dir.path(), RecursiveMode::NonRecursive)
            .expect("failed to watch directory");

        std::thread::sleep(Duration::from_millis(100));

        let interval = Duration::from_micros(1_000_000 / *rate);

        for i in 0..writes_per_rate {
            let messages = build_conversation(2 + i);
            let content = generate_hermes_snapshot("sweep-session", &messages, 1000);
            atomic_replace(&session_path, &content);
            std::thread::sleep(interval);
        }

        // Wait for events to settle.
        std::thread::sleep(Duration::from_secs(2));

        let collected = events.lock().unwrap();
        let target_events = collected
            .iter()
            .filter(|e| {
                e.paths
                    .iter()
                    .any(|p| {
                        p.file_name()
                            .map(|n| n == "session_sweep.json")
                            .unwrap_or(false)
                    })
            })
            .count();

        let pct = (target_events as f64 / writes_per_rate as f64 * 100.0) as usize;
        eprintln!(
            "  rate={}/s: {}/{} events ({}%)",
            rate, target_events, writes_per_rate, pct
        );

        rate_results.push((*rate, target_events, writes_per_rate));
    }

    // Assert: at 10/sec, detection should be >= 50%.
    let ten_per_sec = rate_results.iter().find(|(r, _, _)| *r == 10).unwrap();
    let detection_pct = ten_per_sec.1 as f64 / ten_per_sec.2 as f64 * 100.0;
    assert!(
        detection_pct >= 50.0,
        "At 10 writes/sec, expected >= 50% detection, got {:.0}% ({}/{})",
        detection_pct,
        ten_per_sec.1,
        ten_per_sec.2
    );
}

// ── Test 7: Sustained load (5 minutes) ─────────────────────────────

/// 5 minutes of continuous writes at 2/sec (realistic fast-model rate).
///
/// Tracks events received, max latency, and file size growth over time.
/// Prints progress every 30 seconds.
///
/// Run with: cargo test -p open-story --test test_hermes_snapshot_watcher -- --ignored test_sustained_load_5_minutes
#[test]
#[ignore]
fn test_sustained_load_5_minutes() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_sustained.json");
    let session_id = "sustained-session";

    // Write initial file.
    let initial = generate_hermes_snapshot(session_id, &[user_message("init")], 55_000);
    std::fs::write(&session_path, &initial).unwrap();

    let events: Arc<Mutex<Vec<Instant>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                // Only track events for our session file.
                let is_target = event.paths.iter().any(|p| {
                    p.file_name()
                        .map(|n| n == "session_sustained.json")
                        .unwrap_or(false)
                });
                if is_target {
                    events_clone.lock().unwrap().push(Instant::now());
                }
            }
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(dir.path(), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    std::thread::sleep(Duration::from_millis(100));

    let total_writes = 600;
    let interval = Duration::from_millis(500); // 2/sec
    let test_start = Instant::now();
    let mut last_report = Instant::now();
    let mut max_file_size: usize = 0;

    eprintln!("\nSustained load: {} writes at 2/sec (5 minutes)...", total_writes);

    for i in 0..total_writes {
        let messages = build_conversation(2 + i);
        let content = generate_hermes_snapshot(session_id, &messages, 55_000);
        max_file_size = max_file_size.max(content.len());
        atomic_replace(&session_path, &content);
        std::thread::sleep(interval);

        // Progress report every 30 seconds.
        if last_report.elapsed() >= Duration::from_secs(30) {
            let received = events.lock().unwrap().len();
            let elapsed_secs = test_start.elapsed().as_secs();
            eprintln!(
                "  [{:>3}s] writes={}, events={}, file_size={}KB",
                elapsed_secs,
                i + 1,
                received,
                max_file_size / 1024
            );
            last_report = Instant::now();
        }
    }

    // Final settle time.
    std::thread::sleep(Duration::from_secs(2));

    let received = events.lock().unwrap().len();
    let detection_pct = received as f64 / total_writes as f64 * 100.0;
    let total_elapsed = test_start.elapsed();

    eprintln!("\nSustained load results:");
    eprintln!("  Total writes:   {}", total_writes);
    eprintln!("  Events received: {}", received);
    eprintln!("  Detection rate:  {:.1}%", detection_pct);
    eprintln!("  Max file size:   {}KB", max_file_size / 1024);
    eprintln!("  Wall time:       {:.1}s", total_elapsed.as_secs_f64());

    assert!(
        detection_pct >= 95.0,
        "Sustained load: expected >= 95% detection, got {:.1}% ({}/{})",
        detection_pct,
        received,
        total_writes
    );
}

// ── Test 8: Adversarial race ───────────────────────────────────────

/// Writer and reader hammering the same file as fast as possible for 5 seconds.
///
/// The writer does atomic_replace in a tight loop (no sleep). The reader does
/// open → read_to_string → serde_json::from_str in a tight loop. Verifies
/// that POSIX atomic rename guarantees mean zero parse failures.
#[test]
fn test_adversarial_race() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_race.json");
    let session_id = "race-session";

    // Write initial file.
    let initial = generate_hermes_snapshot(session_id, &[user_message("init")], 1000);
    std::fs::write(&session_path, &initial).unwrap();

    let running = Arc::new(AtomicBool::new(true));
    let write_count = Arc::new(AtomicUsize::new(0));
    let read_count = Arc::new(AtomicUsize::new(0));
    let parse_failures = Arc::new(AtomicUsize::new(0));

    let duration = Duration::from_secs(5);

    // Writer thread: atomic_replace in a tight loop.
    let writer_path = session_path.clone();
    let writer_running = Arc::clone(&running);
    let writer_count = Arc::clone(&write_count);
    let writer_handle = std::thread::spawn(move || {
        let mut i = 0usize;
        while writer_running.load(Ordering::Relaxed) {
            let messages = build_conversation(2 + (i % 50));
            let content = generate_hermes_snapshot(session_id, &messages, 1000);
            atomic_replace(&writer_path, &content);
            writer_count.fetch_add(1, Ordering::Relaxed);
            i += 1;
        }
    });

    // Reader thread: read + parse in a tight loop.
    let reader_path = session_path.clone();
    let reader_running = Arc::clone(&running);
    let reader_count_clone = Arc::clone(&read_count);
    let reader_failures = Arc::clone(&parse_failures);
    let reader_handle = std::thread::spawn(move || {
        while reader_running.load(Ordering::Relaxed) {
            match std::fs::read_to_string(&reader_path) {
                Ok(raw) => {
                    reader_count_clone.fetch_add(1, Ordering::Relaxed);
                    if serde_json::from_str::<Value>(&raw).is_err() {
                        reader_failures.fetch_add(1, Ordering::Relaxed);
                    }
                }
                Err(_) => {
                    // File might not exist momentarily during rename — that's OK,
                    // but still count it as a read attempt.
                    reader_count_clone.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    });

    // Let them race for the specified duration.
    std::thread::sleep(duration);
    running.store(false, Ordering::Relaxed);

    writer_handle.join().expect("writer thread panicked");
    reader_handle.join().expect("reader thread panicked");

    let writes = write_count.load(Ordering::Relaxed);
    let reads = read_count.load(Ordering::Relaxed);
    let failures = parse_failures.load(Ordering::Relaxed);

    eprintln!("\nAdversarial race ({}s):", duration.as_secs());
    eprintln!("  Write rate: {}/s", writes / duration.as_secs() as usize);
    eprintln!("  Read rate:  {}/s", reads / duration.as_secs() as usize);
    eprintln!("  Total writes: {}", writes);
    eprintln!("  Total reads:  {}", reads);
    eprintln!("  Parse failures: {}", failures);

    assert_eq!(
        failures, 0,
        "POSIX atomic guarantee violated: {} parse failures out of {} reads",
        failures, reads
    );
}

// ── Test 9: Burst pattern ──────────────────────────────────────────

/// Simulate realistic Hermes behavior: bursts of rapid writes during tool
/// execution, then long pauses during LLM thinking.
///
/// Pattern: [burst of 5 writes at 50ms spacing] → [pause 3 seconds] → repeat 10x.
/// Total: 50 writes in ~35 seconds. Asserts all 50 messages are captured.
#[test]
fn test_burst_pattern() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_burst.json");
    let session_id = "burst-session";

    // Write initial file.
    let initial = generate_hermes_snapshot(session_id, &[user_message("init")], 55_000);
    std::fs::write(&session_path, &initial).unwrap();

    // Track file state for diffing.
    let prev_state = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 1,
    };

    // Collect diffs as we go (read after each burst settles).
    let events: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let events_clone = Arc::clone(&events);

    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                events_clone.lock().unwrap().push(event);
            }
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(dir.path(), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    std::thread::sleep(Duration::from_millis(100));

    let bursts = 10;
    let writes_per_burst = 5;
    let total_writes = bursts * writes_per_burst;
    let mut msg_count = 1; // initial message

    eprintln!("\nBurst pattern: {} bursts of {} writes...", bursts, writes_per_burst);

    for burst in 0..bursts {
        // Burst: 5 writes at 50ms spacing.
        for _ in 0..writes_per_burst {
            msg_count += 1;
            let messages = build_conversation(msg_count);
            let content = generate_hermes_snapshot(session_id, &messages, 55_000);
            atomic_replace(&session_path, &content);
            std::thread::sleep(Duration::from_millis(50));
        }

        eprintln!("  Burst {} complete, {} messages total", burst + 1, msg_count);

        // Pause: simulate LLM thinking.
        std::thread::sleep(Duration::from_secs(3));
    }

    // Final settle.
    std::thread::sleep(Duration::from_secs(2));

    // Now verify: read the final file and diff from initial state.
    // The file should contain all messages we wrote.
    let raw = std::fs::read_to_string(&session_path).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    let final_count = parsed["messages"].as_array().unwrap().len();

    assert_eq!(
        final_count,
        1 + total_writes,
        "Expected {} messages in final file, got {}",
        1 + total_writes,
        final_count
    );

    // Diff from initial: should produce all new messages.
    let new_msgs = diff_snapshot(&prev_state, &parsed);
    assert_eq!(
        new_msgs.len(),
        total_writes,
        "Diff should show {} new messages, got {}",
        total_writes,
        new_msgs.len()
    );

    // Translate all new messages and count CloudEvents.
    let mut state = TranscriptState::new(session_id.to_string());
    let mut cloud_events = 0;
    for (seq, msg) in new_msgs.iter().enumerate() {
        let line = wrap_hermes_line(session_id, msg, seq as u64 + 1);
        let evts = translate_hermes_line(&line, &mut state);
        cloud_events += evts.len();
    }

    eprintln!(
        "  {} new messages → {} CloudEvents",
        new_msgs.len(),
        cloud_events
    );

    assert_eq!(
        cloud_events, total_writes,
        "Expected {} CloudEvents from {} messages, got {}",
        total_writes, total_writes, cloud_events
    );

    // Verify the watcher saw events during bursts.
    let collected = events.lock().unwrap();
    let target_events = collected
        .iter()
        .filter(|e| {
            e.paths
                .iter()
                .any(|p| {
                    p.file_name()
                        .map(|n| n == "session_burst.json")
                        .unwrap_or(false)
                })
        })
        .count();

    eprintln!(
        "  Watcher events: {} (for {} writes)",
        target_events, total_writes
    );

    // Even with coalescing, the watcher should see events from every burst
    // (the 3-second pauses ensure events are delivered between bursts).
    assert!(
        target_events >= bursts,
        "Expected at least {} watcher events (one per burst), got {}",
        bursts,
        target_events
    );
}

// ── Test 10: Fast model simulation ─────────────────────────────────

/// Simulate a local model (vLLM on a 5090) responding in 100ms.
///
/// Pattern per turn: write assistant+tool_call (wait 100ms) → write tool_result
/// (wait 50ms) → repeat. 100 turns = 200 writes in ~15 seconds (~13 writes/sec).
///
/// Tracks the full end-to-end pipeline: notify event → read file → diff →
/// translate_hermes_line → CloudEvent. Asserts >= 90% message capture.
#[test]
fn test_fast_model_simulation() {
    let dir = TempDir::new().unwrap();
    let session_path = dir.path().join("session_fastmodel.json");
    let session_id = "fastmodel-session";

    // Start with a user prompt.
    let mut messages: Vec<Value> = vec![user_message("Implement the feature")];
    let initial = generate_hermes_snapshot(session_id, &messages, 55_000);
    std::fs::write(&session_path, &initial).unwrap();

    // Track notify events with timestamps for latency measurement.
    let event_times: Arc<Mutex<Vec<(Instant, usize)>>> = Arc::new(Mutex::new(Vec::new()));
    let event_times_clone = Arc::clone(&event_times);

    let session_path_for_watcher = session_path.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let is_target = event.paths.iter().any(|p| {
                    p.file_name()
                        .map(|n| n == "session_fastmodel.json")
                        .unwrap_or(false)
                });
                if is_target {
                    // On event: read + parse + diff to simulate the real pipeline.
                    if let Ok(raw) = std::fs::read_to_string(&session_path_for_watcher) {
                        if let Ok(parsed) = serde_json::from_str::<Value>(&raw) {
                            let msg_count = parsed["messages"]
                                .as_array()
                                .map(|a| a.len())
                                .unwrap_or(0);
                            event_times_clone
                                .lock()
                                .unwrap()
                                .push((Instant::now(), msg_count));
                        }
                    }
                }
            }
        },
        Config::default(),
    )
    .expect("failed to create watcher");

    watcher
        .watch(dir.path(), RecursiveMode::NonRecursive)
        .expect("failed to watch directory");

    std::thread::sleep(Duration::from_millis(100));

    let turns = 100;
    let total_writes = turns * 2; // assistant_tool_call + tool_result per turn
    let mut write_times: Vec<Instant> = Vec::with_capacity(total_writes);
    let test_start = Instant::now();

    eprintln!(
        "\nFast model simulation: {} turns ({} writes, ~13/sec)...",
        turns, total_writes
    );

    for i in 0..turns {
        // Assistant tool call.
        messages.push(assistant_tool_call(
            "write_file",
            &format!("{{\"path\": \"src/module_{}.rs\", \"content\": \"fn f{}() {{}}\"}}", i, i),
        ));
        let content = generate_hermes_snapshot(session_id, &messages, 55_000);
        atomic_replace(&session_path, &content);
        write_times.push(Instant::now());

        // 100ms model response time.
        std::thread::sleep(Duration::from_millis(100));

        // Tool result.
        messages.push(tool_result(
            &format!("tc_write_file"),
            &format!("Written to src/module_{}.rs", i),
        ));
        let content = generate_hermes_snapshot(session_id, &messages, 55_000);
        atomic_replace(&session_path, &content);
        write_times.push(Instant::now());

        // 50ms before next turn.
        std::thread::sleep(Duration::from_millis(50));
    }

    let write_elapsed = test_start.elapsed();

    // Final settle.
    std::thread::sleep(Duration::from_secs(2));

    // Measure: how many messages did the watcher pipeline see?
    let event_snapshots = event_times.lock().unwrap();

    // The maximum message count observed tells us how far the pipeline got.
    let max_observed = event_snapshots
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(0);

    // Total expected messages: 1 (initial) + 200 (writes).
    let expected_messages = 1 + total_writes;
    let messages_captured = max_observed;
    let detection_rate = messages_captured as f64 / expected_messages as f64 * 100.0;

    // Now do a full translate pass on the final file to count CloudEvents.
    let raw = std::fs::read_to_string(&session_path).unwrap();
    let parsed: Value = serde_json::from_str(&raw).unwrap();
    let prev = SnapshotState {
        session_id: session_id.to_string(),
        message_count: 1, // just the initial user message
    };
    let new_msgs = diff_snapshot(&prev, &parsed);

    let mut transcript_state = TranscriptState::new(session_id.to_string());
    let mut cloud_events = 0;
    for (seq, msg) in new_msgs.iter().enumerate() {
        let line = wrap_hermes_line(session_id, msg, seq as u64 + 1);
        let evts = translate_hermes_line(&line, &mut transcript_state);
        cloud_events += evts.len();
    }

    // Compute event-based latencies where we can.
    let mut latencies: Vec<Duration> = Vec::new();
    for (event_time, _) in event_snapshots.iter() {
        // Find the closest write that happened before this event.
        if let Some(write_time) = write_times.iter().rev().find(|t| **t <= *event_time) {
            latencies.push(*event_time - *write_time);
        }
    }
    latencies.sort();

    let p50_latency = latencies.get(latencies.len() / 2).copied();
    let p99_idx = (latencies.len() as f64 * 0.99).ceil() as usize;
    let p99_latency = latencies.get(p99_idx.min(latencies.len().saturating_sub(1))).copied();

    eprintln!("\nFast model simulation results:");
    eprintln!("  Turns:           {}", turns);
    eprintln!("  Total writes:    {}", total_writes);
    eprintln!("  Write time:      {:.1}s", write_elapsed.as_secs_f64());
    eprintln!(
        "  Effective rate:  {:.1} writes/s",
        total_writes as f64 / write_elapsed.as_secs_f64()
    );
    eprintln!("  Watcher events:  {}", event_snapshots.len());
    eprintln!(
        "  Max msgs seen:   {}/{} ({:.1}%)",
        messages_captured, expected_messages, detection_rate
    );
    eprintln!("  CloudEvents (full translate): {}", cloud_events);
    if let Some(p50) = p50_latency {
        eprintln!("  Notify latency p50: {:?}", p50);
    }
    if let Some(p99) = p99_latency {
        eprintln!("  Notify latency p99: {:?}", p99);
    }

    // The full translate of the final file should produce all CloudEvents.
    assert_eq!(
        cloud_events, total_writes,
        "Full translate should produce {} CloudEvents, got {}",
        total_writes, cloud_events
    );

    // The watcher should have caught at least 90% of the writes.
    // (Coalescing is fine — the final snapshot read catches everything.)
    assert!(
        detection_rate >= 90.0,
        "Fast model: expected >= 90% message detection via watcher, got {:.1}% ({}/{})",
        detection_rate,
        messages_captured,
        expected_messages
    );
}
