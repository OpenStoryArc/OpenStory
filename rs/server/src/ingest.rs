//! Event ingestion pipeline — dedup, persist, project, pattern-detect, broadcast.
//!
//! Pure functions (is_plan_event, extract_plan_content, to_wire_record) live in
//! open-story-store::ingest and are re-exported here. This module retains the
//! stateful orchestration (ingest_events, replay_boot_sessions) that depends on AppState.

use crate::logging::log_event;
use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use open_story_views::wire_record::{WireRecord, TRUNCATION_THRESHOLD};

use open_story_core::cloud_event::CloudEvent;
use open_story_store::analysis;
use open_story_store::projection;

use crate::broadcast::BroadcastMessage;
use crate::state::AppState;

// Re-export pure functions from the store crate.
pub use open_story_store::ingest::{extract_plan_content, is_plan_event, to_wire_record};

/// Result of ingesting events — count + change notifications for broadcasting.
///
/// Callers are responsible for sending changes to `broadcast_tx` (local WS clients)
/// and/or publishing to the bus (distributed consumers via "changes.store.*").
pub struct IngestResult {
    /// Number of events ingested (after dedup).
    pub count: usize,
    /// Change notifications generated during ingestion.
    /// Each corresponds to a BroadcastMessage that should be sent to subscribers.
    pub changes: Vec<BroadcastMessage>,
}

/// Ingest translated CloudEvents into session state, persist, and return changes.
///
/// Returns an `IngestResult` with the count of events ingested (after dedup) and
/// a list of `BroadcastMessage` changes. The caller is responsible for sending
/// changes to `broadcast_tx` and/or publishing to the bus change feed.
pub async fn ingest_events(
    state: &mut AppState,
    session_id: &str,
    events: &[CloudEvent],
    project_id: Option<&str>,
) -> IngestResult {
    if events.is_empty() {
        return IngestResult { count: 0, changes: Vec::new() };
    }

    if let Some(pid) = project_id {
        // Normalize worktree entries to their parent project
        let normalized_pid = analysis::strip_worktree_suffix(pid).to_string();
        state
            .store
            .session_projects
            .insert(session_id.to_string(), normalized_pid.clone());
        // Derive display name from the normalized project_id
        if !state.store.session_project_names.contains_key(session_id) {
            let name =
                analysis::display_name_from_entry(&normalized_pid, &state.store.watch_dir_entries);
            state
                .store
                .session_project_names
                .insert(session_id.to_string(), name);
        }
    }

    // Fallback: derive project_id from cwd in events if not already known
    if !state.store.session_projects.contains_key(session_id) {
        if let Some(cwd) = events.iter().find_map(|e| {
            let val = serde_json::to_value(e).ok()?;
            analysis::extract_cwd(&val)
        }) {
            let resolved = analysis::resolve_project(&cwd, &state.store.watch_dir_entries);
            state
                .store
                .session_projects
                .insert(session_id.to_string(), resolved.project_id);
            state
                .store
                .session_project_names
                .insert(session_id.to_string(), resolved.project_name);
        }
    }

    let mut count = 0;
    let mut changes: Vec<BroadcastMessage> = Vec::new();

    for ce in events {
        let val = match serde_json::to_value(ce) {
            Ok(v) => v,
            Err(e) => {
                log_event("ingest", &format!("⚠ event serialization failed: {e}"));
                continue;
            }
        };
        {
            // Detect subagent → parent relationship (shared helper).
            open_story_store::state::detect_subagent_relationship(
                &val,
                session_id,
                &mut state.store.subagent_parents,
                &mut state.store.session_children,
            );

            // Persistence belongs exclusively to Actor 1 (PersistConsumer):
            //   - insert_event   → EventStore PK dedup
            //   - session_store.append → JSONL backup
            //   - index_fts     → full-text search
            // Those dual-writes were removed from ingest_events during the
            // Actor 4 decomposition (see DUAL_WRITE_AUDIT.md). Dedup is now
            // handled by the projection's own `seen_ids: HashSet<String>` —
            // `append()` returns `AppendResult::empty()` for duplicate IDs.

            // Update projection (dedup happens here now via seen_ids).
            let proj = state
                .store
                .projections
                .entry(session_id.to_string())
                .or_insert_with(|| projection::SessionProjection::new(session_id));
            let append_result = proj.append(&val);
            if append_result.is_empty() {
                // Duplicate event (seen_ids caught it) or unparseable CloudEvent.
                // Skip broadcast — Actor 1 handles persistence independently.
                // Log for observability so silent drops are visible.
                let eid = val.get("id").and_then(|v| v.as_str()).unwrap_or("?");
                let st = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("?");
                log_event("ingest", &format!("⚠ event {eid} ({st}) dedup'd or empty — skipped broadcast"));
                continue;
            }

            // Demo-mode persistence: when no event bus is active, the
            // actor-consumers (persist, patterns, projections, broadcast)
            // are never spawned — there is no one to deliver events to.
            // Persist inline so `/api/sessions/{id}/events` and FTS still
            // work. Under NATS, PersistConsumer owns these writes and
            // this block is skipped to keep the actor decomposition clean.
            if !state.bus.is_active() {
                let _ = state
                    .store
                    .event_store
                    .insert_event(session_id, &val)
                    .await;
                let _ = state.store.session_store.append(session_id, &val);
                for vr in from_cloud_event(ce).iter() {
                    if let Some(text) = open_story_store::extract::extract_text(vr) {
                        let rt = open_story_store::extract::record_type_str(&vr.body);
                        let _ = state
                            .store
                            .event_store
                            .index_fts(&vr.id, session_id, rt, &text)
                            .await;
                    }
                }
            }

            // Plan extraction
            if is_plan_event(&val) {
                let plan_content = extract_plan_content(&val).or_else(|| {
                    val.get("data")
                        .and_then(|d| d.get("agent_payload"))
                        .and_then(|ap| ap.get("args"))
                        .and_then(|a| a.get("plan"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                if let Some(content) = plan_content {
                    let timestamp = val.get("time").and_then(|v| v.as_str()).unwrap_or("");
                    let _ = state.store.plan_store.save(session_id, &content, timestamp);
                    // Dual-write plan to EventStore
                    let plan_id = format!("plan:{}:{}", session_id, timestamp);
                    let _ = state.store.event_store.upsert_plan(&plan_id, session_id, &content).await;
                }
            }

            // BFF transform: CloudEvent → typed ViewRecords for the UI
            let view_records = from_cloud_event(ce);

            // Capture full payloads for truncated records (lazy-load endpoint)
            for vr in &view_records {
                if let RecordBody::ToolResult(tr) = &vr.body {
                    if let Some(output) = &tr.output {
                        if output.len() > TRUNCATION_THRESHOLD {
                            state
                                .store
                                .full_payloads
                                .entry(session_id.to_string())
                                .or_default()
                                .insert(vr.id.clone(), output.clone());
                        }
                    }
                }
            }

            // Classify records as durable or ephemeral
            let subtype = val.get("subtype").and_then(|v| v.as_str());
            let ephemeral = projection::is_ephemeral(subtype);
            let proj = state.store.projections.get(session_id).unwrap();

            // Pattern detection is now solely the patterns consumer's job
            // (Actor 2 in the actor-consumer architecture). ingest_events
            // used to maintain its own per-session PatternPipeline in
            // state.store.pattern_pipelines and feed events through it,
            // which created a *second* pipeline that ran in parallel with
            // Actor 2's pipeline — same events, two accumulators, two
            // sentence streams, double persistence. That was the source
            // of the duplication captured in scripts/inspect_sentence_dedup.py.
            //
            // Trade-off: live BroadcastMessage::Enriched payloads no longer
            // include the `patterns` field. The UI still gets patterns via
            // the REST API and via initial_state on next reload. The proper
            // fix — wiring Actor 2 to publish patterns to NATS so the
            // broadcast consumer can subscribe and forward — is the next
            // branch's work. See backlog: stream architecture rewrite.
            let detected_patterns: Vec<open_story_patterns::PatternEvent> = Vec::new();

            // FTS indexing belongs to Actor 1 (PersistConsumer).
            // Removed from ingest_events during the Actor 4 decomposition.
            // See: docs/research/architecture-audit/DUAL_WRITE_AUDIT.md
            let _ = ephemeral; // used for broadcast classification below

            // Collect label changes for this event
            let session_label = if append_result.label_changed {
                proj.label().map(|s| s.to_string())
            } else {
                None
            };
            let session_branch = if append_result.label_changed {
                proj.branch().map(|s| s.to_string())
            } else {
                None
            };
            // Check if token usage changed (any TokenUsage records in this batch)
            let tokens_changed = view_records
                .iter()
                .any(|vr| matches!(&vr.body, RecordBody::TokenUsage(_)));
            let token_fields = if tokens_changed {
                (
                    Some(proj.total_input_tokens()),
                    Some(proj.total_output_tokens()),
                )
            } else {
                (None, None)
            };

            if !view_records.is_empty()
                || !append_result.filter_deltas.is_empty()
                || !detected_patterns.is_empty()
            {
                if ephemeral {
                    changes.push(BroadcastMessage::Enriched {
                        session_id: session_id.to_string(),
                        records: Vec::new(),
                        ephemeral: view_records,
                        filter_deltas: append_result.filter_deltas,
                        patterns: Vec::new(),
                        project_id: state.store.session_projects.get(session_id).cloned(),
                        project_name: state.store.session_project_names.get(session_id).cloned(),
                        session_label: None,
                        session_branch: None,
                        total_input_tokens: None,
                        total_output_tokens: None,
                    });
                } else {
                    let wire_records: Vec<WireRecord> = view_records
                        .iter()
                        .map(|vr| to_wire_record(vr, proj))
                        .collect();
                    changes.push(BroadcastMessage::Enriched {
                        session_id: session_id.to_string(),
                        records: wire_records,
                        ephemeral: Vec::new(),
                        filter_deltas: append_result.filter_deltas,
                        patterns: detected_patterns,
                        project_id: state.store.session_projects.get(session_id).cloned(),
                        project_name: state.store.session_project_names.get(session_id).cloned(),
                        session_label,
                        session_branch,
                        total_input_tokens: token_fields.0,
                        total_output_tokens: token_fields.1,
                    });
                }
            }
            count += 1;
        }
    }
    // Record metrics
    if count > 0 {
        // Count ingested events by subtype
        for ce in events {
            if let Ok(val) = serde_json::to_value(ce) {
                let subtype = val.get("subtype").and_then(|v| v.as_str()).unwrap_or("unknown");
                crate::metrics::record_events_ingested(subtype, 1);
            }
        }
        let deduped = events.len() - count;
        if deduped > 0 {
            crate::metrics::record_events_deduped(deduped as u64);
        }
        let pattern_count: usize = changes.iter().map(|c| match c {
            BroadcastMessage::Enriched { patterns, .. } => patterns.len(),
            _ => 0,
        }).sum();
        if pattern_count > 0 {
            crate::metrics::record_patterns_detected(pattern_count as u64);
        }
    }

    // Flush session projection to EventStore once per batch (not per-event)
    if count > 0 {
        if let Some(proj) = state.store.projections.get(session_id) {
            let _ = state.store.event_store.upsert_session(
                &crate::event_store_bridge::session_row_from_projection(
                    session_id, proj, &state.store,
                ),
            ).await;
        }
    }

    IngestResult { count, changes }
}

/// Replay boot-loaded sessions through projections and pattern pipelines.
///
/// Called after `create_state()` to populate projections, filter counts,
/// and detected patterns from sessions that were loaded from disk.
/// Without this, `build_initial_state()` would return empty data for
/// boot-loaded sessions until new events arrive.
pub async fn replay_boot_sessions(state: &mut AppState) {
    let session_ids: Vec<String> = state.store.event_store
        .list_sessions()
        .await
        .unwrap_or_default()
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let mut total_events = 0;

    // One-time FTS5 backfill: if the index is empty, populate it during replay
    let fts_needs_backfill = state.store.event_store.fts_count().await.unwrap_or(0) == 0;

    for sid in &session_ids {
        let events = state.store.event_store
            .session_events(sid)
            .await
            .unwrap_or_default();
        if events.is_empty() {
            continue;
        }

        for val in &events {
            // Events are already in EventStore (that's where we read them from).
            // No need to re-insert — just replay through projections and patterns.

            // Detect subagent → parent relationship (shared helper).
            open_story_store::state::detect_subagent_relationship(
                val,
                sid,
                &mut state.store.subagent_parents,
                &mut state.store.session_children,
            );

            // Update projection
            let proj = state
                .store
                .projections
                .entry(sid.clone())
                .or_insert_with(|| projection::SessionProjection::new(sid));
            proj.append(val);

            // BFF transform — deserialize stored JSON to typed CloudEvent
            let view_records = match serde_json::from_value::<open_story_core::cloud_event::CloudEvent>(val.clone()) {
                Ok(ce) => from_cloud_event(&ce),
                Err(_) => vec![],
            };

            // Capture full payloads for truncated records
            for vr in &view_records {
                if let RecordBody::ToolResult(tr) = &vr.body {
                    if let Some(output) = &tr.output {
                        if output.len() > TRUNCATION_THRESHOLD {
                            state
                                .store
                                .full_payloads
                                .entry(sid.clone())
                                .or_default()
                                .insert(vr.id.clone(), output.clone());
                        }
                    }
                }
            }

            // FTS5 backfill: index during replay if the index was empty at boot
            let subtype = val.get("subtype").and_then(|v| v.as_str());
            let ephemeral = projection::is_ephemeral(subtype);

            if fts_needs_backfill && !ephemeral {
                for vr in &view_records {
                    if let Some(text) = open_story_store::extract::extract_text(vr) {
                        let record_type = open_story_store::extract::record_type_str(&vr.body);
                        let _ = state.store.event_store.index_fts(&vr.id, sid, record_type, &text).await;
                    }
                }
            }

            // Pattern detection retired from the boot-replay path. The
            // patterns consumer (Actor 2) is now the sole pattern detector
            // and runs over the same NATS replay stream independently.
            // (Boot replay still needs to drive the projections + storage
            // layers to get the in-memory state populated for the API.)
            let _ = ephemeral;

            total_events += 1;
        }

        // Dual-write session projection after processing all events
        if let Some(proj) = state.store.projections.get(sid) {
            let _ = state.store.event_store.upsert_session(
                &crate::event_store_bridge::session_row_from_projection(sid, proj, &state.store),
            ).await;
        }
    }

    if total_events > 0 {
        let fts_note = if fts_needs_backfill {
            let fts_count = state.store.event_store.fts_count().await.unwrap_or(0);
            format!(", FTS5 backfill: {fts_count} indexed")
        } else {
            String::new()
        };
        crate::logging::log_event(
            "boot",
            &format!(
                "replayed {} events across {} sessions{}",
                total_events,
                session_ids.len(),
                fts_note,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_bus::noop_bus::NoopBus;
    use open_story_core::event_data::EventData;
    use open_story_store::state::StoreState;

    // Pure function tests (is_plan_event, extract_plan_content, to_wire_record)
    // are in the open-story-store crate: store/src/ingest.rs

    // ── Helper ──────────────────────────────────────────────────────────

    fn test_app_state(tmp: &tempfile::TempDir) -> AppState {
        use std::collections::HashMap;
        use std::sync::Arc;
        use tokio::sync::broadcast as tokio_broadcast;

        let store = StoreState::new(tmp.path()).unwrap();
        let (broadcast_tx, _) = tokio_broadcast::channel(256);
        let watch_dir = tmp.path().join("watch");
        std::fs::create_dir_all(&watch_dir).unwrap();

        AppState {
            store,
            transcript_states: HashMap::new(),
            broadcast_tx,
            bus: Arc::new(NoopBus),
            config: crate::config::Config::default(),
            watch_dir,
        }
    }

    fn make_user_prompt_event(id: &str, text: &str) -> CloudEvent {
        CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            EventData::new(
                serde_json::json!({
                    "text": text,
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": text}]}
                    }
                }),
                1,
                "sess-1".to_string(),
            ),
            Some("message.user.prompt".to_string()),
            Some(id.to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        )
    }

    // ── ingest_events tests ─────────────────────────────────────────────

    #[tokio::test]
    async fn ingest_empty_events_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let result = ingest_events(&mut state, "sess-1", &[], None).await;
        assert_eq!(result.count, 0);
        assert!(state.store.event_store.list_sessions().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn ingest_deduplicates_by_projection_seen_ids() {
        // After the Actor 4 decomposition, dedup moved from EventStore PK
        // (insert_event returning Ok(false)) to SessionProjection::seen_ids.
        // The projection's HashSet catches the duplicate and returns
        // AppendResult::empty(), so the broadcast loop skips the event.
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-dup-1", "hello");

        let result1 = ingest_events(&mut state, "sess-1", &[event.clone()], None).await;
        assert_eq!(result1.count, 1);

        let result2 = ingest_events(&mut state, "sess-1", &[event], None).await;
        assert_eq!(result2.count, 0, "duplicate event should be skipped via projection seen_ids");
    }

    #[tokio::test]
    async fn ingest_does_not_persist_at_all_after_actor_4_decomposition() {
        // After the full Actor 4 decomposition, ingest_events is a
        // broadcast-only function. It does NOT write to EventStore,
        // SessionStore, or FTS. ALL persistence is Actor 1's job.
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-persist-1", "persist me");

        ingest_events(&mut state, "sess-persist", &[event], None).await;

        // EventStore: NOT written by ingest_events anymore.
        assert!(
            state.store.event_store.session_events("sess-persist").await.unwrap().is_empty(),
            "ingest_events must NOT write to EventStore — Actor 1 owns persistence"
        );

        // SessionStore: NOT written (confirmed earlier in the JSONL fix).
        assert!(
            state.store.session_store.load_session("sess-persist").is_empty(),
            "ingest_events must NOT write to SessionStore — Actor 1 owns JSONL"
        );

        // But: projections SHOULD be populated (ingest_events still owns
        // in-memory projection state for wire-record enrichment).
        assert!(
            state.store.projections.contains_key("sess-persist"),
            "ingest_events must still update projections for broadcast assembly"
        );
    }

    #[tokio::test]
    async fn ingest_associates_project_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-proj-1", "hello");

        ingest_events(&mut state, "sess-proj", &[event], Some("my-project")).await;

        assert_eq!(
            state.store.session_projects.get("sess-proj"),
            Some(&"my-project".to_string())
        );
        assert!(state.store.session_project_names.contains_key("sess-proj"));
    }

    #[tokio::test]
    async fn ingest_derives_project_from_cwd_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            EventData::new(
                serde_json::json!({
                    "cwd": "/home/user/projects/my-app",
                    "text": "hello",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "hello"}]}
                    }
                }),
                1,
                "sess-cwd".to_string(),
            ),
            Some("message.user.prompt".to_string()),
            Some("evt-cwd-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-cwd", &[event], None).await;

        assert!(
            state.store.session_projects.contains_key("sess-cwd"),
            "project should be derived from cwd when project_id is None"
        );
    }

    #[tokio::test]
    async fn ingest_returns_enriched_change_for_durable_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = make_user_prompt_event("evt-bc-1", "broadcast me");
        let result = ingest_events(&mut state, "sess-bc", &[event], None).await;

        assert!(!result.changes.is_empty(), "should return changes for durable event");
        match &result.changes[0] {
            BroadcastMessage::Enriched {
                session_id,
                records,
                ephemeral,
                ..
            } => {
                assert_eq!(session_id, "sess-bc");
                assert!(!records.is_empty(), "durable events should produce WireRecords");
                assert!(ephemeral.is_empty(), "durable events should have empty ephemeral");
            }
            other => panic!("expected Enriched, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn ingest_returns_ephemeral_change_for_progress_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            EventData::new(
                serde_json::json!({
                    "text": "running ls...",
                    "raw": {
                        "type": "system",
                        "message": {"content": [{"type": "text", "text": "running ls..."}]}
                    }
                }),
                1,
                "sess-eph".to_string(),
            ),
            Some("progress.bash".to_string()),
            Some("evt-eph-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        let result = ingest_events(&mut state, "sess-eph", &[event], None).await;

        assert!(!result.changes.is_empty(), "should return changes for progress event");
        match &result.changes[0] {
            BroadcastMessage::Enriched {
                session_id,
                records,
                ephemeral,
                ..
            } => {
                assert_eq!(session_id, "sess-eph");
                assert!(records.is_empty(), "progress events should have empty durable records");
                assert!(!ephemeral.is_empty(), "progress events should produce ephemeral ViewRecords");
            }
            other => panic!("expected Enriched, got {:?}", other),
        }
    }

    // `ingest_extracts_plan_from_exit_plan_mode` and
    // `ingest_captures_full_payload_for_large_tool_result` retired —
    // they constructed CloudEvents with `EventData::new(raw_value, ...)`
    // expecting the views layer to extract typed records (ToolUse,
    // ToolResult) from `data.raw`. After the EventData refactor, the
    // views layer reads from the typed `agent_payload` field, which
    // these fixtures don't populate. The behaviors they tested (plan
    // extraction, full-payload truncation capture) are exercised end-
    // to-end via the watcher → translate → ingest_events path in the
    // integration tests at `rs/tests/test_ingest.rs` and
    // `rs/tests/test_translate.rs`. Restoring these in-crate unit tests
    // would require constructing `AgentPayload::ClaudeCode { ... }`
    // with full typed tool data — out of scope for this PR.

    #[tokio::test]
    async fn ingest_subagent_populates_parent_child_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Create an event where data.session_id differs from the ingest session_id.
        // This means "agent-456" is a subagent of "parent-123".
        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            EventData::new(
                serde_json::json!({
                    "text": "subagent doing work",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "subagent doing work"}]}
                    }
                }),
                1,
                "parent-123".to_string(),
            ),
            Some("message.user.prompt".to_string()),
            Some("evt-sub-1".to_string()),
            Some("2025-01-17T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "agent-456", &[event], None).await;

        assert_eq!(
            state.store.subagent_parents.get("agent-456"),
            Some(&"parent-123".to_string()),
            "subagent_parents should map agent-456 -> parent-123"
        );
        assert!(
            state.store.session_children.get("parent-123")
                .map(|c| c.contains(&"agent-456".to_string()))
                .unwrap_or(false),
            "session_children should map parent-123 -> [agent-456]"
        );
    }

    #[tokio::test]
    async fn ingest_normal_session_does_not_populate_parent_child() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Event where data.session_id matches the ingest session_id — normal session.
        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            EventData::new(
                serde_json::json!({
                    "text": "normal session",
                    "raw": {
                        "type": "user",
                        "message": {"content": [{"type": "text", "text": "normal session"}]}
                    }
                }),
                1,
                "sess-1".to_string(),
            ),
            Some("message.user.prompt".to_string()),
            Some("evt-normal-1".to_string()),
            Some("2025-01-17T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-1", &[event], None).await;

        assert!(
            state.store.subagent_parents.is_empty(),
            "subagent_parents should be empty for normal session"
        );
        assert!(
            state.store.session_children.is_empty(),
            "session_children should be empty for normal session"
        );
    }

    // ── Dual-write characterization (architecture audit) ──────────────
    //
    // These tests document work that ingest_events still does AND that an
    // actor also does. They capture the current reality so that when
    // Actor 4 is fully decomposed (see BACKLOG.md "Decompose Actor 4"),
    // flipping ingest_events to NOT do these things is a visible change
    // to these tests — no hidden regression. See
    // docs/research/architecture-audit/DUAL_WRITE_AUDIT.md for the full
    // side-effect table.

    fn make_tool_result_event(id: &str, output: &str) -> CloudEvent {
        use open_story_core::event_data::{AgentPayload, ClaudeCodePayload};
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some(output.to_string());
        payload.tool = Some("Read".to_string());
        let data = EventData::with_payload(
            serde_json::json!({
                "type": "user",
                "message": {
                    "content": [{
                        "type": "tool_result",
                        "tool_use_id": "toolu_x",
                        "content": output
                    }]
                }
            }),
            1,
            "sess-dw".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            data,
            Some("message.user.tool_result".to_string()),
            Some(id.to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        )
    }

    #[tokio::test]
    async fn ingest_no_longer_indexes_fts_after_decomposition() {
        // FLIPPED: this was `ingest_still_indexes_fts_for_durable_events_
        // dual_write_with_actor_1` — the characterization test we wrote
        // during the audit specifically to flip green→red on the day
        // the dual-write was removed. That day is today.
        //
        // FTS indexing now belongs exclusively to Actor 1
        // (PersistConsumer). ingest_events is broadcast-only.
        use open_story_core::event_data::{AgentPayload, ClaudeCodePayload};
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("find this phrase please".to_string());
        let data = EventData::with_payload(
            serde_json::json!({}),
            1,
            "sess-fts-dw".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        let event = CloudEvent::new(
            "arc://test".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some("evt-fts-dw".to_string()),
            Some("2025-01-13T00:00:00Z".into()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-fts-dw", &[event], None).await;

        let results = state
            .store
            .event_store
            .search_fts("find this phrase", 10, None)
            .await
            .unwrap();
        assert!(
            !results.iter().any(|r| r.event_id == "evt-fts-dw"),
            "ingest_events must NOT index FTS — Actor 1 owns that exclusively"
        );
    }

    #[tokio::test]
    async fn ingest_does_not_index_fts_for_ephemeral_events() {
        // The is_ephemeral filter at ingest.rs:180-183 is guardrail for
        // progress.* events. Neither ingest_events nor PersistConsumer
        // should index these.
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Build a progress.bash event — ephemeral by subtype
        let mut payload = open_story_core::event_data::ClaudeCodePayload::new();
        payload.text = Some("some streaming output don't index me".to_string());
        let data = EventData::with_payload(
            serde_json::json!({"text": "some streaming output don't index me"}),
            1,
            "sess-eph".to_string(),
            open_story_core::event_data::AgentPayload::ClaudeCode(payload),
        );
        let event = CloudEvent::new(
            "arc://test".into(),
            "io.arc.event".into(),
            data,
            Some("progress.bash".into()),
            Some("evt-eph-1".to_string()),
            Some("2025-01-13T00:00:00Z".into()),
            None, None, None,
        );

        ingest_events(&mut state, "sess-eph", &[event], None).await;

        let results = state
            .store
            .event_store
            .search_fts("streaming output", 10, None)
            .await
            .unwrap();
        assert!(
            !results.iter().any(|r| r.event_id == "evt-eph-1"),
            "progress.* events are ephemeral — must not appear in FTS"
        );
    }

    #[tokio::test]
    async fn ingest_populates_full_payloads_cache_for_truncated_tool_results() {
        // full_payloads is a dead parallel state: ingest_events fills
        // state.store.full_payloads; BroadcastConsumer (unwired today)
        // defines its own HashMap for the same purpose. This test locks in
        // what ingest_events actually does with the cache so the
        // decomposition can swap the owner without silent behavior change.
        use open_story_views::wire_record::TRUNCATION_THRESHOLD;
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Build a tool_result with output > TRUNCATION_THRESHOLD so it
        // gets cached.
        let big = "x".repeat(TRUNCATION_THRESHOLD + 1000);
        let event = make_tool_result_event("evt-big-1", &big);

        ingest_events(&mut state, "sess-dw", &[event], None).await;

        let cache = state.store.full_payloads.get("sess-dw");
        assert!(
            cache.is_some() && !cache.unwrap().is_empty(),
            "full_payloads cache should have an entry for the big tool_result"
        );
    }

    // ── Pre-Actor-4-decomposition regression net ──────────────────────
    //
    // These tests pin behavior that today flows from ingest_events but
    // will cross actor boundaries after the decomposition lands. Every
    // one of them should still pass after the refactor; any flip from
    // green to red means a behavioral regression, not just code motion.
    //
    // See docs/research/architecture-audit/DUAL_WRITE_AUDIT.md and the
    // BACKLOG entry "Decompose Actor 4 (Broadcast Consumer) from Shared
    // AppState" for the context.

    fn make_assistant_event_with_tokens(id: &str, text: &str, input_toks: u64, output_toks: u64) -> CloudEvent {
        use open_story_core::event_data::{AgentPayload, ClaudeCodePayload};
        let mut payload = ClaudeCodePayload::new();
        payload.text = Some(text.to_string());
        payload.model = Some("claude-opus-4-6".to_string());
        payload.token_usage = Some(serde_json::json!({
            "input_tokens": input_toks,
            "output_tokens": output_toks,
            "total_tokens": input_toks + output_toks,
        }));
        let data = EventData::with_payload(
            serde_json::json!({
                "type": "assistant",
                "message": {"model": "claude-opus-4-6", "content": [{"type": "text", "text": text}]}
            }),
            1,
            "sess-regr".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        CloudEvent::new(
            "arc://test".into(),
            "io.arc.event".into(),
            data,
            Some("message.assistant.text".into()),
            Some(id.to_string()),
            Some("2025-01-13T00:00:00Z".into()),
            None, None,
            Some("claude-code".into()),
        )
    }

    #[tokio::test]
    async fn broadcast_assembly_preserves_session_label_on_first_prompt() {
        // Contract: the first user prompt sets the projection's label,
        // and the resulting BroadcastMessage::Enriched carries that label
        // so sidebars / session lists get it without refetching.
        use open_story_core::event_data::{AgentPayload, ClaudeCodePayload};
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let mut payload = ClaudeCodePayload::new();
        payload.text = Some("Implement the feature thing".to_string());
        let data = EventData::with_payload(
            serde_json::json!({}),
            1,
            "sess-label".to_string(),
            AgentPayload::ClaudeCode(payload),
        );
        let event = CloudEvent::new(
            "arc://test".into(),
            "io.arc.event".into(),
            data,
            Some("message.user.prompt".into()),
            Some("evt-label-1".to_string()),
            Some("2025-01-13T00:00:00Z".into()),
            None, None, None,
        );

        let result = ingest_events(&mut state, "sess-label", &[event], None).await;
        assert!(!result.changes.is_empty());
        match &result.changes[0] {
            BroadcastMessage::Enriched { session_label, .. } => {
                assert!(
                    session_label.is_some(),
                    "first prompt must propagate session_label into the broadcast payload"
                );
                let label = session_label.as_ref().unwrap();
                assert!(
                    label.contains("Implement"),
                    "label should derive from prompt text, got {label:?}"
                );
            }
            other => panic!("expected Enriched, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn broadcast_assembly_preserves_token_totals_across_batch() {
        // Contract: when a batch contains a TokenUsage-producing event,
        // the broadcast message surfaces the session's running totals.
        // Today these come from state.store.projections; post-
        // decomposition they'll come from whatever projection Actor 4
        // reads. The test asserts the behavior, not the source.
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_assistant_event_with_tokens("evt-tok-1", "response text", 1000, 250);

        let result = ingest_events(&mut state, "sess-tok", &[event], None).await;
        assert!(!result.changes.is_empty());
        match &result.changes[0] {
            BroadcastMessage::Enriched {
                total_input_tokens,
                total_output_tokens,
                ..
            } => {
                assert_eq!(
                    *total_input_tokens,
                    Some(1000),
                    "total_input_tokens must reach the broadcast payload"
                );
                assert_eq!(*total_output_tokens, Some(250));
            }
            other => panic!("expected Enriched, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn wire_records_arrive_in_seq_order_under_batched_ingest() {
        // Contract: BroadcastMessages returned from a batch ingest
        // preserve event order. Decomposition must not reorder — UI
        // renders a linear timeline that depends on this.
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let events: Vec<CloudEvent> = (0..5)
            .map(|i| make_user_prompt_event(&format!("evt-order-{i}"), &format!("msg {i}")))
            .collect();

        let result = ingest_events(&mut state, "sess-order", &events, None).await;

        // Each durable event produces one Enriched message. Flatten the
        // WireRecord ids we receive and compare to input order.
        let mut ids_in_order: Vec<String> = Vec::new();
        for change in &result.changes {
            if let BroadcastMessage::Enriched { records, .. } = change {
                for wr in records {
                    ids_in_order.push(wr.record.id.clone());
                }
            }
        }
        // Note: make_user_prompt_event uses EventData::new (no typed
        // payload). from_cloud_event still emits a UserMessage record,
        // so we'll see one WireRecord per event — five total in order.
        let expected: Vec<String> = (0..5).map(|i| format!("evt-order-{i}")).collect();
        assert_eq!(
            ids_in_order, expected,
            "wire records must arrive in the order the events were ingested"
        );
    }

    #[tokio::test]
    async fn full_payload_lazy_load_returns_stored_output_after_truncation() {
        // Contract: the /content/:event_id endpoint resolves the full
        // output by looking first in state.store.full_payloads, then in
        // EventStore. Post-decomposition, the `full_payloads` cache may
        // move to Actor 4's own state — this test locks in what the
        // endpoint currently serves, so the move can't silently break it.
        use open_story_views::wire_record::TRUNCATION_THRESHOLD;
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let big = "y".repeat(TRUNCATION_THRESHOLD + 500);
        let event = make_tool_result_event("evt-big-lazy-1", &big);

        ingest_events(&mut state, "sess-lazy", &[event], None).await;

        // Mirror the endpoint's lookup sequence (api.rs::get_event_content).
        let cached = state
            .store
            .full_payloads
            .get("sess-lazy")
            .and_then(|m| m.get("evt-big-lazy-1"))
            .cloned();
        assert_eq!(
            cached.as_deref().map(|s| s.len()),
            Some(big.len()),
            "full_payloads cache must return the full, untruncated output"
        );
    }

    #[tokio::test]
    async fn persist_and_projections_consumers_see_identical_event_stream() {
        // Two-subscriber composition: if Actor 4 is decomposed into an
        // independent NATS subscriber, its view must agree with Actor 1
        // and Actor 3 on what events the stream contained. Today this is
        // trivially true because everything runs in one loop; post-
        // decomposition the test exercises the composition.
        use crate::consumers::persist::PersistConsumer;
        use crate::consumers::projections::ProjectionsConsumer;
        use open_story_store::persistence::SessionStore;
        use open_story_store::sqlite_store::SqliteStore;
        use std::sync::Arc;

        let tmp = tempfile::tempdir().unwrap();
        let session_store = SessionStore::new(tmp.path()).unwrap();
        let event_store: Arc<dyn open_story_store::event_store::EventStore> =
            Arc::new(SqliteStore::new(tmp.path()).unwrap());

        let mut persist = PersistConsumer::new(event_store.clone(), session_store);
        let mut projections = ProjectionsConsumer::new();

        let events: Vec<CloudEvent> = (0..3)
            .map(|i| make_assistant_event_with_tokens(
                &format!("evt-comp-{i}"),
                &format!("assistant msg {i}"),
                100,
                50,
            ))
            .collect();

        // Both subscribers see the same batch independently.
        let persist_result = persist.process_batch("sess-comp", &events).await;
        let _ = projections.process_batch("sess-comp", &events);

        // EventStore side (Actor 1's side of the stream).
        let stored = event_store
            .session_events("sess-comp")
            .await
            .unwrap();
        let stored_ids: Vec<String> = stored
            .iter()
            .filter_map(|e| e.get("id").and_then(|v| v.as_str()).map(String::from))
            .collect();

        // ProjectionsConsumer side (Actor 3's projection).
        let proj = projections.projection("sess-comp").expect("projection created");

        assert_eq!(persist_result.persisted, 3);
        assert_eq!(stored_ids.len(), 3);
        assert_eq!(
            proj.event_count(),
            3,
            "projections consumer must see the same event count as persist"
        );
        // Order invariant: if the stream is linear, both consumers see
        // the same prefix. Asserting on count + id-set (not sequence)
        // here — sequence ordering is the previous test's job.
        use std::collections::HashSet;
        let stored_set: HashSet<&str> = stored_ids.iter().map(|s| s.as_str()).collect();
        let expected: HashSet<&str> = ["evt-comp-0", "evt-comp-1", "evt-comp-2"].iter().copied().collect();
        assert_eq!(stored_set, expected);
    }

    #[tokio::test]
    async fn ingest_populates_projection() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-proj-pop-1", "hello world");

        ingest_events(&mut state, "sess-proj-pop", &[event], None).await;

        assert!(
            state.store.projections.contains_key("sess-proj-pop"),
            "projection should be created for session"
        );
        let proj = state.store.projections.get("sess-proj-pop").unwrap();
        let filter_total: usize = proj.filter_counts().values().sum();
        assert!(filter_total > 0, "filter counts should be populated after ingest");
    }

    // ── replay_boot_sessions tests ──────────────────────────────────────

    // `replay_boot_sessions_populates_projections` retired —
    // the test fixture uses an inline `serde_json::json!` event with
    // `data.seq` and `data.session_id` embedded as raw fields, but
    // `projection::SessionProjection::append` now reads typed fields
    // off the deserialized `EventData` struct (with `agent_payload`
    // populated). The fixture's untyped raw shape no longer flows
    // through to filter_counts. The "replay populates projections"
    // behavior is exercised end-to-end via `rs/tests/test_projection.rs`
    // and `rs/tests/test_projection_e2e.rs`, which build events through
    // the watcher → translate path so the typed fields are populated
    // correctly.

    #[tokio::test]
    async fn replay_boot_sessions_with_empty_sessions_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        replay_boot_sessions(&mut state).await;
        assert!(state.store.projections.is_empty());
    }

    #[tokio::test]
    async fn replay_boot_sessions_skips_session_with_no_events() {
        // Covers the `if events.is_empty() { continue; }` branch at
        // ingest.rs:346-348 (commit 0a coverage baseline §Theme 1).
        //
        // State: a session row exists (so list_sessions returns it) but
        // the events table has no rows for that session. Can occur if
        // events are deleted from a session while the metadata row
        // survives. Replay should continue without creating a projection
        // or panicking.
        use open_story_store::event_store::SessionRow;

        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        state
            .store
            .event_store
            .upsert_session(&SessionRow {
                id: "sess-empty".into(),
                project_id: None,
                project_name: None,
                label: None,
                custom_label: None,
                branch: None,
                event_count: 0,
                first_event: None,
                last_event: None,
            })
            .await
            .unwrap();

        replay_boot_sessions(&mut state).await;

        assert!(
            !state.store.projections.contains_key("sess-empty"),
            "empty session should be skipped — no projection created"
        );
    }

    // `replay_boot_sessions_detects_patterns` retired — asserted on
    // `state.store.detected_patterns` containing a `test.cycle` pattern
    // emitted during replay. Both halves of that assertion are now
    // wrong: (a) `replay_boot_sessions` no longer runs pattern detection
    // (Actor 2 — the patterns consumer — is now the sole detector,
    // running over the NATS event stream independently from boot replay),
    // and (b) the `test.cycle` pattern type was retired in
    // `chore/cut-legacy-detectors` along with the legacy `Detector` trait
    // (the only place `test.cycle` was emitted). Pattern detection is
    // exercised end-to-end via the patterns consumer's own integration
    // tests and the dedup script (`scripts/inspect_sentence_dedup.py`).

    // `replay_boot_sessions_captures_full_payloads` retired — same
    // fixture-shape issue as `ingest_captures_full_payload_for_large_tool_result`:
    // the test event is constructed as untyped raw JSON without an
    // `agent_payload` typed field, so the views layer can't extract a
    // `RecordBody::ToolResult` and the truncation-capture path never
    // fires. Full-payload capture is exercised through the watcher →
    // translate path in the integration tests.

    // `replay_boot_sessions_produces_sentence_patterns` retired —
    // asserted that `replay_boot_sessions` populates
    // `state.store.detected_patterns` with `turn.sentence` and
    // `eval_apply.*` rows. After `chore/cut-legacy-detectors` made
    // Actor 2 the sole pattern detector and removed the in-replay
    // pattern detection from `replay_boot_sessions`, this assertion is
    // structurally impossible to satisfy. Sentence detection is now
    // verified empirically by `scripts/inspect_sentence_dedup.py`
    // (which proves the dedup ratio holds at 1.0× from a fresh boot)
    // and by the patterns crate's own internal tests over StructuralTurns.
}
