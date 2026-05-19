//! open-story-mcp — Rust-native MCP server for open-story.
//!
//! Designed for streaming session subscriptions over JSON-RPC 2.0.
//! See `docs/research/streaming-mcp/` for motivation, plan, and test specs.

pub mod nats_bus;
pub mod protocol;
pub mod stdio;
pub mod subscription;
pub mod tokens;
pub mod tools;
