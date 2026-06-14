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
    /// `OPEN_STORY_HOST`). Populated from the first CloudEvent in a batch;
    /// survives NATS replication so origin identity is preserved.
    /// `None` for pre-migration rows and events that arrived without a
    /// host stamp.
    #[serde(default)]
    pub host: Option<String>,
    /// Human user who owns this OpenStory instance (`OPEN_STORY_USER` or
    /// `$USER`). Orthogonal to `host`: a single user runs OpenStory on
    /// multiple machines. Populated from the first CloudEvent in a batch.
    /// `None` for pre-migration rows and events that arrived without a
    /// user stamp.
    #[serde(default)]
    pub user: Option<String>,
    /// Agent platform that produced this session (for example `claude-code`,
    /// `codex`, `pi-mono`, or `hermes`). Populated from the first CloudEvent
    /// in a batch and coalesced like host/user.
    #[serde(default)]
    pub origin_agent: Option<String>,
    /// OpenStory directory person id — the sovereign owner of this session
    /// in the identity model. Distinct from the OS-level `user` field.
    /// Populated from the first CloudEvent's `person_id` in a batch.
    /// `None` for pre-migration rows and events that arrived without a
    /// person_id stamp.
    #[serde(default)]
    pub person_id: Option<String>,
    /// Principal id — the device or agent in the person's fleet that
    /// produced this session. A session is one transcript file from one
    /// source, so it has exactly one principal. Used by the UI to group
    /// sessions under the fleet member that produced them.
    /// `None` for pre-migration rows.
    #[serde(default)]
    pub principal_id: Option<String>,
}

impl SessionRow {
    /// Display label: custom_label if set, otherwise auto-generated label.
    pub fn display_label(&self) -> Option<&str> {
        self.custom_label.as_deref().or(self.label.as_deref())
    }
}

/// Per-session share/store policy mode (Admin v0 → Phase 4 federation).
///
/// Sovereignty by default is **share by default at the operator's choice** —
/// the *operator* decides whether a session leaves the device, and the
/// default is "yes" (matches today's behavior where every session goes to
/// the hub aggregate). Marking a session `Private` makes it explicit:
/// it never leaves the device. The bus enforces this via `filter_subject`
/// on the cross-domain source; the API enforces it on digests and event
/// reads (invariant ① from the federation research doc).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SharePolicyMode {
    Shared,
    Private,
}

impl SharePolicyMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            SharePolicyMode::Shared => "shared",
            SharePolicyMode::Private => "private",
        }
    }
}

impl std::str::FromStr for SharePolicyMode {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "shared" => Ok(SharePolicyMode::Shared),
            "private" => Ok(SharePolicyMode::Private),
            _ => Err(format!("invalid share policy mode '{}': expected 'shared' or 'private'", s)),
        }
    }
}

/// One row in the share_policy table — sessions that have been **explicitly**
/// configured. Sessions without a row default to `SharePolicyMode::Private`
/// (sharing is opt-in).
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct SharePolicyRow {
    pub session_id: String,
    pub mode: SharePolicyMode,
    /// ISO-8601 wall clock when this row was last written.
    pub updated_at: String,
    /// Optional principal that performed the update (when auth tracks it).
    pub updated_by: Option<String>,
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

    /// Insert a batch and report, per input event, whether it was newly
    /// inserted (`true`) or a duplicate skipped by the PK (`false`). Order
    /// matches `events`.
    ///
    /// This is the dedup-aware form the persist consumer needs: it gates the
    /// JSONL append and FTS index on *which* events are new, but wants the
    /// whole batch to land in a single transaction (one fsync) rather than
    /// one transaction per event. The default impl preserves semantics by
    /// looping `insert_event`; `SqliteStore` overrides it with a single
    /// transaction. See the batched persist path in
    /// `server/src/consumers/persist.rs`.
    async fn insert_batch_returning(
        &self,
        session_id: &str,
        events: &[Value],
    ) -> Result<Vec<bool>> {
        let mut flags = Vec::with_capacity(events.len());
        for event in events {
            flags.push(self.insert_event(session_id, event).await?);
        }
        Ok(flags)
    }

    /// Load all events for a session, ordered by timestamp.
    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>>;

    /// List all sessions with summary metadata.
    async fn list_sessions(&self) -> Result<Vec<SessionRow>>;

    /// Update session projection metadata.
    async fn upsert_session(&self, session: &SessionRow) -> Result<()>;

    /// Recompute and **authoritatively set** `first_event`/`last_event` for a
    /// session from its durably-stored events, excluding subtypes whose `time`
    /// is synthesized at translation (`file.snapshot` — the source line has no
    /// timestamp, so it is stamped `Utc::now()` and would otherwise re-stamp a
    /// dead session's recency to boot time on every restart). See
    /// `open_story_core::subtype::SYNTHESIZED_TIME_SUBTYPES`.
    ///
    /// Unlike [`upsert_session`](Self::upsert_session), which MIN/MAX-merges
    /// (and therefore can only *raise* `last_event`), this overwrites — so it
    /// can lower a value already polluted by boot-stamped snapshots. Returns
    /// the new `(first_event, last_event)` bounds.
    ///
    /// Default impl is a no-op for backends that don't track session bounds.
    async fn recompute_session_bounds(
        &self,
        _session_id: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        Ok((None, None))
    }

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
    async fn upsert_plan(&self, plan_id: &str, session_id: &str, content: &str) -> Result<()>;

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

    /// Delete sessions whose most recent activity is older than
    /// `retention_days`. Returns the number of sessions deleted.
    ///
    /// **Invariant ② (federation Phase 4):** sessions whose `host` equals
    /// `keep_host` MUST be preserved regardless of age — your own data is
    /// yours, always; only fleet-mirrored sessions are eligible for the
    /// sweep. Pass `None` only when operating outside federation (single-
    /// host dev cleanup or a legacy retention sweep that hasn't been
    /// audited for sovereignty). Default impl is a no-op for backends that
    /// don't track session bounds.
    async fn cleanup_old_sessions(
        &self,
        _retention_days: u32,
        _keep_host: Option<&str>,
    ) -> Result<u64> {
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
    async fn query_project_context(
        &self,
        _project_id: &str,
        _limit: usize,
    ) -> Vec<queries::ProjectSession> {
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
    async fn query_token_usage(
        &self,
        _days: Option<u32>,
        _session_id: Option<&str>,
        _model: &str,
    ) -> queries::TokenUsageSummary {
        queries::TokenUsageSummary {
            session_count: 0,
            usage: queries::TokenUsage::default(),
            cost: queries::CostEstimate {
                input: 0.0,
                output: 0.0,
                cache_read: 0.0,
                cache_creation: 0.0,
                total: 0.0,
                model: "sonnet".into(),
            },
            sessions: Vec::new(),
        }
    }

    /// Daily token usage trend.
    async fn query_daily_token_usage(&self, _days: Option<u32>) -> Vec<queries::DailyTokenUsage> {
        Vec::new()
    }

    // ── Full-text search (default: not supported on JSONL fallback) ──

    /// Index a record in the full-text index.
    async fn index_fts(
        &self,
        _event_id: &str,
        _session_id: &str,
        _record_type: &str,
        _text: &str,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    /// Index a batch of records in the full-text index in one transaction.
    ///
    /// Each tuple is `(event_id, session_id, record_type, text)`. The default
    /// impl loops `index_fts`; `SqliteStore` overrides it to commit once for
    /// the whole batch (one fsync instead of one per record).
    async fn index_fts_batch(&self, records: &[(String, String, String, String)]) -> Result<()> {
        for (event_id, session_id, record_type, text) in records {
            self.index_fts(event_id, session_id, record_type, text)
                .await?;
        }
        Ok(())
    }

    /// Full-text search across indexed events.
    async fn search_fts(
        &self,
        _query: &str,
        _limit: usize,
        _session_filter: Option<&str>,
    ) -> anyhow::Result<Vec<queries::FtsSearchResult>> {
        Ok(vec![])
    }

    /// Count of records in the full-text index.
    async fn fts_count(&self) -> anyhow::Result<u64> {
        Ok(0)
    }

    // ── Share policy (Admin v0 → Phase 4) ────────────────────────────────
    //
    // The sovereignty-by-default convention: a session whose policy is
    // unset returns `Shared`. Marking it `Private` is the explicit edge
    // sovereignty hook — the bus's `filter_subject` and the API's digest
    // filter both consult this.
    //
    // Default impls cover backends that don't implement policy storage:
    // get returns `Private`, set returns `Err` (so an unsupported backend
    // surfaces the gap loudly instead of silently dropping a write). A
    // backend that can't record consent must fail safe — never share.
    // `SqliteStore` overrides both.

    /// Get this session's share policy. Returns `Private` when no row exists —
    /// sharing is opt-in (sovereignty default).
    async fn get_share_policy(&self, _session_id: &str) -> Result<SharePolicyMode> {
        Ok(SharePolicyMode::Private)
    }

    /// Set this session's share policy. Backends without storage error out.
    async fn set_share_policy(
        &self,
        _session_id: &str,
        _mode: SharePolicyMode,
        _updated_by: Option<&str>,
    ) -> Result<()> {
        anyhow::bail!("share policy storage not implemented for this EventStore backend")
    }

    /// List every session that has an explicitly-set policy. Sessions not
    /// returned default to `Shared`.
    async fn list_share_policies(&self) -> Result<Vec<SharePolicyRow>> {
        Ok(vec![])
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
            user: None,
            origin_agent: None,
            person_id: None,
            principal_id: None,
        };
        assert_eq!(row.id, "test");
        assert_eq!(row.event_count, 0);
    }
}
