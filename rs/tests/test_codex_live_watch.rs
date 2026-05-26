use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;
use tempfile::TempDir;
use walkdir::WalkDir;

#[derive(Debug)]
struct TimelineEntry {
    at_ms: u128,
    phase: &'static str,
    detail: String,
}

fn push_timeline(
    timeline: &mut Vec<TimelineEntry>,
    start: Instant,
    phase: &'static str,
    detail: impl Into<String>,
) {
    timeline.push(TimelineEntry {
        at_ms: start.elapsed().as_millis(),
        phase,
        detail: detail.into(),
    });
}

fn codex_tree(root: &Path) -> PathBuf {
    root.join("sessions").join("2026").join("05").join("24")
}

fn rollout_path(root: &Path) -> PathBuf {
    codex_tree(root).join("rollout-2026-05-24T09-01-22-019e5a13-69cf-7b13-baeb-d6891eafd55e.jsonl")
}

fn codex_line(timestamp: &str, kind: &str, payload: &str) -> String {
    format!(r#"{{"timestamp":"{timestamp}","type":"{kind}","payload":{payload}}}"#)
}

fn append_line(path: &Path, line: &str) {
    let mut file = OpenOptions::new()
        .append(true)
        .open(path)
        .expect("open rollout for append");
    writeln!(file, "{line}").expect("append rollout line");
    file.flush().expect("flush rollout append");
    file.sync_data().expect("sync rollout append");
}

fn process_notify_paths(
    paths: &[PathBuf],
    states: &mut HashMap<PathBuf, TranscriptState>,
) -> Vec<String> {
    let mut subtypes = Vec::new();
    for path in paths {
        if path.is_dir() {
            for entry in WalkDir::new(path)
                .follow_links(true)
                .into_iter()
                .filter_map(Result::ok)
            {
                let candidate = entry.path();
                if candidate.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                subtypes.extend(read_path(candidate, states));
            }
        } else {
            subtypes.extend(read_path(path, states));
        }
    }
    subtypes
}

fn read_path(path: &Path, states: &mut HashMap<PathBuf, TranscriptState>) -> Vec<String> {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Vec::new();
    }

    let state = states.entry(path.to_path_buf()).or_insert_with(|| {
        TranscriptState::new(
            "rollout-2026-05-24T09-01-22-019e5a13-69cf-7b13-baeb-d6891eafd55e".to_string(),
        )
    });
    read_new_lines(path, state)
        .expect("read new codex rollout lines")
        .into_iter()
        .filter_map(|event| event.subtype)
        .collect()
}

enum Observation {
    Write {
        at: Instant,
        detail: String,
    },
    Notify {
        at: Instant,
        event: notify::Result<Event>,
    },
}

fn print_timeline(timeline: &[TimelineEntry]) {
    eprintln!("\nCodex live watch timeline");
    eprintln!("+--------+----------------+----------------------------------------------");
    eprintln!("| t+ms   | phase          | detail");
    eprintln!("+--------+----------------+----------------------------------------------");
    for entry in timeline {
        eprintln!(
            "| {:>6} | {:<14} | {}",
            entry.at_ms, entry.phase, entry.detail
        );
    }
    eprintln!("+--------+----------------+----------------------------------------------\n");
}

#[test]
fn codex_rollout_appends_are_seen_by_recursive_notify_and_reader() {
    let tmp = TempDir::new().expect("temp dir");
    let root = tmp.path().canonicalize().expect("canonical temp dir");
    let sessions_root = root.join("sessions");
    let codex_day = codex_tree(&root);
    std::fs::create_dir_all(&codex_day).expect("create codex date tree");
    let rollout = rollout_path(&root);

    let start = Instant::now();
    let mut timeline = Vec::new();
    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();

    let session_meta = codex_line(
        "2026-05-24T13:01:22.000Z",
        "session_meta",
        r#"{"id":"019e5a13-69cf-7b13-baeb-d6891eafd55e","timestamp":"2026-05-24T13:01:22.000Z","cwd":"/tmp/openstory-codex-live","originator":"codex-tui","cli_version":"0.133.0","source":"cli","thread_source":"user","model_provider":"openai"}"#,
    );
    std::fs::write(&rollout, format!("{session_meta}\n")).expect("write initial rollout");
    push_timeline(
        &mut timeline,
        start,
        "write",
        "initial session_meta before watcher",
    );

    let backfill_subtypes = read_path(&rollout, &mut states);
    push_timeline(
        &mut timeline,
        start,
        "backfill",
        format!("{backfill_subtypes:?}"),
    );

    let (tx, rx) = mpsc::channel::<Observation>();
    let notify_tx = tx.clone();
    let mut watcher = RecommendedWatcher::new(
        move |res| {
            let _ = notify_tx.send(Observation::Notify {
                at: Instant::now(),
                event: res,
            });
        },
        Config::default(),
    )
    .expect("create watcher");
    watcher
        .watch(&sessions_root, RecursiveMode::Recursive)
        .expect("watch codex sessions root");
    push_timeline(
        &mut timeline,
        start,
        "watch",
        format!("recursive {}", sessions_root.display()),
    );

    std::thread::sleep(Duration::from_millis(250));
    while rx.try_recv().is_ok() {}

    let appended: Vec<String> = vec![
        codex_line(
            "2026-05-24T13:01:23.000Z",
            "event_msg",
            r#"{"type":"user_message","message":"live prompt","images":[],"local_images":[],"text_elements":[]}"#,
        ),
        codex_line(
            "2026-05-24T13:01:24.000Z",
            "response_item",
            r#"{"type":"function_call","name":"exec_command","arguments":"{\"cmd\":\"pwd\"}","call_id":"call_live"}"#,
        ),
        codex_line(
            "2026-05-24T13:01:25.000Z",
            "response_item",
            r#"{"type":"function_call_output","call_id":"call_live","output":"Output:\n/tmp/openstory-codex-live\n"}"#,
        ),
        codex_line(
            "2026-05-24T13:01:26.000Z",
            "event_msg",
            r#"{"type":"agent_message","message":"live answer","phase":"final_answer","memory_citation":null}"#,
        ),
    ];

    let writer_tx = tx.clone();
    let writer_rollout = rollout.clone();
    let writer = std::thread::spawn(move || {
        for (index, line) in appended.iter().enumerate() {
            append_line(&writer_rollout, line);
            let _ = writer_tx.send(Observation::Write {
                at: Instant::now(),
                detail: format!("line {} byte_len={}", index + 1, line.len() + 1),
            });
            std::thread::sleep(Duration::from_millis(75));
        }
    });

    let expected = vec![
        "message.user.prompt".to_string(),
        "message.assistant.tool_use".to_string(),
        "message.user.tool_result".to_string(),
        "message.assistant.text".to_string(),
    ];
    let mut observed = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);

    while Instant::now() < deadline && observed.len() < expected.len() {
        let timeout = deadline
            .checked_duration_since(Instant::now())
            .unwrap_or(Duration::ZERO)
            .min(Duration::from_millis(500));
        let Ok(observation) = rx.recv_timeout(timeout) else {
            continue;
        };
        match observation {
            Observation::Write { at, detail } => push_timeline(
                &mut timeline,
                start,
                "append",
                format!("t+{}ms {detail}", at.duration_since(start).as_millis()),
            ),
            Observation::Notify { at, event } => match event {
                Ok(event) => {
                    let raw_detail = format!("{:?} {:?}", event.kind, event.paths);
                    push_timeline(
                        &mut timeline,
                        start,
                        "notify",
                        format!("t+{}ms {raw_detail}", at.duration_since(start).as_millis()),
                    );
                    let subtypes = process_notify_paths(&event.paths, &mut states);
                    if !subtypes.is_empty() {
                        push_timeline(&mut timeline, start, "reader", format!("{subtypes:?}"));
                        observed.extend(subtypes);
                    }
                }
                Err(error) => {
                    push_timeline(&mut timeline, start, "notify-error", error.to_string())
                }
            },
        }
    }
    writer.join().expect("writer thread");

    print_timeline(&timeline);
    assert_eq!(observed, expected);
}
