//! open-story-mcp — MCP server binary, stdio transport.
//!
//! The query tools read through the OpenStory REST API; the streaming
//! tools (`subscribe_session`, `subscribe_tokens`) subscribe over NATS.
//! Neither path opens the SQLite store directly, so nothing here is
//! resolved relative to the process's working directory — launch from
//! anywhere.
//!
//! Environment:
//!   OPENSTORY_API_URL    REST server origin. Default: http://localhost:3002.
//!                        This is where every query tool reads from.
//!   OPENSTORY_API_TOKEN  Bearer token, if the server has `api_token` set.
//!                        Default: none (no Authorization header).
//!   OPENSTORY_NATS_URL   NATS endpoint for the streaming tools.
//!                        Default: nats://localhost:4222.
//!
//! The binary fails loudly if NATS is unreachable — subscriptions over a
//! bus with no publisher pretend to work and silently deliver nothing,
//! which is worse than erroring out. Query tools degrade per-call: a
//! request that can't reach the API returns an empty/`isError` result
//! with the cause, rather than killing the process.

use anyhow::{Context, Result};
use open_story_mcp::http_store::HttpEventStore;
use open_story_mcp::nats_bus::NatsBus;
use open_story_mcp::plan_source::{HttpPlanSource, PlanSource};
use open_story_mcp::server::Server;
use open_story_store::event_store::EventStore;
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let api_url = std::env::var("OPENSTORY_API_URL")
        .unwrap_or_else(|_| "http://localhost:3002".to_string());
    let api_token = std::env::var("OPENSTORY_API_TOKEN").ok().filter(|t| !t.is_empty());
    let nats_url = std::env::var("OPENSTORY_NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());

    let subscriber = NatsBus::connect(&nats_url).await.with_context(|| {
        format!(
            "open-story-mcp requires NATS at {nats_url} for streaming tools — \
             is the OpenStory server running? (override with OPENSTORY_NATS_URL=…)"
        )
    })?;
    eprintln!("open-story-mcp: connected to NATS at {nats_url}");

    let store: Arc<dyn EventStore> =
        Arc::new(HttpEventStore::new(&api_url, api_token.clone()));
    let plan_store: Arc<dyn PlanSource> =
        Arc::new(HttpPlanSource::new(&api_url, api_token));
    eprintln!(
        "open-story-mcp: query tools read REST API at {api_url}{}",
        if std::env::var("OPENSTORY_API_TOKEN").map(|t| !t.is_empty()).unwrap_or(false) {
            " (bearer auth)"
        } else {
            ""
        }
    );

    let server = Server::new(subscriber, store, plan_store);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    open_story_mcp::stdio::run(stdin, stdout, server).await?;
    Ok(())
}
