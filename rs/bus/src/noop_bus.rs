//! NoopBus — a do-nothing Bus implementation for testing.
//!
//! Publish silently succeeds. Subscribe returns a channel that never receives.
//! Replay returns an empty list. Useful for unit tests that don't need bus behavior.

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::{Bus, BusSubscription, IngestBatch};

/// A no-op Bus implementation for testing.
pub struct NoopBus;

#[async_trait]
impl Bus for NoopBus {
    async fn publish(&self, _subject: &str, _batch: &IngestBatch) -> Result<()> {
        Ok(())
    }

    async fn publish_bytes(&self, _subject: &str, _data: &[u8]) -> Result<()> {
        Ok(())
    }

    async fn subscribe(&self, _pattern: &str) -> Result<BusSubscription> {
        let (_tx, rx) = mpsc::channel(1);
        Ok(BusSubscription { receiver: rx })
    }

    async fn replay(&self, _pattern: &str) -> Result<Vec<IngestBatch>> {
        Ok(vec![])
    }

    fn is_active(&self) -> bool {
        false
    }
}
