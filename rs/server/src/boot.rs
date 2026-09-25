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

/// The serving flag as a watch channel, so a task can wait for the phase
/// to flip without polling (B-05: consumers hold until serving). Starts
/// true because the default phase is serving.
fn serving_tx() -> &'static tokio::sync::watch::Sender<bool> {
    static TX: std::sync::OnceLock<tokio::sync::watch::Sender<bool>> = std::sync::OnceLock::new();
    TX.get_or_init(|| tokio::sync::watch::channel(true).0)
}

fn update(f: impl FnOnce(&mut BootState)) {
    let mut g = STATE.lock().unwrap_or_else(|e| e.into_inner());
    f(&mut g);
    serving_tx().send_replace(g.phase == Phase::Serving);
}

/// Resolves once the node serves; at once when it already does.
pub async fn wait_for_serving() {
    let mut rx = serving_tx().subscribe();
    // The sender is static and never dropped, so this cannot fail; if it
    // ever did, holding a consumer forever would be the wrong answer.
    let _ = rx.wait_for(|serving| *serving).await;
}

/// When the consumer actors may subscribe (B-05): at `serving` (the
/// default — replay finishes with the store's heap alone, then the
/// actors drain their backlog) or at `boot` (today's behaviour, both at
/// once). Config `consumers_start`, env `OPEN_STORY_CONSUMERS_START`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumersStart {
    Serving,
    Boot,
}

impl std::str::FromStr for ConsumersStart {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "serving" => Ok(ConsumersStart::Serving),
            "boot" => Ok(ConsumersStart::Boot),
            other => Err(format!(
                "consumers_start must be \"serving\" or \"boot\", got \"{other}\""
            )),
        }
    }
}

/// The future a supervisor awaits before its first run: ready now for
/// `Boot`, the serving flip for `Serving`.
pub fn consumer_gate(
    mode: ConsumersStart,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
    match mode {
        ConsumersStart::Boot => Box::pin(std::future::ready(())),
        ConsumersStart::Serving => Box::pin(wait_for_serving()),
    }
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
