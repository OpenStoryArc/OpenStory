//! open-story-mcp — MCP server binary, stdio transport.
//!
//! Environment:
//!   OPENSTORY_NATS_URL — NATS endpoint. Default: nats://localhost:4222.
//!   OPENSTORY_DATA_DIR — Data directory holding open-story.db.
//!                        Default: ./data (matches the server's default).
//!
//! The binary fails loudly if NATS is unreachable OR the data dir
//! cannot be opened. There is no in-memory fallback — subscriptions
//! over a bus with no publisher pretend to work and silently deliver
//! nothing, which is worse than erroring out.

use anyhow::{Context, Result};
use open_story_mcp::nats_bus::NatsBus;
use open_story_mcp::server::Server;
use open_story_store::event_store::EventStore;
use open_story_store::plan_store::PlanStore;
use open_story_store::sqlite_store::SqliteStore;
use std::path::PathBuf;
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let url = std::env::var("OPENSTORY_NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let data_dir = std::env::var("OPENSTORY_DATA_DIR")
        .unwrap_or_else(|_| "./data".to_string());
    let data_path = PathBuf::from(&data_dir);

    let subscriber = NatsBus::connect(&url).await.with_context(|| {
        format!(
            "open-story-mcp requires NATS at {url} — is the OpenStory server running? \
             (override with OPENSTORY_NATS_URL=…)"
        )
    })?;
    eprintln!("open-story-mcp: connected to NATS at {url}");

    let store: Arc<dyn EventStore> = Arc::new(
        SqliteStore::new(&data_path).with_context(|| {
            format!(
                "open-story-mcp requires the OpenStory data dir at {data_dir} \
                 (override with OPENSTORY_DATA_DIR=…)"
            )
        })?,
    );
    eprintln!("open-story-mcp: opened store at {data_dir}");

    let plans_dir = data_path.join("plans");
    std::fs::create_dir_all(&plans_dir).ok(); // idempotent — server may have created it already
    let plan_store = Arc::new(
        PlanStore::new(&plans_dir).with_context(|| format!("open PlanStore at {plans_dir:?}"))?,
    );

    let server = Server::new(subscriber, store, plan_store);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    open_story_mcp::stdio::run(stdin, stdout, server).await?;
    Ok(())
}
