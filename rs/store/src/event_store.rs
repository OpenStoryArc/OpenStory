//! EventStore trait — the persistence interface for open-story.
//!
//! SQLite is the default implementation. JSONL is the fallback. MongoDB is
//! available behind the `mongo` feature flag for distributed deployments.
//! The trait is shaped by what SQLite can do (indexed queries, dedup via PK).
//!
//! All methods are `async`. Sync backends (SQLite via rusqlite) hold their
//! work inside the async fn body without internal `.await` points — the
//! `Mutex<Connection>` guard never crosses an await boundary. If a backend
//! grows long-running blocking calls, wrap them in `tokio::task::spawn_blocking`.

use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};

use crate::queries;

/// Summary row for a session — materialized from SessionProjection.
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SessionRow {
    pub id: String,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub label: Option<String>,
    /// User-set custom name. Takes precedence over auto-generated label.
    /// Boot replay never overwrites this field.
    pub custom_label: Option<String>,
    pub branch: Option<String>,
    pub event_count: u64,
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    /// Host where the session's events were translated (`gethostname()` or
    /// `OPENSTORY_HOST`). Populated from the first CloudEvent in a batch;
    /// survives NATS replication so origin identity is preserved.
    /// `None` for pre-migration rows and events that arrived without a
    /// host stamp.
    #[serde(default)]
    pub host: Option<String>,
}

impl SessionRow {
    /// Display label: custom_label if set, otherwise auto-generated label.
    pub fn display_label(&self) -> Option<&str> {
        self.custom_label.as_deref().or(self.label.as_deref())
    }
}

/// Persistence interface for events, sessions, patterns, and plans.
///
/// Implementations:
/// - `SqliteStore` — default, full query capability
/// - `JsonlStore` — fallback, append-only, limited queries
/// - `MongoStore` — distributed alternative (behind `mongo` feature)
#[async_trait]
pub trait EventStore: Send + Sync {
    /// Insert an event. Returns true if new, false if duplicate.
    async fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool>;

    /// Insert a batch of events. Returns count of new (non-duplicate) events.
    async fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize>;

    /// Load all events for a session, ordered by timestamp.
    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>>;

    /// List all sessions with summary metadata.
    async fn list_sessions(&self) -> Result<Vec<SessionRow>>;

    /// Update session projection metadata.
    async fn upsert_session(&self, session: &SessionRow) -> Result<()>;

    /// Insert a detected pattern.
    async fn insert_pattern(&self, session_id: &str, pattern: &PatternEvent) -> Result<()>;

    /// Query patterns for a session, optionally filtered by type.
    async fn session_patterns(
        &self,
        session_id: &str,
        pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>>;

    /// Insert a completed structural turn.
    async fn insert_turn(&self, session_id: &str, turn: &StructuralTurn) -> Result<()>;

    /// Query structural turns for a session, ordered by turn_number.
    async fn session_turns(&self, session_id: &str) -> Result<Vec<StructuralTurn>>;

    /// Store a plan.
    async fn upsert_plan(
        &self,
        plan_id: &str,
        session_id: &str,
        content: &str,
    ) -> Result<()>;

    /// Get full payload for an event (un-truncated).
    async fn full_payload(&self, event_id: &str) -> Result<Option<String>>;

    /// Set a user-defined custom label for a session.
    async fn update_session_label(&self, _session_id: &str, _label: &str) -> Result<()> {
        Ok(())
    }

    /// Delete a session and all its events, patterns, and plans.
    /// Returns the number of events deleted.
    async fn delete_session(&self, _session_id: &str) -> Result<u64> {
        Ok(0)
    }

    /// Export all events for a session as JSONL (newline-delimited JSON).
    async fn export_session_jsonl(&self, session_id: &str) -> Result<String> {
        let events = self.session_events(session_id).await?;
        let mut lines = Vec::with_capacity(events.len());
        for event in &events {
            lines.push(serde_json::to_string(event)?);
        }
        Ok(lines.join("\n"))
    }

    /// Delete sessions older than `retention_days`.
    /// Returns the number of sessions deleted.
    async fn cleanup_old_sessions(&self, _retention_days: u32) -> Result<u64> {
        Ok(0)
    }

    // ── Query methods (default: not supported on JSONL fallback) ─────

    /// Session synopsis.
    async fn query_session_synopsis(&self, _session_id: &str) -> Option<queries::SessionSynopsis> {
        None
    }

    /// Tool journey for a session.
    async fn query_tool_journey(&self, _session_id: &str) -> Vec<queries::ToolStep> {
        Vec::new()
    }

    /// File impact for a session.
    async fn query_file_impact(&self, _session_id: &str) -> Vec<queries::FileImpact> {
        Vec::new()
    }

    /// Session errors.
    async fn query_session_errors(&self, _session_id: &str) -> Vec<queries::SessionError> {
        Vec::new()
    }

    /// Project activity pulse.
    async fn query_project_pulse(&self, _days: u32) -> Vec<queries::ProjectPulse> {
        Vec::new()
    }

    /// Tool evolution over time.
    async fn query_tool_evolution(&self, _days: u32) -> Vec<queries::ToolEvolution> {
        Vec::new()
    }

    /// Session efficiency metrics.
    async fn query_session_efficiency(&self) -> Vec<queries::SessionEfficiency> {
        Vec::new()
    }

    /// Project context: recent sessions.
    async fn query_project_context(&self, _project_id: &str, _limit: usize) -> Vec<queries::ProjectSession> {
        Vec::new()
    }

    /// Recent files for a project.
    async fn query_recent_files(&self, _project_id: &str, _session_limit: usize) -> Vec<String> {
        Vec::new()
    }

    /// Productivity by hour.
    async fn query_productivity_by_hour(&self, _days: u32) -> Vec<queries::HourlyActivity> {
        Vec::new()
    }

    /// Token usage summary (optionally filtered by days or session).
    async fn query_token_usage(&self, _days: Option<u32>, _session_id: Option<&str>, _model: &str) -> queries::TokenUsageSummary {
        queries::TokenUsageSummary {
            session_count: 0,
            usage: queries::TokenUsage::default(),
            cost: queries::CostEstimate { input: 0.0, output: 0.0, cache_read: 0.0, cache_creation: 0.0, total: 0.0, model: "sonnet".into() },
            sessions: Vec::new(),
        }
    }

    /// Daily token usage trend.
    async fn query_daily_token_usage(&self, _days: Option<u32>) -> Vec<queries::DailyTokenUsage> {
        Vec::new()
    }

    // ── Full-text search (default: not supported on JSONL fallback) ──

    /// Index a record in the full-text index.
    async fn index_fts(&self, _event_id: &str, _session_id: &str, _record_type: &str, _text: &str) -> anyhow::Result<()> {
        Ok(())
    }

    /// Full-text search across indexed events.
    async fn search_fts(&self, _query: &str, _limit: usize, _session_filter: Option<&str>) -> anyhow::Result<Vec<queries::FtsSearchResult>> {
        Ok(vec![])
    }

    /// Count of records in the full-text index.
    async fn fts_count(&self) -> anyhow::Result<u64> {
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_is_object_safe() {
        // Compile-time check: EventStore can be used as a trait object
        fn _assert_object_safe(_: &dyn EventStore) {}
    }

    #[test]
    fn session_row_default_fields() {
        let row = SessionRow {
            id: "test".to_string(),
            project_id: None,
            project_name: None,
            label: None,
            custom_label: None,
            branch: None,
            event_count: 0,
            first_event: None,
            last_event: None,
            host: None,
        };
        assert_eq!(row.id, "test");
        assert_eq!(row.event_count, 0);
    }
}
