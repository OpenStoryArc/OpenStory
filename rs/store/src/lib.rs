//! open-story-store: Event store — bus consumer → dedup → persist → project → detect patterns.
//!
//! This crate owns the write path and state management:
//! - Persistence (SessionStore, EventLog) — JSONL append-only storage
//! - Projections (SessionProjection) — incremental materialized views
//! - Plan extraction (PlanStore) — plan detection and storage
//! - Analysis (session summaries, tool analytics)
//! - Ingest pipeline (dedup → persist → project → detect → broadcast)
//!
//! The store subscribes to the Bus and processes every event batch.
//! The server crate reads from the store to serve API requests.

pub mod analysis;
pub mod event_store;
pub mod extract;
pub mod ingest;
pub mod jsonl_store;
#[cfg(feature = "mongo")]
pub mod mongo_store;
pub mod persistence;
pub mod plan_store;
pub mod projection;
pub mod queries;
pub mod sqlite_store;
pub mod state;
