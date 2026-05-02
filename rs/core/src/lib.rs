//! Core types and utilities for open-story.
//!
//! This crate provides the foundational types shared across all open-story crates:
//! - `CloudEvent` — the universal event envelope (CloudEvents 1.0)
//! - `translate` — JSON transcript lines → CloudEvent conversion
//! - `reader` — incremental file reader for transcript files
//! - `output` — CloudEvent → JSONL file writer
//! - `paths` — path utilities for session/project ID extraction
//! - `host` — per-process host identity resolver for event origin stamping
//! - `user` — per-process user identity resolver (parallel to host; identifies the human, not the machine)

pub mod cloud_event;
pub mod event_data;
pub mod host;
pub mod output;
pub mod paths;
pub mod reader;
pub mod strings;
pub mod subtype;
pub mod translate;
pub mod translate_hermes;
pub mod translate_pi;
pub mod user;
