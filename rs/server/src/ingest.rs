//! Event ingestion pipeline — dedup, persist, project, pattern-detect, broadcast.
//!
//! Pure functions (is_plan_event, extract_plan_content, to_wire_record) live in
//! open-story-store::ingest and are re-exported here. This module retains the
//! stateful orchestration (ingest_events, replay_boot_sessions) that depends on AppState.

use std::collections::HashMap;

use open_story_views::from_cloud_event::from_cloud_event;
use open_story_views::unified::RecordBody;
use open_story_views::wire_record::{WireRecord, TRUNCATION_THRESHOLD};

use open_story_core::cloud_event::CloudEvent;
use open_story_patterns::FeedContext;
use open_story_semantic::worker::EmbedRequest;
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
pub fn ingest_events(
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
        if let Ok(val) = serde_json::to_value(ce) {
            // Dedup: skip events we've already seen
            let event_id = val.get("id").and_then(|v| v.as_str()).unwrap_or("");
            if !event_id.is_empty() && !state.store.seen_event_ids.insert(event_id.to_string()) {
                continue;
            }

            // Detect subagent → parent relationship.
            // The event's data.session_id comes from the transcript's sessionId field,
            // which is the PARENT session. The session_id parameter is the subagent's
            // own ID (from filename or hook payload). When they differ, record the mapping.
            if let Some(data_sid) = val.get("data")
                .and_then(|d| d.get("session_id"))
                .and_then(|v| v.as_str())
            {
                if data_sid != session_id && !state.store.subagent_parents.contains_key(session_id) {
                    state.store.subagent_parents.insert(session_id.to_string(), data_sid.to_string());
                    state.store.session_children
                        .entry(data_sid.to_string())
                        .or_default()
                        .push(session_id.to_string());
                }
            }

            let _ = state.store.session_store.append(session_id, &val);
            // Persist to EventStore (SQLite default, JSONL fallback)
            let _ = state.store.event_store.insert_event(session_id, &val);

            // Update projection
            let proj = state
                .store
                .projections
                .entry(session_id.to_string())
                .or_insert_with(|| projection::SessionProjection::new(session_id));
            let append_result = proj.append(&val);

            // Plan extraction
            if is_plan_event(&val) {
                let plan_content = extract_plan_content(&val).or_else(|| {
                    val.get("data")
                        .and_then(|d| d.get("args"))
                        .and_then(|a| a.get("plan"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                });
                if let Some(content) = plan_content {
                    let timestamp = val.get("time").and_then(|v| v.as_str()).unwrap_or("");
                    let _ = state.store.plan_store.save(session_id, &content, timestamp);
                    // Dual-write plan to EventStore
                    let plan_id = format!("plan:{}:{}", session_id, timestamp);
                    let _ = state.store.event_store.upsert_plan(&plan_id, session_id, &content);
                }
            }

            // BFF transform: CloudEvent → typed ViewRecords for the UI
            let view_records = from_cloud_event(&val);

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

            // Feed ViewRecords to pattern pipeline (durable events only)
            let mut detected_patterns = Vec::new();
            if !ephemeral && !view_records.is_empty() {
                let pipeline = state
                    .store
                    .pattern_pipelines
                    .entry(session_id.to_string())
                    .or_default();
                for vr in &view_records {
                    let depth = proj.node_depth(&vr.id);
                    let parent_uuid_owned = proj.node_parent(&vr.id).map(|s| s.to_string());
                    let ctx = FeedContext {
                        record: vr,
                        depth,
                        parent_uuid: parent_uuid_owned.as_deref(),
                    };
                    detected_patterns.extend(pipeline.feed(&ctx));
                }
                // Store detected patterns for initial_state
                if !detected_patterns.is_empty() {
                    // Extract agent labels from delegation patterns
                    for pe in &detected_patterns {
                        if pe.pattern_type == "agent.delegation" {
                            if let Some(desc) =
                                pe.metadata.get("description").and_then(|v| v.as_str())
                            {
                                if !desc.is_empty() {
                                    if let Some(first_id) = pe.event_ids.first() {
                                        state
                                            .store
                                            .agent_labels
                                            .insert(first_id.clone(), desc.to_string());
                                    }
                                }
                            }
                        }
                        // Dual-write pattern to EventStore
                        let _ = state.store.event_store.insert_pattern(session_id, pe);
                    }
                    state
                        .store
                        .detected_patterns
                        .entry(session_id.to_string())
                        .or_default()
                        .extend(detected_patterns.clone());
                }
            }

            // Send embeddable records to the embedding worker (non-blocking)
            if let Some(tx) = &state.embedding_tx {
                if !ephemeral {
                    for vr in &view_records {
                        let req = EmbedRequest {
                            session_id: session_id.to_string(),
                            record: vr.clone(),
                        };
                        if tx.try_send(req).is_err() {
                            // Channel full or closed — log once and move on.
                            // Embedding must never block ingest.
                        }
                    }
                }
            }

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
            // Collect new agent labels from delegation patterns
            let mut new_agent_labels = HashMap::new();
            for pe in &detected_patterns {
                if pe.pattern_type == "agent.delegation" {
                    if let Some(desc) = pe.metadata.get("description").and_then(|v| v.as_str()) {
                        if let Some(first_id) = pe.event_ids.first() {
                            new_agent_labels.insert(first_id.clone(), desc.to_string());
                        }
                    }
                }
            }

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
                        agent_labels: HashMap::new(),
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
                        agent_labels: new_agent_labels,
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
            );
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
pub fn replay_boot_sessions(state: &mut AppState) {
    let session_ids: Vec<String> = state.store.event_store
        .list_sessions()
        .unwrap_or_default()
        .iter()
        .map(|r| r.id.clone())
        .collect();
    let mut total_events = 0;
    let mut total_patterns = 0;

    for sid in &session_ids {
        let events = state.store.event_store
            .session_events(sid)
            .unwrap_or_default();
        if events.is_empty() {
            continue;
        }

        for val in &events {
            // Events are already in EventStore (that's where we read them from).
            // No need to re-insert — just replay through projections and patterns.

            // Detect subagent → parent relationship during replay
            if let Some(data_sid) = val.get("data")
                .and_then(|d| d.get("session_id"))
                .and_then(|v| v.as_str())
            {
                if data_sid != sid && !state.store.subagent_parents.contains_key(sid.as_str()) {
                    state.store.subagent_parents.insert(sid.clone(), data_sid.to_string());
                    state.store.session_children
                        .entry(data_sid.to_string())
                        .or_default()
                        .push(sid.clone());
                }
            }

            // Update projection
            let proj = state
                .store
                .projections
                .entry(sid.clone())
                .or_insert_with(|| projection::SessionProjection::new(sid));
            proj.append(val);

            // BFF transform
            let view_records = from_cloud_event(val);

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

            // Feed to pattern pipeline (skip ephemeral)
            let subtype = val.get("subtype").and_then(|v| v.as_str());
            let ephemeral = projection::is_ephemeral(subtype);

            if !ephemeral && !view_records.is_empty() {
                let proj = state.store.projections.get(sid).unwrap();
                let pipeline = state
                    .store
                    .pattern_pipelines
                    .entry(sid.clone())
                    .or_default();
                for vr in &view_records {
                    let depth = proj.node_depth(&vr.id);
                    let parent_uuid_owned = proj.node_parent(&vr.id).map(|s| s.to_string());
                    let ctx = FeedContext {
                        record: vr,
                        depth,
                        parent_uuid: parent_uuid_owned.as_deref(),
                    };
                    let detected = pipeline.feed(&ctx);
                    if !detected.is_empty() {
                        total_patterns += detected.len();
                        for pe in &detected {
                            let _ = state.store.event_store.insert_pattern(sid, pe);
                        }
                        state
                            .store
                            .detected_patterns
                            .entry(sid.clone())
                            .or_default()
                            .extend(detected);
                    }
                }
            }

            total_events += 1;
        }

        // Flush remaining patterns from each session's pipeline
        if let Some(pipeline) = state.store.pattern_pipelines.get_mut(sid) {
            let flushed = pipeline.flush();
            if !flushed.is_empty() {
                total_patterns += flushed.len();
                for pe in &flushed {
                    let _ = state.store.event_store.insert_pattern(sid, pe);
                }
                state
                    .store
                    .detected_patterns
                    .entry(sid.clone())
                    .or_default()
                    .extend(flushed);
            }
        }

        // Dual-write session projection after processing all events
        if let Some(proj) = state.store.projections.get(sid) {
            let _ = state.store.event_store.upsert_session(
                &crate::event_store_bridge::session_row_from_projection(sid, proj, &state.store),
            );
        }
    }

    if total_events > 0 {
        crate::logging::log_event(
            "boot",
            &format!(
                "replayed {} events across {} sessions ({} patterns detected)",
                total_events,
                session_ids.len(),
                total_patterns,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use open_story_bus::noop_bus::NoopBus;
    use open_story_semantic::NoopSemanticStore;
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
            semantic_store: Arc::new(NoopSemanticStore),
            embedding_tx: None,
            embedder: None,
            config: crate::config::Config::default(),
            watch_dir,
        }
    }

    fn make_user_prompt_event(id: &str, text: &str) -> CloudEvent {
        CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-1",
                "text": text,
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": text}]}
                }
            }),
            Some("message.user.prompt".to_string()),
            Some(id.to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        )
    }

    // ── ingest_events tests ─────────────────────────────────────────────

    #[test]
    fn ingest_empty_events_returns_zero() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let result = ingest_events(&mut state, "sess-1", &[], None);
        assert_eq!(result.count, 0);
        assert!(state.store.event_store.list_sessions().unwrap().is_empty());
    }

    #[test]
    fn ingest_deduplicates_by_event_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-dup-1", "hello");

        let result1 = ingest_events(&mut state, "sess-1", &[event.clone()], None);
        assert_eq!(result1.count, 1);

        let result2 = ingest_events(&mut state, "sess-1", &[event], None);
        assert_eq!(result2.count, 0, "duplicate event should be skipped");

        assert_eq!(state.store.event_store.session_events("sess-1").unwrap().len(), 1);
    }

    #[test]
    fn ingest_persists_to_session_store() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-persist-1", "persist me");

        ingest_events(&mut state, "sess-persist", &[event], None);

        // In-memory sessions
        assert!(!state.store.event_store.session_events("sess-persist").unwrap().is_empty());
        assert_eq!(state.store.event_store.session_events("sess-persist").unwrap().len(), 1);

        // Persisted to store (load from disk)
        let loaded = state.store.session_store.load_session("sess-persist");
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded[0].get("id").and_then(|v| v.as_str()),
            Some("evt-persist-1")
        );
    }

    #[test]
    fn ingest_associates_project_id() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-proj-1", "hello");

        ingest_events(&mut state, "sess-proj", &[event], Some("my-project"));

        assert_eq!(
            state.store.session_projects.get("sess-proj"),
            Some(&"my-project".to_string())
        );
        assert!(state.store.session_project_names.contains_key("sess-proj"));
    }

    #[test]
    fn ingest_derives_project_from_cwd_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-cwd",
                "cwd": "/home/user/projects/my-app",
                "text": "hello",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "hello"}]}
                }
            }),
            Some("message.user.prompt".to_string()),
            Some("evt-cwd-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-cwd", &[event], None);

        assert!(
            state.store.session_projects.contains_key("sess-cwd"),
            "project should be derived from cwd when project_id is None"
        );
    }

    #[test]
    fn ingest_returns_enriched_change_for_durable_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = make_user_prompt_event("evt-bc-1", "broadcast me");
        let result = ingest_events(&mut state, "sess-bc", &[event], None);

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

    #[test]
    fn ingest_returns_ephemeral_change_for_progress_events() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-eph",
                "text": "running ls...",
                "raw": {
                    "type": "system",
                    "message": {"content": [{"type": "text", "text": "running ls..."}]}
                }
            }),
            Some("progress.bash".to_string()),
            Some("evt-eph-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        let result = ingest_events(&mut state, "sess-eph", &[event], None);

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

    #[test]
    fn ingest_extracts_plan_from_exit_plan_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-plan",
                "tool": "ExitPlanMode",
                "args": { "plan": "# My Plan\n\nStep 1: do things" },
                "raw": {
                    "type": "assistant",
                    "message": {
                        "model": "claude-4",
                        "content": [{
                            "type": "tool_use",
                            "id": "toolu_plan",
                            "name": "ExitPlanMode",
                            "input": { "plan": "# My Plan\n\nStep 1: do things" }
                        }]
                    }
                }
            }),
            Some("message.assistant.tool_use".to_string()),
            Some("evt-plan-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-plan", &[event], None);

        let plans = state.store.plan_store.list_plans();
        assert!(
            plans.iter().any(|p| p.session_id == "sess-plan"),
            "plan should be extracted and stored"
        );
    }

    #[test]
    fn ingest_captures_full_payload_for_large_tool_result() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let large_output = "x".repeat(TRUNCATION_THRESHOLD + 500);
        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-trunc",
                "call_id": "toolu_big",
                "output": large_output,
                "is_error": false,
                "raw": {
                    "type": "tool_result",
                    "message": {
                        "content": [{"type": "tool_result", "tool_use_id": "toolu_big", "content": large_output, "is_error": false}]
                    }
                }
            }),
            Some("message.user.tool_result".to_string()),
            Some("evt-trunc-1".to_string()),
            Some("2025-01-13T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-trunc", &[event], None);

        let session_payloads = state.store.full_payloads.get("sess-trunc");
        assert!(
            session_payloads.is_some(),
            "full_payloads should have entry for session with large output"
        );
        let payloads = session_payloads.unwrap();
        assert!(!payloads.is_empty(), "should have captured at least one full payload");
        let captured = payloads.values().next().unwrap();
        assert_eq!(captured.len(), TRUNCATION_THRESHOLD + 500);
    }

    #[test]
    fn ingest_subagent_populates_parent_child_index() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Create an event where data.session_id differs from the ingest session_id.
        // This means "agent-456" is a subagent of "parent-123".
        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "parent-123",
                "text": "subagent doing work",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "subagent doing work"}]}
                }
            }),
            Some("message.user.prompt".to_string()),
            Some("evt-sub-1".to_string()),
            Some("2025-01-17T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "agent-456", &[event], None);

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

    #[test]
    fn ingest_normal_session_does_not_populate_parent_child() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        // Event where data.session_id matches the ingest session_id — normal session.
        let event = CloudEvent::new(
            "arc://test".to_string(),
            "io.arc.event".to_string(),
            serde_json::json!({
                "seq": 1,
                "session_id": "sess-1",
                "text": "normal session",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "normal session"}]}
                }
            }),
            Some("message.user.prompt".to_string()),
            Some("evt-normal-1".to_string()),
            Some("2025-01-17T00:00:00Z".to_string()),
            None,
            None,
            None,
        );

        ingest_events(&mut state, "sess-1", &[event], None);

        assert!(
            state.store.subagent_parents.is_empty(),
            "subagent_parents should be empty for normal session"
        );
        assert!(
            state.store.session_children.is_empty(),
            "session_children should be empty for normal session"
        );
    }

    #[test]
    fn ingest_populates_projection() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);
        let event = make_user_prompt_event("evt-proj-pop-1", "hello world");

        ingest_events(&mut state, "sess-proj-pop", &[event], None);

        assert!(
            state.store.projections.contains_key("sess-proj-pop"),
            "projection should be created for session"
        );
        let proj = state.store.projections.get("sess-proj-pop").unwrap();
        let filter_total: usize = proj.filter_counts().values().sum();
        assert!(filter_total > 0, "filter counts should be populated after ingest");
    }

    // ── replay_boot_sessions tests ──────────────────────────────────────

    #[test]
    fn replay_boot_sessions_populates_projections() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let event = serde_json::json!({
            "id": "evt-1",
            "type": "io.arc.event",
            "subtype": "message.user.prompt",
            "source": "arc://transcript/sess-1",
            "time": "2025-01-13T00:00:00Z",
            "data": {
                "seq": 1,
                "session_id": "sess-1",
                "text": "Hello world",
                "raw": {
                    "type": "user",
                    "message": {"content": [{"type": "text", "text": "Hello world"}]}
                }
            }
        });
        let _ = state.store.event_store.insert_event("sess-1", &event);
        let _ = state.store.event_store.upsert_session(&open_story_store::event_store::SessionRow {
            id: "sess-1".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
        });

        assert!(state.store.projections.is_empty());

        replay_boot_sessions(&mut state);

        assert!(state.store.projections.contains_key("sess-1"));
        let proj = state.store.projections.get("sess-1").unwrap();
        assert!(proj.filter_counts().values().sum::<usize>() > 0);
    }

    #[test]
    fn replay_boot_sessions_with_empty_sessions_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        replay_boot_sessions(&mut state);
        assert!(state.store.projections.is_empty());
    }

    #[test]
    fn replay_boot_sessions_detects_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let events: Vec<serde_json::Value> = vec![
            serde_json::json!({
                "id": "tc-1", "type": "io.arc.event", "subtype": "message.assistant.tool_use",
                "source": "arc://test", "time": "2025-01-13T00:00:00Z",
                "data": { "tool": "Bash", "args": {"command": "cargo test"},
                    "raw": {"type": "assistant", "message": {"model": "claude-4", "content": [
                        {"type": "tool_use", "id": "toolu_test1", "name": "Bash", "input": {"command": "cargo test"}}
                    ]}}}
            }),
            serde_json::json!({
                "id": "tc-2", "type": "io.arc.event", "subtype": "message.user.tool_result",
                "source": "arc://test", "time": "2025-01-13T00:00:01Z",
                "data": { "raw": {"type": "user", "message": {"content": [
                    {"type": "tool_result", "tool_use_id": "toolu_test1", "content": "test result: FAILED. 1 passed; 2 failed"}
                ]}}}
            }),
            serde_json::json!({
                "id": "tc-3", "type": "io.arc.event", "subtype": "message.assistant.tool_use",
                "source": "arc://test", "time": "2025-01-13T00:00:02Z",
                "data": { "tool": "Edit", "args": {"file": "src/lib.rs"},
                    "raw": {"type": "assistant", "message": {"model": "claude-4", "content": [
                        {"type": "tool_use", "id": "toolu_edit1", "name": "Edit", "input": {"file": "src/lib.rs"}}
                    ]}}}
            }),
            serde_json::json!({
                "id": "tc-4", "type": "io.arc.event", "subtype": "message.assistant.tool_use",
                "source": "arc://test", "time": "2025-01-13T00:00:03Z",
                "data": { "tool": "Bash", "args": {"command": "cargo test"},
                    "raw": {"type": "assistant", "message": {"model": "claude-4", "content": [
                        {"type": "tool_use", "id": "toolu_test2", "name": "Bash", "input": {"command": "cargo test"}}
                    ]}}}
            }),
            serde_json::json!({
                "id": "tc-5", "type": "io.arc.event", "subtype": "message.user.tool_result",
                "source": "arc://test", "time": "2025-01-13T00:00:04Z",
                "data": { "raw": {"type": "user", "message": {"content": [
                    {"type": "tool_result", "tool_use_id": "toolu_test2", "content": "test result: ok. 3 passed; exit code 0"}
                ]}}}
            }),
        ];

        let _ = state.store.event_store.insert_batch("sess-tc", &events);
        let _ = state.store.event_store.upsert_session(&open_story_store::event_store::SessionRow {
            id: "sess-tc".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: events.len() as u64,
                custom_label: None,
            first_event: None, last_event: None,
        });
        replay_boot_sessions(&mut state);

        let patterns = state.store.detected_patterns.get("sess-tc");
        assert!(patterns.is_some(), "should have detected patterns during replay");
        let test_cycles: Vec<_> = patterns.unwrap()
            .iter()
            .filter(|p| p.pattern_type == "test.cycle")
            .collect();
        assert!(!test_cycles.is_empty(), "should have detected at least one test.cycle pattern");
        assert!(test_cycles[0].summary.contains("PASS"));
    }

    #[test]
    fn replay_boot_sessions_captures_full_payloads() {
        let tmp = tempfile::tempdir().unwrap();
        let mut state = test_app_state(&tmp);

        let large_output = "z".repeat(TRUNCATION_THRESHOLD + 200);
        let event = serde_json::json!({
            "id": "replay-trunc-1",
            "type": "io.arc.event",
            "subtype": "message.user.tool_result",
            "source": "arc://test",
            "time": "2025-01-13T00:00:00Z",
            "data": {
                "raw": {"type": "user", "message": {"content": [
                    {"type": "tool_result", "tool_use_id": "toolu_replay", "content": large_output}
                ]}}
            }
        });

        let _ = state.store.event_store.insert_event("sess-replay-trunc", &event);
        let _ = state.store.event_store.upsert_session(&open_story_store::event_store::SessionRow {
            id: "sess-replay-trunc".into(), project_id: None, project_name: None,
            label: None, branch: None, event_count: 1,
                custom_label: None,
            first_event: None, last_event: None,
        });
        replay_boot_sessions(&mut state);

        let payloads = state.store.full_payloads.get("sess-replay-trunc");
        assert!(payloads.is_some(), "replay should capture full payloads for large tool results");
        let payloads = payloads.unwrap();
        assert!(!payloads.is_empty());
        assert_eq!(payloads.values().next().unwrap().len(), TRUNCATION_THRESHOLD + 200);
    }
}
