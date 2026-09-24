//! Consumer supervision (REQUIREMENTS E-02, E-03).
//!
//! Every consumer actor is a loop over a bus subscription. Today the loop
//! ends silently when the subscription closes. `Driven` wraps the receiver:
//! the loop asks it for the next batch, and when the channel ends it logs
//! `event=consumer_ended` at ERROR with the actor and the reason, and
//! `finish` returns that as an error the supervisor (E-03) acts on. A
//! consumer never disappears without a line.

use open_story_bus::IngestBatch;
use tokio::sync::mpsc;

/// Why a consumer loop returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConsumerExit {
    /// The bus subscription's channel closed; `batches` were handled first.
    SubscriptionClosed { batches: u64 },
    /// The bus refused the subscription.
    SubscribeFailed { error: String },
}

impl std::fmt::Display for ConsumerExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsumerExit::SubscriptionClosed { batches } => {
                write!(f, "subscription closed after {batches} batches")
            }
            ConsumerExit::SubscribeFailed { error } => write!(f, "subscribe failed: {error}"),
        }
    }
}

impl std::error::Error for ConsumerExit {}

/// A consumer's subscription, driven batch by batch with the ending named.
pub struct Driven {
    actor: &'static str,
    rx: mpsc::Receiver<IngestBatch>,
    batches: u64,
    ended: bool,
}

impl Driven {
    pub fn new(actor: &'static str, rx: mpsc::Receiver<IngestBatch>) -> Self {
        Driven {
            actor,
            rx,
            batches: 0,
            ended: false,
        }
    }

    /// The next batch, or `None` once the subscription has ended. The
    /// ending is logged exactly once, at ERROR, with actor, reason, and the
    /// number of batches handled before it.
    pub async fn next(&mut self) -> Option<IngestBatch> {
        match self.rx.recv().await {
            Some(batch) => {
                self.batches += 1;
                // H-05: what is still queued behind this batch.
                let lag = self.rx.len() as u64;
                stats().update(self.actor, |h| h.lag = lag);
                Some(batch)
            }
            None => {
                if !self.ended {
                    self.ended = true;
                    let actor = self.actor;
                    let batches = self.batches;
                    tracing::error!(
                        event = "consumer_ended",
                        actor,
                        reason = "subscription closed",
                        batches,
                        "consumer {actor} ended: subscription closed after {batches} batches"
                    );
                }
                None
            }
        }
    }

    /// How the loop ended: an error when the subscription closed, so the
    /// caller cannot mistake a dead consumer for a finished one.
    pub fn finish(self) -> Result<(), ConsumerExit> {
        if self.ended {
            Err(ConsumerExit::SubscriptionClosed {
                batches: self.batches,
            })
        } else {
            Ok(())
        }
    }
}

// ── Supervisor (E-03) ───────────────────────────────────────────────────────

/// Exponential backoff before restart attempt `attempt` (1-based):
/// 1 s, 2 s, 4 s, … capped at 30 s.
pub fn backoff(attempt: u32) -> std::time::Duration {
    let secs = 1u64 << attempt.saturating_sub(1).min(5);
    std::time::Duration::from_secs(secs.min(30))
}

/// What health reports per supervised consumer (E-04, H-05).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub struct ConsumerHealth {
    pub alive: bool,
    pub restarts: u32,
    pub last_restart: Option<String>,
    pub last_exit: Option<String>,
    /// Batches waiting in the consumer's channel at its last receive (H-05).
    pub lag: u64,
}

/// Process-wide restart bookkeeping, read by `/api/health`.
#[derive(Default)]
pub struct SupervisorStats {
    inner: std::sync::Mutex<std::collections::HashMap<&'static str, ConsumerHealth>>,
}

impl SupervisorStats {
    fn update(&self, actor: &'static str, f: impl FnOnce(&mut ConsumerHealth)) {
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(g.entry(actor).or_default());
    }

    pub fn snapshot(&self) -> std::collections::HashMap<&'static str, ConsumerHealth> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

pub fn stats() -> &'static SupervisorStats {
    static STATS: std::sync::OnceLock<SupervisorStats> = std::sync::OnceLock::new();
    STATS.get_or_init(SupervisorStats::default)
}

/// A boxed run of one consumer attempt.
pub type ConsumerRun =
    std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ConsumerExit>> + Send>>;

/// Own a consumer: run `start` and, whenever the run returns an error,
/// log `event=consumer_restarted` at WARN with the attempt, the backoff,
/// and the reason, wait (via `sleep`, injectable for tests), and run it
/// again. A run that returns `Ok(())` ended on purpose and is not
/// restarted. Restart counts and timestamps land in `stats()`.
pub async fn supervise<F, S, SF>(actor: &'static str, mut start: F, sleep: S)
where
    F: FnMut() -> ConsumerRun + Send,
    S: Fn(std::time::Duration) -> SF + Send,
    SF: std::future::Future<Output = ()> + Send,
{
    let mut attempt: u32 = 0;
    loop {
        stats().update(actor, |h| h.alive = true);
        let result = start().await;
        stats().update(actor, |h| h.alive = false);
        match result {
            Ok(()) => {
                tracing::info!(
                    event = "consumer_finished",
                    actor,
                    "consumer {actor} finished"
                );
                return;
            }
            Err(exit) => {
                attempt += 1;
                let delay = backoff(attempt);
                let reason = exit.to_string();
                tracing::warn!(
                    event = "consumer_restarted",
                    actor,
                    attempt,
                    backoff_ms = delay.as_millis() as u64,
                    reason = %reason,
                    "consumer {actor} died ({reason}); restart {attempt} in {}s",
                    delay.as_secs()
                );
                stats().update(actor, |h| {
                    h.restarts = attempt;
                    h.last_restart = Some(chrono::Utc::now().to_rfc3339());
                    h.last_exit = Some(reason.clone());
                });
                metrics::counter!("openstory_consumer_restarts_total", "actor" => actor)
                    .increment(1);
                sleep(delay).await;
            }
        }
    }
}
