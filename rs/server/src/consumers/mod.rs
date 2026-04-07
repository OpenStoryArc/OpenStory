//! Independent actor-consumers for the decomposed ingest pipeline.
//!
//! Each consumer subscribes to NATS subjects and owns its own state.
//! They communicate through message-passing, not shared mutable state.
//!
//! Architecture (from CLAUDE.md):
//! > "The system is a network of independent actors communicating through messages.
//! > Each actor has a single responsibility and its own lifecycle."

pub mod broadcast;
pub mod patterns;
pub mod persist;
pub mod projections;
