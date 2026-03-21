//! Core types and utilities for open-story.
//!
//! This crate provides the foundational types shared across all open-story crates:
//! - `CloudEvent` — the universal event envelope (CloudEvents 1.0)
//! - `translate` — JSON transcript lines → CloudEvent conversion
//! - `reader` — incremental file reader for transcript files
//! - `output` — CloudEvent → JSONL file writer
//! - `paths` — path utilities for session/project ID extraction

pub mod cloud_event;
pub mod output;
pub mod paths;
pub mod reader;
pub mod translate;
