//! File system watcher using the `notify` crate.
//!
//! Recursively watches a directory for JSONL file modifications.
//! Maintains per-file TranscriptState for incremental reading.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, SystemTime};

use anyhow::Result;
use notify::{Config, Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use walkdir::WalkDir;

/// Max events per NATS batch. Keeps message size under NATS max_payload (default 1MB).
/// 100 events × ~5-10KB each ≈ 500KB-1MB per message.
const BATCH_CHUNK_SIZE: usize = 100;

use crate::cloud_event::CloudEvent;
use crate::output::emit_events;
use crate::paths::{nats_subject_from_path, project_id_from_path, session_id_from_path};
use crate::reader::read_new_lines;
use crate::translate::TranscriptState;

/// Process a single file: read new lines, return events.
fn process_file_raw(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
) -> Result<Vec<CloudEvent>> {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Ok(vec![]);
    }

    let canonical = path.to_path_buf();
    let state = states
        .entry(canonical.clone())
        .or_insert_with(|| TranscriptState::new(session_id_from_path(path)));

    read_new_lines(path, state)
}

/// Process a single file: read new lines, emit events.
fn process_file(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
    output_file: Option<&Path>,
    stdout: bool,
) -> Result<Vec<CloudEvent>> {
    let events = process_file_raw(path, states)?;
    if !events.is_empty() {
        emit_events(&events, output_file, stdout)?;
    }
    Ok(events)
}

/// Backfill: process all existing JSONL files in the watch directory.
pub fn backfill(
    watch_dir: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
    output_file: Option<&Path>,
    stdout: bool,
) -> Result<u64> {
    let mut total = 0u64;
    for entry in WalkDir::new(watch_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let events = process_file(path, states, output_file, stdout)?;
            total += events.len() as u64;
        }
    }
    Ok(total)
}

/// Watch a directory with a callback for each batch of events.
///
/// This blocks the current thread. The callback receives
/// `(session_id, project_id, subject, events)`.
///
/// `backfill_window_hours` controls the startup backfill: when `Some(h)` with
/// `h > 0`, files whose mtime is older than `h` hours are skipped; `Some(0)`
/// disables the filter (load every JSONL the watcher sees, regardless of
/// age — useful for tests with static fixture data); `None` skips backfill
/// entirely (only events that arrive after startup are processed).
pub fn watch_with_callback<F>(
    watch_dir: &Path,
    backfill_window_hours: Option<u64>,
    mut on_events: F,
) -> Result<()>
where
    F: FnMut(&str, Option<&str>, &str, Vec<CloudEvent>),
{
    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();

    if let Some(window_hours) = backfill_window_hours {
        let now = SystemTime::now();
        let window = if window_hours == 0 {
            None // unlimited — load every file
        } else {
            Some(Duration::from_secs(window_hours * 3600))
        };

        let mut total = 0u64;
        let mut skipped = 0u64;
        for entry in WalkDir::new(watch_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }

            // Skip files older than the configured backfill window. None = no filter.
            let in_window = match window {
                None => true,
                Some(w) => entry
                    .metadata()
                    .ok()
                    .and_then(|m| m.modified().ok())
                    .map(|mtime| {
                        now.duration_since(mtime).unwrap_or(Duration::ZERO) <= w
                    })
                    .unwrap_or(false),
            };

            if !in_window {
                skipped += 1;
                continue;
            }

            let events = process_file_raw(path, &mut states)?;
            if !events.is_empty() {
                total += events.len() as u64;
                let sid = session_id_from_path(path);
                let pid = project_id_from_path(path, watch_dir);
                let subject = nats_subject_from_path(path, watch_dir);
                for chunk in events.chunks(BATCH_CHUNK_SIZE) {
                    on_events(&sid, pid.as_deref(), &subject, chunk.to_vec());
                }
            }
        }

        let window_desc = match window {
            None => "all files (no window filter)".to_string(),
            Some(w) => format!("files modified within {}h", w.as_secs() / 3600),
        };
        eprintln!(
            "Backfilled {} events from {} (skipped {} older)",
            total, window_desc, skipped
        );
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(watch_dir, RecursiveMode::Recursive)?;

    eprintln!("Watching {} for JSONL changes...", watch_dir.display());

    for res in rx {
        match res {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    for path in &event.paths {
                        match process_file_raw(path, &mut states) {
                            Ok(events) if !events.is_empty() => {
                                let sid = session_id_from_path(path);
                                let pid = project_id_from_path(path, watch_dir);
                                let subject = nats_subject_from_path(path, watch_dir);
                                for chunk in events.chunks(BATCH_CHUNK_SIZE) {
                                    on_events(&sid, pid.as_deref(), &subject, chunk.to_vec());
                                }
                            }
                            Ok(_) => {}
                            Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
                        }
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        }
    }

    Ok(())
}

/// Watch a directory for JSONL file changes and emit CloudEvents.
///
/// This blocks the current thread. Use Ctrl+C or drop the watcher to stop.
pub fn watch_directory(
    watch_dir: &Path,
    output_file: Option<&Path>,
    stdout: bool,
    do_backfill: bool,
) -> Result<()> {
    let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();

    if do_backfill {
        let count = backfill(watch_dir, &mut states, output_file, stdout)?;
        eprintln!("Backfilled {} events from existing files", count);
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(watch_dir, RecursiveMode::Recursive)?;

    eprintln!("Watching {} for JSONL changes...", watch_dir.display());

    for res in rx {
        match res {
            Ok(event) => {
                if matches!(
                    event.kind,
                    EventKind::Modify(_) | EventKind::Create(_)
                ) {
                    for path in &event.paths {
                        if let Err(e) = process_file(path, &mut states, output_file, stdout) {
                            eprintln!("Error processing {}: {}", path.display(), e);
                        }
                    }
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        }
    }

    Ok(())
}

