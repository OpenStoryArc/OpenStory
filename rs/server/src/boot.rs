//! Boot phase (REQUIREMENTS H-02, H-03).
//!
//! A node starts, replays its store into projections, then serves. Until
//! it serves, `/api/health` answers 503 so a readiness probe, a deploy
//! gate, or an agent can tell "replaying" from "hung"; `/health` stays
//! 200 as long as the process is up. The phase is process-wide, like the
//! log ring and the supervisor stats. Default is `serving`: a node that
//! never replays (publisher role, tests) is ready at once.

use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Starting,
    Replaying,
    Serving,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct Replay {
    pub done: usize,
    pub total: usize,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct BootState {
    pub phase: Phase,
    pub replay: Replay,
}

static STATE: Mutex<BootState> = Mutex::new(BootState {
    phase: Phase::Serving,
    replay: Replay {
        done: 0,
        total: 0,
        elapsed_ms: 0,
    },
});

fn update(f: impl FnOnce(&mut BootState)) {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g);
}

/// The process has started and has not begun replay yet.
pub fn set_starting() {
    update(|s| s.phase = Phase::Starting);
}

/// Replay in progress: `done` of `total` sessions after `elapsed_ms`.
pub fn set_replaying(done: usize, total: usize, elapsed_ms: u64) {
    update(|s| {
        s.phase = Phase::Replaying;
        s.replay = Replay {
            done,
            total,
            elapsed_ms,
        };
    });
}

/// Replay finished (or never needed): the node serves. `done` is set to
/// `total` so a reader sees a complete bar.
pub fn set_serving() {
    update(|s| {
        s.phase = Phase::Serving;
        s.replay.done = s.replay.total;
    });
}

pub fn snapshot() -> BootState {
    *STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// True once the node is serving: what readiness means.
pub fn is_serving() -> bool {
    snapshot().phase == Phase::Serving
}
