//! open-story-mcp — Rust-native MCP server for open-story.
//!
//! Designed for streaming session subscriptions over JSON-RPC 2.0.
//! See `docs/research/streaming-mcp/` for motivation, plan, and test specs.

pub mod bus;
pub mod protocol;
pub mod stdio;
pub mod tools;
