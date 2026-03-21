//! open-story-server: HTTP/WS server for Open Story.
//!
//! API endpoints, WebSocket broadcast, hook receiver, ingest pipeline.
//! This crate contains all server logic; the binary crate (`open-story-cli`)
//! calls `run_server()` from the parent `open-story` crate which wires
//! this together with the file watcher.

pub mod api;
pub mod auth;
pub mod broadcast;
pub mod config;
pub mod event_store_bridge;
pub mod hooks;
pub mod ingest;
pub mod logging;
pub mod metrics;
pub mod router;
pub mod state;
pub mod tool_schemas;
pub mod transcript;
pub mod ws;
