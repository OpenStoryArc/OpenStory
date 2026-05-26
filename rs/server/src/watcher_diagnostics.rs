use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use serde::Serialize;

const RECENT_LIMIT: usize = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WatcherActorConfig {
    pub agent: String,
    pub protocol: WatcherProtocol,
    pub root: PathBuf,
    pub recursive_mode: String,
}

impl WatcherActorConfig {
    pub fn new(agent: impl Into<String>, protocol: WatcherProtocol, root: PathBuf) -> Self {
        Self {
            agent: agent.into(),
            protocol,
            root,
            recursive_mode: "recursive".to_string(),
        }
    }

    pub fn actor_id(&self) -> String {
        let canonical = canonicalize_path(&self.root);
        format!("{}:{}", self.agent, canonical.to_string_lossy())
    }
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum WatcherProtocol {
    AppendJsonl,
    Snapshot,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct WatcherBackfillSnapshot {
    pub window_hours: Option<u64>,
    pub files_seen: u64,
    pub files_loaded: u64,
    pub events_emitted: u64,
    pub files_skipped: u64,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq, Eq)]
pub struct WatcherCountersSnapshot {
    pub raw_notify_events: u64,
    pub ignored_notify_events: u64,
    pub files_processed: u64,
    pub zero_new_byte_reads: u64,
    pub cloud_events_emitted: u64,
    pub publish_failures: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WatcherObservation {
    pub at: String,
    pub phase: String,
    pub path: Option<String>,
    pub detail: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct WatcherSnapshot {
    pub actor: String,
    pub agent: String,
    pub protocol: WatcherProtocol,
    pub root: String,
    pub canonical_root: String,
    pub recursive_mode: String,
    pub started_at: String,
    pub backfill: WatcherBackfillSnapshot,
    pub counters: WatcherCountersSnapshot,
    pub last_event_at: Option<String>,
    pub last_processed_path: Option<String>,
    pub recent: Vec<WatcherObservation>,
}

#[derive(Clone, Debug)]
pub struct FileProcessObservation {
    pub path: PathBuf,
    pub canonical_path: PathBuf,
    pub byte_offset_before: u64,
    pub byte_offset_after: u64,
    pub line_count_before: u64,
    pub line_count_after: u64,
    pub format: String,
    pub events_emitted: usize,
    pub subtypes: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct PublishObservation {
    pub subject: String,
    pub session_id: String,
    pub event_count: usize,
    pub first_subtype: Option<String>,
    pub last_subtype: Option<String>,
    pub success: bool,
    pub latency_ms: u128,
}

#[derive(Clone, Default)]
pub struct WatcherDiagnostics {
    inner: Arc<Mutex<HashMap<String, WatcherState>>>,
}

#[derive(Clone, Debug)]
struct WatcherState {
    actor: String,
    agent: String,
    protocol: WatcherProtocol,
    root: PathBuf,
    canonical_root: PathBuf,
    recursive_mode: String,
    started_at: String,
    backfill: WatcherBackfillSnapshot,
    counters: WatcherCountersSnapshot,
    last_event_at: Option<String>,
    last_processed_path: Option<PathBuf>,
    recent: VecDeque<WatcherObservation>,
}

impl WatcherDiagnostics {
    pub fn register_actor(&self, config: &WatcherActorConfig) -> String {
        let actor = config.actor_id();
        let canonical_root = canonicalize_path(&config.root);
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        inner.entry(actor.clone()).or_insert_with(|| WatcherState {
            actor: actor.clone(),
            agent: config.agent.clone(),
            protocol: config.protocol.clone(),
            root: config.root.clone(),
            canonical_root,
            recursive_mode: config.recursive_mode.clone(),
            started_at: now_rfc3339(),
            backfill: WatcherBackfillSnapshot::default(),
            counters: WatcherCountersSnapshot::default(),
            last_event_at: None,
            last_processed_path: None,
            recent: VecDeque::new(),
        });
        actor
    }

    pub fn record_backfill(&self, actor: &str, snapshot: WatcherBackfillSnapshot) {
        self.with_actor(actor, |state| {
            state.backfill = snapshot.clone();
            push_recent(
                state,
                "backfill",
                None,
                format!(
                    "files_seen={} files_loaded={} events_emitted={} files_skipped={}",
                    snapshot.files_seen,
                    snapshot.files_loaded,
                    snapshot.events_emitted,
                    snapshot.files_skipped
                ),
            );
        });
    }

    pub fn record_notify(
        &self,
        actor: &str,
        kind: impl Into<String>,
        raw_paths: &[PathBuf],
        accepted: bool,
        ignored_reason: Option<&str>,
    ) {
        self.with_actor(actor, |state| {
            state.counters.raw_notify_events = state.counters.raw_notify_events.saturating_add(1);
            if !accepted {
                state.counters.ignored_notify_events =
                    state.counters.ignored_notify_events.saturating_add(1);
            }
            state.last_event_at = Some(now_rfc3339());
            let canonical_paths = raw_paths
                .iter()
                .map(|path| canonicalize_path(path).to_string_lossy().to_string())
                .collect::<Vec<_>>();
            push_recent(
                state,
                "notify",
                raw_paths.first().map(PathBuf::as_path),
                format!(
                    "kind={} accepted={} ignored_reason={} canonical_paths={:?}",
                    kind.into(),
                    accepted,
                    ignored_reason.unwrap_or(""),
                    canonical_paths
                ),
            );
        });
    }

    pub fn record_file(&self, actor: &str, observation: FileProcessObservation) {
        self.with_actor(actor, |state| {
            state.counters.files_processed = state.counters.files_processed.saturating_add(1);
            if observation.byte_offset_after == observation.byte_offset_before {
                state.counters.zero_new_byte_reads =
                    state.counters.zero_new_byte_reads.saturating_add(1);
            }
            state.counters.cloud_events_emitted = state
                .counters
                .cloud_events_emitted
                .saturating_add(observation.events_emitted as u64);
            state.last_processed_path = Some(observation.canonical_path.clone());
            push_recent(
                state,
                "reader",
                Some(observation.path.as_path()),
                format!(
                    "offset={}..{} lines={}..{} format={} emitted={} subtypes={:?}",
                    observation.byte_offset_before,
                    observation.byte_offset_after,
                    observation.line_count_before,
                    observation.line_count_after,
                    observation.format,
                    observation.events_emitted,
                    observation.subtypes
                ),
            );
        });
    }

    pub fn record_publish(&self, actor: &str, observation: PublishObservation) {
        self.with_actor(actor, |state| {
            if !observation.success {
                state.counters.publish_failures =
                    state.counters.publish_failures.saturating_add(1);
            }
            push_recent(
                state,
                "publish",
                None,
                format!(
                    "subject={} session_id={} events={} first={:?} last={:?} success={} latency_ms={}",
                    observation.subject,
                    observation.session_id,
                    observation.event_count,
                    observation.first_subtype,
                    observation.last_subtype,
                    observation.success,
                    observation.latency_ms
                ),
            );
        });
    }

    pub fn snapshots(&self) -> Vec<WatcherSnapshot> {
        let inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut snapshots = inner
            .values()
            .map(WatcherState::snapshot)
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| a.actor.cmp(&b.actor));
        snapshots
    }

    fn with_actor(&self, actor: &str, f: impl FnOnce(&mut WatcherState)) {
        let mut inner = self
            .inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(state) = inner.get_mut(actor) {
            f(state);
        }
    }
}

impl WatcherState {
    fn snapshot(&self) -> WatcherSnapshot {
        WatcherSnapshot {
            actor: self.actor.clone(),
            agent: self.agent.clone(),
            protocol: self.protocol.clone(),
            root: self.root.to_string_lossy().to_string(),
            canonical_root: self.canonical_root.to_string_lossy().to_string(),
            recursive_mode: self.recursive_mode.clone(),
            started_at: self.started_at.clone(),
            backfill: self.backfill.clone(),
            counters: self.counters.clone(),
            last_event_at: self.last_event_at.clone(),
            last_processed_path: self
                .last_processed_path
                .as_ref()
                .map(|path| path.to_string_lossy().to_string()),
            recent: self.recent.iter().cloned().collect(),
        }
    }
}

fn push_recent(
    state: &mut WatcherState,
    phase: impl Into<String>,
    path: Option<&Path>,
    detail: impl Into<String>,
) {
    if state.recent.len() >= RECENT_LIMIT {
        state.recent.pop_front();
    }
    state.recent.push_back(WatcherObservation {
        at: now_rfc3339(),
        phase: phase.into(),
        path: path.map(|path| path.to_string_lossy().to_string()),
        detail: detail.into(),
    });
}

pub fn canonicalize_path(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_records_actor_and_recent_observations() {
        let diagnostics = WatcherDiagnostics::default();
        let actor = diagnostics.register_actor(&WatcherActorConfig::new(
            "codex",
            WatcherProtocol::AppendJsonl,
            PathBuf::from("/tmp/codex"),
        ));

        diagnostics.record_notify(
            &actor,
            "Modify(Data)",
            &[PathBuf::from("/tmp/codex/session.jsonl")],
            true,
            None,
        );
        diagnostics.record_file(
            &actor,
            FileProcessObservation {
                path: PathBuf::from("/tmp/codex/session.jsonl"),
                canonical_path: PathBuf::from("/tmp/codex/session.jsonl"),
                byte_offset_before: 10,
                byte_offset_after: 20,
                line_count_before: 1,
                line_count_after: 2,
                format: "codex".to_string(),
                events_emitted: 1,
                subtypes: vec!["message.user.prompt".to_string()],
            },
        );

        let snapshots = diagnostics.snapshots();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].agent, "codex");
        assert_eq!(snapshots[0].counters.raw_notify_events, 1);
        assert_eq!(snapshots[0].counters.files_processed, 1);
        assert_eq!(snapshots[0].counters.cloud_events_emitted, 1);
        assert_eq!(snapshots[0].recent.len(), 2);
    }
}
