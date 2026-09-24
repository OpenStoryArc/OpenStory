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
}

impl std::fmt::Display for ConsumerExit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsumerExit::SubscriptionClosed { batches } => {
                write!(f, "subscription closed after {batches} batches")
            }
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
