//! An EventStore that remembers every call it forwards, so a spec can say
//! what a boot asked the store for (the boot-pass plan, rows B-02..B-04)
//! without reading logs. Sibling of `recording_bus.rs`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use open_story_patterns::{PatternEvent, StructuralTurn};
use open_story_store::event_store::{BootFacts, EventStore, PresenceRow, SessionRow};
use open_story_store::queries;

pub struct RecordingStore {
    inner: Arc<dyn EventStore>,
    calls: Mutex<HashMap<&'static str, usize>>,
}

impl RecordingStore {
    pub fn wrap(inner: Arc<dyn EventStore>) -> Arc<Self> {
        Arc::new(Self {
            inner,
            calls: Mutex::new(HashMap::new()),
        })
    }

    /// How many times `method` was called since construction.
    pub fn calls(&self, method: &str) -> usize {
        self.calls.lock().unwrap().get(method).copied().unwrap_or(0)
    }

    /// Every method called, with its count — for a failure message.
    pub fn all_calls(&self) -> Vec<(String, usize)> {
        let mut v: Vec<(String, usize)> = self
            .calls
            .lock()
            .unwrap()
            .iter()
            .map(|(k, n)| (k.to_string(), *n))
            .collect();
        v.sort();
        v
    }

    fn hit(&self, method: &'static str) {
        *self.calls.lock().unwrap().entry(method).or_insert(0) += 1;
    }
}

#[async_trait]
impl EventStore for RecordingStore {
    async fn insert_event(&self, session_id: &str, event: &Value) -> Result<bool> {
        self.hit("insert_event");
        self.inner.insert_event(session_id, event).await
    }
    async fn insert_batch(&self, session_id: &str, events: &[Value]) -> Result<usize> {
        self.hit("insert_batch");
        self.inner.insert_batch(session_id, events).await
    }
    async fn insert_batch_returning(
        &self,
        session_id: &str,
        events: &[Value],
    ) -> Result<Vec<bool>> {
        self.hit("insert_batch_returning");
        self.inner.insert_batch_returning(session_id, events).await
    }
    async fn session_events(&self, session_id: &str) -> Result<Vec<Value>> {
        self.hit("session_events");
        self.inner.session_events(session_id).await
    }
    async fn session_boot_facts(&self, session_id: &str) -> Result<BootFacts> {
        self.hit("session_boot_facts");
        self.inner.session_boot_facts(session_id).await
    }
    async fn session_events_before(
        &self,
        session_id: &str,
        before_seq: Option<u64>,
        limit: usize,
    ) -> Result<Vec<Value>> {
        self.hit("session_events_before");
        self.inner
            .session_events_before(session_id, before_seq, limit)
            .await
    }
    async fn list_sessions(&self) -> Result<Vec<SessionRow>> {
        self.hit("list_sessions");
        self.inner.list_sessions().await
    }
    async fn upsert_session(&self, session: &SessionRow) -> Result<()> {
        self.hit("upsert_session");
        self.inner.upsert_session(session).await
    }
    async fn recompute_session_bounds(
        &self,
        session_id: &str,
    ) -> Result<(Option<String>, Option<String>)> {
        self.hit("recompute_session_bounds");
        self.inner.recompute_session_bounds(session_id).await
    }
    async fn insert_pattern(&self, session_id: &str, pattern: &PatternEvent) -> Result<()> {
        self.hit("insert_pattern");
        self.inner.insert_pattern(session_id, pattern).await
    }
    async fn session_patterns(
        &self,
        session_id: &str,
        pattern_type: Option<&str>,
    ) -> Result<Vec<PatternEvent>> {
        self.hit("session_patterns");
        self.inner.session_patterns(session_id, pattern_type).await
    }
    async fn insert_turn(&self, session_id: &str, turn: &StructuralTurn) -> Result<()> {
        self.hit("insert_turn");
        self.inner.insert_turn(session_id, turn).await
    }
    async fn session_turns(&self, session_id: &str) -> Result<Vec<StructuralTurn>> {
        self.hit("session_turns");
        self.inner.session_turns(session_id).await
    }
    async fn upsert_presence(&self, row: &PresenceRow) -> Result<()> {
        self.hit("upsert_presence");
        self.inner.upsert_presence(row).await
    }
    async fn latest_presence(&self) -> Result<Vec<PresenceRow>> {
        self.hit("latest_presence");
        self.inner.latest_presence().await
    }
    async fn upsert_plan(&self, plan_id: &str, session_id: &str, content: &str) -> Result<()> {
        self.hit("upsert_plan");
        self.inner.upsert_plan(plan_id, session_id, content).await
    }
    async fn full_payload(&self, event_id: &str) -> Result<Option<String>> {
        self.hit("full_payload");
        self.inner.full_payload(event_id).await
    }
    async fn update_session_label(&self, session_id: &str, label: &str) -> Result<()> {
        self.hit("update_session_label");
        self.inner.update_session_label(session_id, label).await
    }
    async fn delete_session(&self, session_id: &str) -> Result<u64> {
        self.hit("delete_session");
        self.inner.delete_session(session_id).await
    }
    async fn export_session_jsonl(&self, session_id: &str) -> Result<String> {
        self.hit("export_session_jsonl");
        self.inner.export_session_jsonl(session_id).await
    }
    async fn cleanup_old_sessions(
        &self,
        retention_days: u32,
        keep_host: Option<&str>,
    ) -> Result<u64> {
        self.hit("cleanup_old_sessions");
        self.inner
            .cleanup_old_sessions(retention_days, keep_host)
            .await
    }
    async fn query_session_synopsis(&self, session_id: &str) -> Option<queries::SessionSynopsis> {
        self.hit("query_session_synopsis");
        self.inner.query_session_synopsis(session_id).await
    }
    async fn query_tool_journey(&self, session_id: &str) -> Vec<queries::ToolStep> {
        self.hit("query_tool_journey");
        self.inner.query_tool_journey(session_id).await
    }
    async fn query_file_impact(&self, session_id: &str) -> Vec<queries::FileImpact> {
        self.hit("query_file_impact");
        self.inner.query_file_impact(session_id).await
    }
    async fn query_session_errors(&self, session_id: &str) -> Vec<queries::SessionError> {
        self.hit("query_session_errors");
        self.inner.query_session_errors(session_id).await
    }
    async fn query_project_pulse(&self, days: u32) -> Vec<queries::ProjectPulse> {
        self.hit("query_project_pulse");
        self.inner.query_project_pulse(days).await
    }
    async fn query_tool_evolution(&self, days: u32) -> Vec<queries::ToolEvolution> {
        self.hit("query_tool_evolution");
        self.inner.query_tool_evolution(days).await
    }
    async fn query_session_efficiency(&self) -> Vec<queries::SessionEfficiency> {
        self.hit("query_session_efficiency");
        self.inner.query_session_efficiency().await
    }
    async fn query_project_context(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Vec<queries::ProjectSession> {
        self.hit("query_project_context");
        self.inner.query_project_context(project_id, limit).await
    }
    async fn query_recent_files(&self, project_id: &str, session_limit: usize) -> Vec<String> {
        self.hit("query_recent_files");
        self.inner
            .query_recent_files(project_id, session_limit)
            .await
    }
    async fn query_productivity_by_hour(&self, days: u32) -> Vec<queries::HourlyActivity> {
        self.hit("query_productivity_by_hour");
        self.inner.query_productivity_by_hour(days).await
    }
    async fn query_token_usage(
        &self,
        days: Option<u32>,
        session_id: Option<&str>,
        model: &str,
    ) -> queries::TokenUsageSummary {
        self.hit("query_token_usage");
        self.inner.query_token_usage(days, session_id, model).await
    }
    async fn query_daily_token_usage(&self, days: Option<u32>) -> Vec<queries::DailyTokenUsage> {
        self.hit("query_daily_token_usage");
        self.inner.query_daily_token_usage(days).await
    }
    async fn index_fts(
        &self,
        event_id: &str,
        session_id: &str,
        record_type: &str,
        text: &str,
    ) -> Result<()> {
        self.hit("index_fts");
        self.inner
            .index_fts(event_id, session_id, record_type, text)
            .await
    }
    async fn index_fts_batch(&self, records: &[(String, String, String, String)]) -> Result<()> {
        self.hit("index_fts_batch");
        self.inner.index_fts_batch(records).await
    }
    async fn fts_count_for_session(&self, session_id: &str) -> Result<Option<u64>> {
        self.hit("fts_count_for_session");
        self.inner.fts_count_for_session(session_id).await
    }
    async fn search_fts(
        &self,
        query: &str,
        limit: usize,
        session_filter: Option<&str>,
    ) -> Result<Vec<queries::FtsSearchResult>> {
        self.hit("search_fts");
        self.inner.search_fts(query, limit, session_filter).await
    }
    async fn fts_count(&self) -> Result<u64> {
        self.hit("fts_count");
        self.inner.fts_count().await
    }
}
