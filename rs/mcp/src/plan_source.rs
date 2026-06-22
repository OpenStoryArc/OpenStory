//! `PlanSource` — the seam for `session_plans`, the one tool that reads
//! plans rather than the `EventStore`.
//!
//! Plans live outside SQLite (file-backed `PlanStore`), so they need
//! their own abstraction to follow the same "read through the server"
//! migration as the query tools. Two implementations:
//!
//! - `PlanStore` (file-backed) — used by tests and the legacy local path.
//!   `Arc<PlanStore>` coerces to `Arc<dyn PlanSource>` at every existing
//!   `Server::new` call site, so no test churn.
//! - `HttpPlanSource` — reads `GET /api/sessions/{id}/plans`. This is
//!   strictly *more* complete than the local file read: the server folds
//!   in subagent child-session plans (`session_children`) that a bare
//!   `PlanStore` can't see.

use async_trait::async_trait;
use open_story_store::plan_store::{PlanMeta, PlanStore};

/// Source of `/plan` documents for a session, newest-or-any order
/// (the `session_plans` tool re-sorts).
#[async_trait]
pub trait PlanSource: Send + Sync {
    async fn list_for_session(&self, session_id: &str) -> Vec<PlanMeta>;
}

/// File-backed: delegates to the synchronous `PlanStore`.
#[async_trait]
impl PlanSource for PlanStore {
    async fn list_for_session(&self, session_id: &str) -> Vec<PlanMeta> {
        PlanStore::list_for_session(self, session_id)
    }
}

/// REST-backed plan source. Mirrors `HttpEventStore` but for the plans
/// endpoint, which `PlanStore` doesn't model on the `EventStore` trait.
#[derive(Clone)]
pub struct HttpPlanSource {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpPlanSource {
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        let base_url = base_url.into().trim_end_matches('/').to_string();
        Self {
            base_url,
            token,
            client: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlanSource for HttpPlanSource {
    async fn list_for_session(&self, session_id: &str) -> Vec<PlanMeta> {
        let url = format!("{}/api/sessions/{session_id}/plans", self.base_url);
        let mut req = self.client.get(&url);
        if let Some(tok) = &self.token {
            req = req.bearer_auth(tok);
        }
        match req.send().await.and_then(|r| r.error_for_status()) {
            Ok(resp) => resp.json::<Vec<PlanMeta>>().await.unwrap_or_else(|e| {
                eprintln!("open-story-mcp: GET {url}: decode: {e} — no plans");
                Vec::new()
            }),
            Err(e) => {
                eprintln!("open-story-mcp: GET {url}: {e} — no plans");
                Vec::new()
            }
        }
    }
}
