//! open-story-mcp — MCP server binary, stdio transport.
//!
//! Environment:
//!   OPENSTORY_NATS_URL      NATS endpoint. Default: nats://localhost:4222.
//!   OPENSTORY_DATA_BACKEND  Which store: "sqlite" (default) or "mongo".
//!                           "mongo" requires building with --features mongo.
//!   OPENSTORY_DATA_DIR      SQLite data directory (default ./data).
//!                           Used when DATA_BACKEND=sqlite.
//!   OPENSTORY_MONGO_URI     Mongo connection URI (default
//!                           mongodb://localhost:27017).
//!                           Used when DATA_BACKEND=mongo.
//!   OPENSTORY_MONGO_DB      Mongo database name (default `openstory`).
//!                           Used when DATA_BACKEND=mongo.
//!
//! The binary fails loudly if NATS is unreachable OR the store cannot
//! be opened. There is no in-memory fallback — subscriptions over a
//! bus with no publisher pretend to work and silently deliver nothing,
//! which is worse than erroring out.

use anyhow::{Context, Result};
use open_story_mcp::nats_bus::NatsBus;
use open_story_mcp::server::Server;
use open_story_store::event_store::EventStore;
use open_story_store::plan_store::PlanStore;
use open_story_store::sqlite_store::SqliteStore;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let nats_url = std::env::var("OPENSTORY_NATS_URL")
        .unwrap_or_else(|_| "nats://localhost:4222".to_string());
    let backend = std::env::var("OPENSTORY_DATA_BACKEND")
        .unwrap_or_else(|_| "sqlite".to_string())
        .to_lowercase();
    let data_dir = std::env::var("OPENSTORY_DATA_DIR")
        .unwrap_or_else(|_| "./data".to_string());
    let data_path = PathBuf::from(&data_dir);

    let subscriber = NatsBus::connect(&nats_url).await.with_context(|| {
        format!(
            "open-story-mcp requires NATS at {nats_url} — is the OpenStory server running? \
             (override with OPENSTORY_NATS_URL=…)"
        )
    })?;
    eprintln!("open-story-mcp: connected to NATS at {nats_url}");

    let store: Arc<dyn EventStore> = match backend.as_str() {
        "sqlite" => Arc::new(open_sqlite_store(&data_path, &data_dir)?),
        "mongo" | "mongodb" => open_mongo_store_or_die().await?,
        other => {
            anyhow::bail!(
                "OPENSTORY_DATA_BACKEND={other} is not supported — expected 'sqlite' or 'mongo'"
            );
        }
    };

    // PlanStore is always a file-backed thing under data_dir/plans —
    // it's orthogonal to the EventStore backend.
    let plans_dir = data_path.join("plans");
    std::fs::create_dir_all(&plans_dir).ok();
    let plan_store = Arc::new(
        PlanStore::new(&plans_dir).with_context(|| format!("open PlanStore at {plans_dir:?}"))?,
    );

    let server = Server::new(subscriber, store, plan_store);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    open_story_mcp::stdio::run(stdin, stdout, server).await?;
    Ok(())
}

fn open_sqlite_store(data_path: &Path, data_dir: &str) -> Result<SqliteStore> {
    let store = SqliteStore::new(data_path).with_context(|| {
        format!(
            "open-story-mcp requires the OpenStory data dir at {data_dir} \
             (override with OPENSTORY_DATA_DIR=…)"
        )
    })?;
    eprintln!("open-story-mcp: opened SqliteStore at {data_dir}");
    Ok(store)
}

#[cfg(feature = "mongo")]
async fn open_mongo_store_or_die() -> Result<Arc<dyn EventStore>> {
    let uri = std::env::var("OPENSTORY_MONGO_URI")
        .unwrap_or_else(|_| "mongodb://localhost:27017".to_string());
    let db_name = std::env::var("OPENSTORY_MONGO_DB").unwrap_or_else(|_| "openstory".to_string());
    let store = open_story_store::mongo_store::MongoStore::connect(&uri, &db_name)
        .await
        .with_context(|| {
            format!(
                "open-story-mcp requires Mongo at {uri} (db={db_name}) — \
                 override with OPENSTORY_MONGO_URI / OPENSTORY_MONGO_DB"
            )
        })?;
    eprintln!("open-story-mcp: opened MongoStore at {uri} (db={db_name})");
    Ok(Arc::new(store) as Arc<dyn EventStore>)
}

#[cfg(not(feature = "mongo"))]
async fn open_mongo_store_or_die() -> Result<Arc<dyn EventStore>> {
    anyhow::bail!(
        "OPENSTORY_DATA_BACKEND=mongo selected but this binary was built without the `mongo` \
         feature. Rebuild with: cargo build --release -p open-story-mcp --features mongo"
    )
}
