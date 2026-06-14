//! `HttpEventStore` — an `EventStore` that reads through the OpenStory
//! REST API instead of opening SQLite directly.
//!
//! ## Why this exists
//!
//! The MCP query tools call `EventStore::query_*` / `session_events` /
//! `session_patterns` and serialize the typed result. The REST server
//! (`open-story-server`) wraps the *same* `open_story_store::queries`
//! types over HTTP. So we can satisfy the trait by issuing the matching
//! HTTP request and deserializing back into the shared struct — every
//! tool, the dispatcher, and stdio stay untouched.
//!
//! The payoff is sovereignty-of-launch: the binary needs only a URL
//! (`OPENSTORY_API_URL`), not a data directory resolved relative to the
//! process's working directory. Launching the MCP from `~` no longer
//! opens an empty `./data/open-story.db` and reports an empty index.
//!
//! ## Endpoint map (verified against rs/server/src/api.rs)
//!
//! | Trait method                | REST route                                   | Shape         |
//! |-----------------------------|----------------------------------------------|---------------|
//! | `list_sessions`             | `GET /api/sessions`                          | `{sessions:[…]}` (trimmed) |
//! | `session_events`            | `GET /api/sessions/{id}/events`              | bare array    |
//! | `session_patterns`          | `GET /api/sessions/{id}/patterns?type=`      | `{patterns:[…]}` |
//! | `query_session_synopsis`    | `GET /api/sessions/{id}/synopsis`            | bare struct / null |
//! | `query_tool_journey`        | `GET /api/sessions/{id}/tool-journey`        | bare array    |
//! | `query_file_impact`         | `GET /api/sessions/{id}/file-impact`         | bare array    |
//! | `query_session_errors`      | `GET /api/sessions/{id}/errors`              | bare array    |
//! | `query_project_pulse`       | `GET /api/insights/pulse?days=`              | bare array    |
//! | `query_project_context`     | `GET /api/agent/project-context?project=`    | bare array    |
//! | `query_recent_files`        | `GET /api/agent/recent-files?project=`       | bare array    |
//! | `query_productivity_by_hour`| `GET /api/insights/productivity?days=`       | bare array    |
//! | `query_token_usage`         | `GET /api/insights/token-usage?…`            | bare struct   |
//! | `query_daily_token_usage`   | `GET /api/insights/token-usage/daily?days=`  | bare array    |
//! | `search_fts`                | `GET /api/search?q=&limit=&session_id=`      | bare array    |
//!
//! Methods the MCP never calls (writes, lifecycle, turns, full_payload)
//! are implemented as read-only no-ops or clear errors — see the bottom
//! of the impl block.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::queries;

/// A read-only `EventStore` backed by the OpenStory REST API.
#[derive(Clone)]
pub struct HttpEventStore {
    base_url: String,
    token: Option<String>,
    client: reqwest::Client,
}

impl HttpEventStore {
    /// `base_url` is the server origin, e.g. `http://localhost:3002`.
    /// `token`, when present, is sent as `Authorization: Bearer <token>`.
    pub fn new(base_url: impl Into<String>, token: Option<String>) -> Self {
        let base_url = base_url.into();
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            token,
            client: reqwest::Client::new(),
        }
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    /// Issue a GET and deserialize the JSON body into `T`. Carries the
    /// bearer token when configured. Errors carry the path for context.
    async fn get<T: for<'de> Deserialize<'de>>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> Result<T> {
        let mut req = self.client.get(self.url(path));
        if !query.is_empty() {
            req = req.query(query);
        }
        if let Some(tok) = &self.token {
            req = req.bearer_auth(tok);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| anyhow!("GET {path}: {e}"))?
            .error_for_status()
            .map_err(|e| anyhow!("GET {path}: {e}"))?;
        resp.json::<T>()
            .await
            .map_err(|e| anyhow!("GET {path}: decode: {e}"))
    }

    /// Like `get`, but a non-2xx or transport error becomes the method's
    /// default value rather than propagating — matches the trait's
    /// infallible `query_*` signatures (they return `Vec`/`Option`/struct,
    /// never `Result`). Failures are logged to stderr for diagnosis.
    async fn get_or_default<T: for<'de> Deserialize<'de> + Default>(
        &self,
        path: &str,
        query: &[(&str, String)],
    ) -> T {
        match self.get::<T>(path, query).await {
            Ok(v) => v,
            Err(e) => {
                eprintln!("open-story-mcp: {e} — returning empty");
                T::default()
            }
        }
    }
}

/// Envelope: `GET /api/sessions/{id}/patterns` returns `{"patterns": […]}`.
#[derive(Deserialize)]
struct PatternsEnvelope {
    #[serde(default)]
    patterns: Vec<PatternEvent>,
}

/// Envelope: `GET /api/sessions` returns `{"sessions": […], "total": N}`.
#[derive(Deserialize)]
struct SessionsEnvelope {
    #[serde(default)]
    sessions: Vec<ApiSessionRow>,
}

/// The trimmed per-session row the REST list endpoint emits. Field names
/// differ from `SessionRow` (`session_id`/`start_time`), so we map rather
/// than deserialize directly. Aliases tolerate either naming.
#[derive(Deserialize)]
struct ApiSessionRow {
    #[serde(alias = "id")]
    session_id: String,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    project_name: Option<String>,
    #[serde(default)]
    label: Option<String>,
    #[serde(default, alias = "start", alias = "first_event")]
    start_time: Option<String>,
    #[serde(default)]
    last_event: Option<String>,
    #[serde(default)]
    event_count: u64,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    host: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    origin_agent: Option<String>,
}

impl From<ApiSessionRow> for SessionRow {
    fn from(r: ApiSessionRow) -> Self {
        SessionRow {
            id: r.session_id,
            project_id: r.project_id,
            project_name: r.project_name,
            label: r.label,
            custom_label: None,
            branch: r.branch,
            event_count: r.event_count,
            first_event: r.start_time,
            last_event: r.last_event,
            host: r.host,
            user: r.user,
            origin_agent: r.origin_agent,
        }
    }
}

#[async_trait]
impl EventStore for HttpEventStore {
    // ── Primitives the tools (and composite tools) build on ──────────

    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
        // Bare array of raw CloudEvents (api.rs: get_events → Json(Array)).
        self.get(&format!("/api/sessions/{session_id}/events"), &[])
            .await
    }

    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        // The tool filters/sorts/limits itself, so mirror SqliteStore and
        // return the full set. Ask for a high cap to avoid server-side
        // pagination truncating the view the tool reasons over.
        let env: SessionsEnvelope = self
            .get("/api/sessions", &[("limit", "100000".to_string())])
            .await?;
        Ok(env.sessions.into_iter().map(SessionRow::from).collect())
    }

    async fn session_patterns(
        &self,
        session_id: &str,
        pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        let query: Vec<(&str, String)> = match pattern_type {
            Some(t) => vec![("type", t.to_string())],
            None => vec![],
        };
        let env: PatternsEnvelope = self
            .get(&format!("/api/sessions/{session_id}/patterns"), &query)
            .await?;
        Ok(env.patterns)
    }

    // ── Analytics / narrative query methods (infallible signatures) ──

    async fn query_session_synopsis(&self, session_id: &str) -> Option<queries::SessionSynopsis> {
        // 404 (unknown session) or a null body both mean "no synopsis".
        match self
            .get::<Option<queries::SessionSynopsis>>(
                &format!("/api/sessions/{session_id}/synopsis"),
                &[],
            )
            .await
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("open-story-mcp: {e} — no synopsis");
                None
            }
        }
    }

    async fn query_tool_journey(&self, session_id: &str) -> Vec<queries::ToolStep> {
        self.get_or_default(&format!("/api/sessions/{session_id}/tool-journey"), &[])
            .await
    }

    async fn query_file_impact(&self, session_id: &str) -> Vec<queries::FileImpact> {
        self.get_or_default(&format!("/api/sessions/{session_id}/file-impact"), &[])
            .await
    }

    async fn query_session_errors(&self, session_id: &str) -> Vec<queries::SessionError> {
        self.get_or_default(&format!("/api/sessions/{session_id}/errors"), &[])
            .await
    }

    async fn query_project_pulse(&self, days: u32) -> Vec<queries::ProjectPulse> {
        self.get_or_default("/api/insights/pulse", &[("days", days.to_string())])
            .await
    }

    async fn query_project_context(
        &self,
        project_id: &str,
        _limit: usize,
    ) -> Vec<queries::ProjectSession> {
        // REST hardcodes limit=5 server-side; the MCP `limit` arg is not
        // wired through (parity note in tests/http_store.rs).
        self.get_or_default(
            "/api/agent/project-context",
            &[("project", project_id.to_string())],
        )
        .await
    }

    async fn query_recent_files(&self, project_id: &str, _session_limit: usize) -> Vec<String> {
        self.get_or_default(
            "/api/agent/recent-files",
            &[("project", project_id.to_string())],
        )
        .await
    }

    async fn query_productivity_by_hour(&self, days: u32) -> Vec<queries::HourlyActivity> {
        self.get_or_default("/api/insights/productivity", &[("days", days.to_string())])
            .await
    }

    async fn query_token_usage(
        &self,
        days: Option<u32>,
        session_id: Option<&str>,
        model: &str,
    ) -> queries::TokenUsageSummary {
        let mut query: Vec<(&str, String)> = vec![("model", model.to_string())];
        if let Some(d) = days {
            query.push(("days", d.to_string()));
        }
        if let Some(sid) = session_id {
            query.push(("session_id", sid.to_string()));
        }
        match self
            .get::<queries::TokenUsageSummary>("/api/insights/token-usage", &query)
            .await
        {
            Ok(v) => v,
            Err(e) => {
                eprintln!("open-story-mcp: {e} — zero token usage");
                // Mirror the trait's default shape (model echoed back).
                queries::TokenUsageSummary {
                    session_count: 0,
                    usage: queries::TokenUsage::default(),
                    cost: queries::CostEstimate {
                        input: 0.0,
                        output: 0.0,
                        cache_read: 0.0,
                        cache_creation: 0.0,
                        total: 0.0,
                        model: model.to_string(),
                    },
                    sessions: Vec::new(),
                }
            }
        }
    }

    async fn query_daily_token_usage(&self, days: Option<u32>) -> Vec<queries::DailyTokenUsage> {
        let query = match days {
            Some(d) => vec![("days", d.to_string())],
            None => vec![],
        };
        self.get_or_default("/api/insights/token-usage/daily", &query)
            .await
    }

    async fn search_fts(
        &self,
        query: &str,
        limit: usize,
        session_filter: Option<&str>,
    ) -> Result<Vec<queries::FtsSearchResult>> {
        let mut q: Vec<(&str, String)> =
            vec![("q", query.to_string()), ("limit", limit.to_string())];
        if let Some(sid) = session_filter {
            q.push(("session_id", sid.to_string()));
        }
        self.get("/api/search", &q).await
    }

    // ── Read-through extras (not used by tools today, kept honest) ───

    async fn session_turns(&self, session_id: &str) -> Result<Vec<StructuralTurn>> {
        // /api/sessions/{id}/turns exists; deserialize defensively.
        self.get(&format!("/api/sessions/{session_id}/turns"), &[])
            .await
    }

    async fn full_payload(&self, _event_id: &str) -> Result<Option<String>> {
        // Not reached by any MCP tool; the content endpoint is keyed by
        // (session_id, event_id) which we don't have here. Return None.
        Ok(None)
    }

    // ── Writes: this store is read-only. The MCP never calls these. ──

    async fn insert_event(&self, _session_id: &str, _event: &Value) -> Result<bool> {
        Err(anyhow!("HttpEventStore is read-only: insert_event unsupported"))
    }

    async fn insert_batch(&self, _session_id: &str, _events: &[Value]) -> Result<usize> {
        Err(anyhow!("HttpEventStore is read-only: insert_batch unsupported"))
    }

    async fn upsert_session(&self, _session: &SessionRow) -> Result<()> {
        Err(anyhow!("HttpEventStore is read-only: upsert_session unsupported"))
    }

    async fn insert_pattern(&self, _session_id: &str, _pattern: &PatternEvent) -> Result<()> {
        Err(anyhow!("HttpEventStore is read-only: insert_pattern unsupported"))
    }

    async fn insert_turn(&self, _session_id: &str, _turn: &StructuralTurn) -> Result<()> {
        Err(anyhow!("HttpEventStore is read-only: insert_turn unsupported"))
    }

    async fn upsert_plan(&self, _plan_id: &str, _session_id: &str, _content: &str) -> Result<()> {
        Err(anyhow!("HttpEventStore is read-only: upsert_plan unsupported"))
    }
}
