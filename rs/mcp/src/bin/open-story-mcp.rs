//! open-story-mcp — MCP server binary, stdio transport.
//!
//! Environment:
//!   OPENSTORY_NATS_URL — NATS endpoint. Default: nats://localhost:4222.
//!
//! The binary fails loudly if NATS is unreachable. There is no
//! in-memory fallback — subscriptions over a bus with no publisher
//! pretend to work and silently deliver nothing, which is worse than
//! erroring out.

use anyhow::{Context, Result};
use open_story_mcp::nats_bus::NatsBus;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let url = std::env::var("OPENSTORY_NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let subscriber = NatsBus::connect(&url).await.with_context(|| {
        format!(
            "open-story-mcp requires NATS at {url} — is the OpenStory server running? \
             (override with OPENSTORY_NATS_URL=…)"
        )
    })?;
    eprintln!("open-story-mcp: connected to NATS at {url}");

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    open_story_mcp::stdio::run(stdin, stdout, subscriber).await?;
    Ok(())
}
