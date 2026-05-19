//! Self-reflective token watcher.
//!
//! A pure aggregator that walks a NATS-derived event payload, extracts
//! usage data from any embedded assistant messages, and maintains a
//! running total. Used by `subscribe_tokens` to stream a running tally
//! over a live session — useful for the agent to watch its own context
//! consumption as it works.

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_create: u64,
}

impl TokenCounts {
    pub fn is_zero(&self) -> bool {
        self.input == 0 && self.output == 0 && self.cache_read == 0 && self.cache_create == 0
    }

    pub fn add(&mut self, other: &Self) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_create += other.cache_create;
    }

    /// Total tokens that contribute to the context window cost
    /// (input + cache_read + cache_create + output). The cost weighting
    /// is intentionally simple — fancier formulas can land later.
    pub fn total(&self) -> u64 {
        self.input + self.cache_read + self.cache_create + self.output
    }
}

#[derive(Debug, Default, Clone)]
pub struct TokenAggregator {
    pub running: TokenCounts,
}

impl TokenAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Pull token usage out of one CloudEvent.
    ///
    /// The expected shape (Claude Code translator output):
    /// `data.raw.message.usage` is an object with
    ///   - `input_tokens`
    ///   - `cache_read_input_tokens`
    ///   - `cache_creation_input_tokens`
    ///   - `output_tokens` *or* `iterations[].output_tokens`
    pub fn extract_one(event: &Value) -> TokenCounts {
        let usage = event
            .get("data")
            .and_then(|d| d.get("raw"))
            .and_then(|r| r.get("message"))
            .and_then(|m| m.get("usage"));
        let Some(usage) = usage else {
            return TokenCounts::default();
        };

        let mut counts = TokenCounts {
            input: usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            cache_read: usage
                .get("cache_read_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            cache_create: usage
                .get("cache_creation_input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0),
            output: 0,
        };

        if let Some(iters) = usage.get("iterations").and_then(|v| v.as_array()) {
            for it in iters {
                counts.output += it.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
            }
        } else {
            counts.output = usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
        }

        counts
    }

    /// Walk a batched payload (`{ events: [...] }`) and sum its usage.
    pub fn extract_batch(batch: &Value) -> TokenCounts {
        let mut total = TokenCounts::default();
        if let Some(events) = batch.get("events").and_then(|v| v.as_array()) {
            for ev in events {
                let one = Self::extract_one(ev);
                total.add(&one);
            }
        }
        total
    }

    /// Observe a batch and, if it contributed tokens, return
    /// `(delta, running_after)`. Returns `None` for zero-token batches
    /// so downstream consumers can skip emission.
    pub fn observe(&mut self, batch: &Value) -> Option<(TokenCounts, TokenCounts)> {
        let delta = Self::extract_batch(batch);
        if delta.is_zero() {
            return None;
        }
        self.running.add(&delta);
        Some((delta, self.running))
    }
}
