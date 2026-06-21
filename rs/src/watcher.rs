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
use open_story_server::metrics;
use open_story_server::watcher_diagnostics::{
    FileProcessObservation, WatcherBackfillSnapshot, WatcherDiagnostics, canonicalize_path,
};
use walkdir::WalkDir;

/// Max events per NATS batch. Keeps message size under NATS max_payload (default 1MB).
/// 100 events × ~5-10KB each ≈ 500KB-1MB per message.
const BATCH_CHUNK_SIZE: usize = 100;

use crate::cloud_event::CloudEvent;
use crate::output::emit_events;
use crate::paths::{nats_subject_from_path, project_id_from_path, session_id_from_path};
use crate::reader::read_new_lines;
use crate::translate::{TranscriptFormat, TranscriptState};

#[derive(Clone)]
pub struct WatcherObserver {
    pub diagnostics: WatcherDiagnostics,
    pub actor: String,
}

/// Process a single file: read new lines, return events.
fn process_file_raw(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
) -> Result<Vec<CloudEvent>> {
    process_file_raw_observed(path, states, None)
}

/// Cap on retained per-file watcher state. Each file the watcher has processed
/// keeps one `TranscriptState` (byte offset, line count, session id) so reads
/// stay incremental. Finished sessions never drop out on their own, so a
/// long-lived process would grow unboundedly. When the cap is crossed we evict
/// the states of the least-recently-*modified* files — finished sessions, which
/// won't be written again. If an evicted file is somehow appended to later, it
/// simply re-reads from offset 0 and the ingest-layer event-id dedup absorbs the
/// replay. ~200 bytes/entry → the cap bounds this map to well under 1 MB.
const MAX_WATCH_STATES: usize = 4096;
/// Evict down to this low-water mark so eviction amortizes instead of firing on
/// every new file once at the cap.
const WATCH_STATES_LOW_WATER: usize = 3584;

/// Drop the oldest-by-mtime entries when `states` exceeds `cap`, down to
/// `low_water`. mtime is only stat'd when actually over the cap, so steady
/// state is free.
fn evict_stale_states(
    states: &mut HashMap<PathBuf, TranscriptState>,
    cap: usize,
    low_water: usize,
) {
    if states.len() <= cap {
        return;
    }
    let mut by_mtime: Vec<(PathBuf, SystemTime)> = states
        .keys()
        .map(|p| {
            let mtime = std::fs::metadata(p)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            (p.clone(), mtime)
        })
        .collect();
    by_mtime.sort_by_key(|(_, t)| *t); // oldest first
    let evict = states.len().saturating_sub(low_water);
    for (p, _) in by_mtime.into_iter().take(evict) {
        states.remove(&p);
    }
}

fn process_file_raw_observed(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
    observer: Option<&WatcherObserver>,
) -> Result<Vec<CloudEvent>> {
    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
        return Ok(vec![]);
    }

    let canonical = canonicalize_path(path);
    let grew = !states.contains_key(&canonical);
    let state = states
        .entry(canonical.clone())
        .or_insert_with(|| TranscriptState::new(session_id_from_path(path)));
    let byte_offset_before = state.byte_offset;
    let line_count_before = state.line_count;

    let events = read_new_lines(path, state)?;

    if let Some(observer) = observer {
        let byte_offset_after = state.byte_offset;
        let line_count_after = state.line_count;
        let subtypes = events
            .iter()
            .filter_map(|event| event.subtype.clone())
            .collect::<Vec<_>>();
        observer.diagnostics.record_file(
            &observer.actor,
            FileProcessObservation {
                path: path.to_path_buf(),
                canonical_path: canonical,
                byte_offset_before,
                byte_offset_after,
                line_count_before,
                line_count_after,
                format: transcript_format_label(&state.format).to_string(),
                events_emitted: events.len(),
                subtypes,
            },
        );
        metrics::record_watcher_file_processed(
            &observer.actor,
            events.len() as u64,
            byte_offset_after == byte_offset_before,
        );
    }

    // Only when a brand-new file was added — steady-state reads are free.
    if grew {
        evict_stale_states(states, MAX_WATCH_STATES, WATCH_STATES_LOW_WATER);
    }

    Ok(events)
}

fn process_watch_path_raw(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
) -> Result<Vec<(PathBuf, Vec<CloudEvent>)>> {
    process_watch_path_raw_observed(path, states, None)
}

fn process_watch_path_raw_observed(
    path: &Path,
    states: &mut HashMap<PathBuf, TranscriptState>,
    observer: Option<&WatcherObserver>,
) -> Result<Vec<(PathBuf, Vec<CloudEvent>)>> {
    if path.is_dir() {
        let mut batches = Vec::new();
        for entry in WalkDir::new(path)
            .follow_links(true)
            .into_iter()
            .filter_map(|entry| entry.ok())
        {
            let candidate = entry.path();
            if candidate.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let events = process_file_raw_observed(candidate, states, observer)?;
            if !events.is_empty() {
                batches.push((candidate.to_path_buf(), events));
            }
        }
        Ok(batches)
    } else {
        let events = process_file_raw_observed(path, states, observer)?;
        if events.is_empty() {
            Ok(Vec::new())
        } else {
            Ok(vec![(path.to_path_buf(), events)])
        }
    }
}

fn transcript_format_label(format: &TranscriptFormat) -> &'static str {
    match format {
        TranscriptFormat::Unknown => "unknown",
        TranscriptFormat::ClaudeCode => "claude-code",
        TranscriptFormat::Codex => "codex",
        TranscriptFormat::PiMono => "pi-mono",
        TranscriptFormat::Hermes => "hermes",
    }
}

/// Decide whether a file's mtime falls within the backfill window.
///
/// `window` semantics:
///   - `None` — no upper bound; every file qualifies (`true`)
///   - `Some(d)` — file qualifies only when `now - mtime <= d`
///
/// Pure function — extracted from `watch_with_callback` so the
/// boundary table can be tested without spinning up a watcher.
fn is_in_backfill_window(window: Option<Duration>, mtime: SystemTime, now: SystemTime) -> bool {
    match window {
        None => true,
        Some(w) => now.duration_since(mtime).unwrap_or(Duration::ZERO) <= w,
    }
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
    on_events: F,
) -> Result<()>
where
    F: FnMut(&str, Option<&str>, &str, Vec<CloudEvent>),
{
    watch_with_callback_observed(watch_dir, backfill_window_hours, None, on_events)
}

pub fn watch_with_callback_observed<F>(
    watch_dir: &Path,
    backfill_window_hours: Option<u64>,
    observer: Option<WatcherObserver>,
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
        let mut files_seen = 0u64;
        let mut files_loaded = 0u64;
        for entry in WalkDir::new(watch_dir)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            files_seen += 1;

            // Skip files older than the configured backfill window.
            let in_window = match entry.metadata().ok().and_then(|m| m.modified().ok()) {
                Some(mtime) => is_in_backfill_window(window, mtime, now),
                None => false, // can't read mtime → safest to skip
            };

            if !in_window {
                skipped += 1;
                continue;
            }

            let events = process_file_raw_observed(path, &mut states, observer.as_ref())?;
            files_loaded += 1;
            if !events.is_empty() {
                total += events.len() as u64;
                let sid = session_id_from_path(path);
                let pid = project_id_from_path(path, watch_dir);
                let subject = nats_subject_from_path(path, watch_dir, open_story_core::host::host());
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
        if let Some(observer) = observer.as_ref() {
            observer.diagnostics.record_backfill(
                &observer.actor,
                WatcherBackfillSnapshot {
                    window_hours: Some(window_hours),
                    files_seen,
                    files_loaded,
                    events_emitted: total,
                    files_skipped: skipped,
                },
            );
        }
    }

    // macOS: prefer kqueue (precise per-write vnode events). FSEvents coalesces
    // and silently drops held-open buffered appends — e.g. Codex rollout files,
    // for which FSEvents fired ~2 events across a 47-minute session. See
    // kqueue_watcher.rs. Escape hatch: OPEN_STORY_WATCHER=fsevents.
    #[cfg(target_os = "macos")]
    if std::env::var("OPEN_STORY_WATCHER").as_deref() != Ok("fsevents") {
        let process = |path: &Path| {
            if let Some(obs) = observer.as_ref() {
                let p = vec![path.to_path_buf()];
                obs.diagnostics
                    .record_notify(&obs.actor, "kqueue:vnode", &p, true, None);
            }
            match process_watch_path_raw_observed(path, &mut states, observer.as_ref()) {
                Ok(batches) => {
                    for (source_path, events) in batches {
                        let sid = session_id_from_path(&source_path);
                        let pid = project_id_from_path(&source_path, watch_dir);
                        let subject = nats_subject_from_path(&source_path, watch_dir);
                        for chunk in events.chunks(BATCH_CHUNK_SIZE) {
                            on_events(&sid, pid.as_deref(), &subject, chunk.to_vec());
                        }
                    }
                }
                Err(e) => eprintln!("Error processing {}: {}", path.display(), e),
            }
        };
        return crate::kqueue_watcher::run_event_loop(
            watch_dir,
            crate::kqueue_watcher::DEFAULT_FILE_BUDGET,
            process,
        )
        .map_err(anyhow::Error::from);
    }

    let (tx, rx) = mpsc::channel::<notify::Result<Event>>();

    let mut watcher = RecommendedWatcher::new(tx, Config::default())?;
    watcher.watch(watch_dir, RecursiveMode::Recursive)?;

    eprintln!("Watching {} for JSONL changes...", watch_dir.display());

    for res in rx {
        match res {
            Ok(event) => {
                let kind = format!("{:?}", event.kind);
                let accepted = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                if let Some(observer) = observer.as_ref() {
                    observer.diagnostics.record_notify(
                        &observer.actor,
                        kind.as_str(),
                        &event.paths,
                        accepted,
                        (!accepted).then_some("unsupported_kind"),
                    );
                    metrics::record_watcher_raw_event(&observer.actor, kind.as_str());
                    if !accepted {
                        metrics::record_watcher_ignored_event(&observer.actor, "unsupported_kind");
                    }
                }
                if accepted {
                    for path in &event.paths {
                        match process_watch_path_raw_observed(path, &mut states, observer.as_ref())
                        {
                            Ok(batches) => {
                                for (source_path, events) in batches {
                                    let sid = session_id_from_path(&source_path);
                                    let pid = project_id_from_path(&source_path, watch_dir);
                                    let subject =
                                        nats_subject_from_path(&source_path, watch_dir, open_story_core::host::host());
                                    for chunk in events.chunks(BATCH_CHUNK_SIZE) {
                                        on_events(&sid, pid.as_deref(), &subject, chunk.to_vec());
                                    }
                                }
                            }
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── process_file_raw — file detection + state initialization ──────

    #[test]
    fn process_file_raw_returns_empty_for_non_jsonl() {
        let tmp = tempfile::NamedTempFile::with_suffix(".txt").unwrap();
        std::fs::write(tmp.path(), "not a jsonl file\n").unwrap();
        let mut states = HashMap::new();
        let events = process_file_raw(tmp.path(), &mut states).unwrap();
        assert_eq!(events.len(), 0);
        assert!(
            states.is_empty(),
            "non-jsonl file must not initialize state"
        );
    }

    #[test]
    fn process_file_raw_initializes_state_on_first_call() {
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        std::fs::write(tmp.path(), "").unwrap(); // empty jsonl is valid
        let mut states = HashMap::new();

        let _ = process_file_raw(tmp.path(), &mut states).unwrap();
        assert_eq!(
            states.len(),
            1,
            "first call on a jsonl path must initialize TranscriptState"
        );
    }

    #[test]
    fn process_file_raw_reuses_state_across_calls() {
        // Two calls on the same file should share state — that's what
        // gives us incremental reads (byte_offset advances). If a
        // future change recreated state on each call, every poll would
        // re-emit every event from byte 0.
        let tmp = tempfile::NamedTempFile::with_suffix(".jsonl").unwrap();
        std::fs::write(tmp.path(), "").unwrap();
        let mut states = HashMap::new();

        process_file_raw(tmp.path(), &mut states).unwrap();
        process_file_raw(tmp.path(), &mut states).unwrap();

        assert_eq!(states.len(), 1, "state must be reused, not recreated");
    }

    #[test]
    #[cfg(unix)]
    fn process_file_raw_reuses_state_for_equivalent_paths() {
        // macOS can report the same file through different spellings
        // (`/var/...` vs `/private/var/...`, symlinked paths, etc.). The
        // watcher state key must be canonical, or a later notify event can
        // re-read an already-backfilled rollout from byte 0.
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("session.jsonl");
        std::fs::write(&real, "").unwrap();
        let alias = dir.path().join("alias.jsonl");
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let mut states = HashMap::new();

        process_file_raw(&real, &mut states).unwrap();
        process_file_raw(&alias, &mut states).unwrap();

        assert_eq!(states.len(), 1, "equivalent paths must share state");
    }

    #[test]
    fn evict_stale_states_drops_oldest_keeps_newest() {
        use filetime::{set_file_mtime, FileTime};
        let dir = tempfile::tempdir().unwrap();
        let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
        let mut paths = Vec::new();
        for (i, name) in ["old.jsonl", "mid.jsonl", "new.jsonl"].iter().enumerate() {
            let p = dir.path().join(name);
            std::fs::write(&p, b"").unwrap();
            // Strictly increasing mtime so eviction order is deterministic.
            set_file_mtime(&p, FileTime::from_unix_time(1000 + i as i64 * 100, 0)).unwrap();
            states.insert(p.clone(), TranscriptState::new("s".to_string()));
            paths.push(p);
        }
        // cap 2, low_water 1: 3 > 2 → evict down to 1, keeping the newest.
        evict_stale_states(&mut states, 2, 1);
        assert_eq!(states.len(), 1, "must evict down to the low-water mark");
        assert!(states.contains_key(&paths[2]), "newest mtime must survive");
        assert!(!states.contains_key(&paths[0]), "oldest mtime must be evicted");
    }

    #[test]
    fn evict_stale_states_is_a_noop_under_cap() {
        let dir = tempfile::tempdir().unwrap();
        let mut states: HashMap<PathBuf, TranscriptState> = HashMap::new();
        let p = dir.path().join("a.jsonl");
        std::fs::write(&p, b"").unwrap();
        states.insert(p, TranscriptState::new("s".to_string()));
        evict_stale_states(&mut states, 10, 5);
        assert_eq!(states.len(), 1, "under the cap nothing is evicted");
    }

    #[test]
    fn process_watch_path_raw_expands_directory_events_to_nested_jsonl_files() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("2026").join("05").join("24");
        std::fs::create_dir_all(&nested).unwrap();
        let rollout =
            nested.join("rollout-2026-05-24T09-01-22-019e5a13-69cf-7b13-baeb-d6891eafd55e.jsonl");
        std::fs::write(
            &rollout,
            r#"{"timestamp":"2026-05-24T13:01:22.000Z","type":"session_meta","payload":{"id":"019e5a13-69cf-7b13-baeb-d6891eafd55e","cwd":"/Users/maxglassie/projects/OpenStory/rs","originator":"codex-api","cli_version":"0.133.0"}}
"#,
        )
        .unwrap();

        let mut states = HashMap::new();
        let batches = process_watch_path_raw(dir.path(), &mut states).unwrap();
        let events: Vec<CloudEvent> = batches
            .iter()
            .flat_map(|(_, events)| events.iter().cloned())
            .collect();

        assert_eq!(events.len(), 1);
        // State is keyed by canonical path (macOS reports tempdirs under
        // `/var/...` which canonicalizes to `/private/var/...`).
        assert!(states.contains_key(&canonicalize_path(&rollout)));
        assert_eq!(
            events[0].data.session_id,
            "019e5a13-69cf-7b13-baeb-d6891eafd55e"
        );
    }

    // ── is_in_backfill_window — boundary table ────────────────────────

    fn now_minus(secs: u64) -> SystemTime {
        SystemTime::now() - Duration::from_secs(secs)
    }

    #[test]
    fn backfill_window_none_accepts_every_file() {
        // `window = None` is the test/explicit-load case ("load every
        // JSONL the watcher sees, regardless of age"). Every mtime
        // qualifies, including ancient files.
        let now = SystemTime::now();
        assert!(is_in_backfill_window(None, now_minus(0), now));
        assert!(is_in_backfill_window(None, now_minus(60), now));
        assert!(is_in_backfill_window(None, now_minus(86_400 * 365), now));
    }

    #[test]
    fn backfill_window_some_accepts_within_bound() {
        let now = SystemTime::now();
        let one_hour = Some(Duration::from_secs(3600));
        // mtime 30 minutes ago is within a 1-hour window
        assert!(is_in_backfill_window(one_hour, now_minus(1800), now));
    }

    #[test]
    fn backfill_window_some_rejects_outside_bound() {
        let now = SystemTime::now();
        let one_hour = Some(Duration::from_secs(3600));
        // mtime 2 hours ago is outside a 1-hour window
        assert!(!is_in_backfill_window(one_hour, now_minus(7200), now));
    }

    #[test]
    fn backfill_window_handles_clock_skew_gracefully() {
        // mtime in the future (clock skew, file copy preserving mtime
        // from a faster machine, etc.) — `duration_since` errs, we
        // fall back to ZERO, and the file is considered in-window
        // (treated as "newer than now"). Documenting the choice:
        // safest to include rather than silently drop.
        let now = SystemTime::now();
        let future_mtime = now + Duration::from_secs(60);
        assert!(is_in_backfill_window(
            Some(Duration::from_secs(3600)),
            future_mtime,
            now
        ));
    }
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
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    for path in &event.paths {
                        match process_watch_path_raw(path, &mut states) {
                            Ok(batches) => {
                                for (_, events) in batches {
                                    if let Err(e) = emit_events(&events, output_file, stdout) {
                                        eprintln!("Error emitting {}: {}", path.display(), e);
                                    }
                                }
                            }
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
