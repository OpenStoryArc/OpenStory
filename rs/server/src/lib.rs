//! open-story-server: HTTP/WS server for Open Story.
//!
//! API endpoints, WebSocket broadcast, hook receiver, ingest pipeline.
//! This crate contains all server logic; the binary crate (`open-story-cli`)
//! calls `run_server()` from the parent `open-story` crate which wires
//! this together with the file watcher.

pub mod account_config;
pub mod admin;
pub mod annotations;
pub mod api;
pub mod auth;
pub mod broadcast;
pub mod catch_up;
pub mod config;
pub mod consumers;
pub mod directory;
pub mod event_store_bridge;
pub mod fleet;
pub mod ingest;
pub mod logging;
pub mod metrics;
pub mod principal_resolver;
pub mod reconcile;
pub mod reproject;
pub mod router;
pub mod state;
pub mod tool_schemas;
pub mod transcript;
pub mod watcher_diagnostics;
pub mod ws;
