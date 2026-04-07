//! Query module — sovereignty-serving queries over the event store.
//!
//! Each query answers a specific question that helps a creative understand
//! their story. Three consumers: CLI (human), API (dashboard), Agent (MCP/skill).
//!
//! All query functions take a `&Connection` and return serializable results.
//! They are pure read-only SQL queries — no mutation, no side effects.

use rusqlite::Connection;
use serde::Serialize;

// ── Canonical timestamp format ───────────────────────────────────────
//
// The translator (`rs/core/src/translate.rs:473` and
// `translate_pi.rs:330`) emits CloudEvent `time` fields exactly in this
// format — it pass-throughs the JSONL `timestamp` field unchanged, and
// both Claude Code and pi-mono produce ISO 8601 UTC with millisecond
// precision and `Z` suffix. See §1.5 of
// `docs/research/mongo-analytics-parity-plan.md` for the verification.
//
// Cutoffs and any test fixture timestamps MUST use this constant.
// Never use `chrono::DateTime::to_rfc3339()` here — it produces a
// `+00:00` suffix that is NOT byte-equal to stored values, and the
// resulting lexical comparison only works by ASCII collation accident
// (`Z > +`). See §6.8 of the same plan.
pub(crate) const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Format a `DateTime<Utc>` in the canonical translator format.
pub(crate) fn format_ts(dt: chrono::DateTime<chrono::Utc>) -> String {
    dt.format(TS_FORMAT).to_string()
}

// ── FTS5 Search ────────────────────────────────────────────────────

/// Result from a full-text search query.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FtsSearchResult {
    pub event_id: String,
    pub session_id: String,
    pub record_type: String,
    pub snippet: String,
    pub rank: f64,
}

// ── Session Narrative Queries ───────────────────────────────────────

/// Synopsis of a session: goal, journey, outcome.
/// "Did I achieve what I set out to do?"
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionSynopsis {
    pub session_id: String,
    pub label: Option<String>,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub event_count: u64,
    pub tool_count: u64,
    pub error_count: u64,
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    pub duration_secs: Option<i64>,
    pub top_tools: Vec<ToolCount>,
}

/// Tool usage count for a session.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolCount {
    pub tool: String,
    pub count: u64,
}

/// Query session synopsis from SQLite.
pub fn session_synopsis(conn: &Connection, session_id: &str) -> Option<SessionSynopsis> {
    let row = conn
        .query_row(
            "SELECT id, label, project_id, project_name, event_count, first_event, last_event
             FROM sessions WHERE id = ?1",
            [session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                ))
            },
        )
        .ok()?;

    let tool_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND subtype = 'message.assistant.tool_use'",
            [session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let error_count: u64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND subtype = 'system.error'",
            [session_id],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let duration_secs = match (&row.5, &row.6) {
        (Some(first), Some(last)) => {
            let f = chrono::DateTime::parse_from_rfc3339(first).ok()?;
            let l = chrono::DateTime::parse_from_rfc3339(last).ok()?;
            Some((l - f).num_seconds())
        }
        _ => None,
    };

    let top_tools = session_top_tools(conn, session_id, 5);

    Some(SessionSynopsis {
        session_id: row.0,
        label: row.1,
        project_id: row.2,
        project_name: row.3,
        event_count: row.4,
        tool_count,
        error_count,
        first_event: row.5,
        last_event: row.6,
        duration_secs,
        top_tools,
    })
}

/// Top N tools used in a session by frequency.
fn session_top_tools(conn: &Connection, session_id: &str, limit: usize) -> Vec<ToolCount> {
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(payload, '$.data.agent_payload.tool') as tool, COUNT(*) as cnt
             FROM events
             WHERE session_id = ?1 AND subtype = 'message.assistant.tool_use'
               AND tool IS NOT NULL
             GROUP BY tool
             ORDER BY cnt DESC
             LIMIT ?2",
        )
        .unwrap();

    stmt.query_map(rusqlite::params![session_id, limit], |row| {
        Ok(ToolCount {
            tool: row.get(0)?,
            count: row.get(1)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Tool journey: sequence of tools used with file targets.
/// "What strategy did the agent use?"
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolStep {
    pub tool: String,
    pub file: Option<String>,
    pub timestamp: String,
}

/// Query the tool usage sequence for a session.
pub fn tool_journey(conn: &Connection, session_id: &str) -> Vec<ToolStep> {
    let mut stmt = conn
        .prepare(
            "SELECT json_extract(payload, '$.data.agent_payload.tool') as tool,
                    COALESCE(
                        json_extract(payload, '$.data.agent_payload.args.file_path'),
                        json_extract(payload, '$.data.agent_payload.args.file'),
                        json_extract(payload, '$.data.agent_payload.args.path'),
                        json_extract(payload, '$.data.agent_payload.args.command')
                    ) as target,
                    timestamp
             FROM events
             WHERE session_id = ?1 AND subtype = 'message.assistant.tool_use'
               AND tool IS NOT NULL
             ORDER BY timestamp ASC",
        )
        .unwrap();

    stmt.query_map([session_id], |row| {
        Ok(ToolStep {
            tool: row.get(0)?,
            file: row.get(1)?,
            timestamp: row.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// File impact: files read vs. written, blast radius.
/// "What did the agent change?"
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct FileImpact {
    pub file: String,
    pub reads: u64,
    pub writes: u64,
}

/// Query file impact for a session.
pub fn file_impact(conn: &Connection, session_id: &str) -> Vec<FileImpact> {
    let mut stmt = conn
        .prepare(
            "SELECT target, tool,
                    COUNT(*) as cnt
             FROM (
                 SELECT COALESCE(
                            json_extract(payload, '$.data.agent_payload.args.file_path'),
                            json_extract(payload, '$.data.agent_payload.args.file'),
                            json_extract(payload, '$.data.agent_payload.args.path')
                        ) as target,
                        json_extract(payload, '$.data.agent_payload.tool') as tool
                 FROM events
                 WHERE session_id = ?1 AND subtype = 'message.assistant.tool_use'
                   AND target IS NOT NULL
             )
             GROUP BY target, tool
             ORDER BY target, tool",
        )
        .unwrap();

    let mut impacts: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();

    stmt.query_map([session_id], |row| {
        let target: String = row.get(0)?;
        let tool: String = row.get(1)?;
        let count: u64 = row.get(2)?;
        Ok((target, tool, count))
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .for_each(|(target, tool, count)| {
        let entry = impacts.entry(target).or_insert((0, 0));
        match tool.as_str() {
            "Read" | "Glob" | "Grep" => entry.0 += count,
            "Edit" | "Write" | "NotebookEdit" => entry.1 += count,
            _ => {} // ignore Bash, etc.
        }
    });

    let mut result: Vec<FileImpact> = impacts
        .into_iter()
        .map(|(file, (reads, writes))| FileImpact {
            file,
            reads,
            writes,
        })
        .collect();
    result.sort_by(|a, b| (b.reads + b.writes).cmp(&(a.reads + a.writes)));
    result
}

/// Session errors with timestamps.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionError {
    pub timestamp: String,
    pub message: String,
}

/// Query errors for a session.
pub fn session_errors(conn: &Connection, session_id: &str) -> Vec<SessionError> {
    let mut stmt = conn
        .prepare(
            "SELECT timestamp,
                    COALESCE(json_extract(payload, '$.data.agent_payload.text'), json_extract(payload, '$.data.raw.message.content'))
             FROM events
             WHERE session_id = ?1 AND subtype = 'system.error'
             ORDER BY timestamp ASC",
        )
        .unwrap();

    stmt.query_map([session_id], |row| {
        Ok(SessionError {
            timestamp: row.get(0)?,
            message: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ── Cross-Session Intelligence ──────────────────────────────────────

/// Project activity pulse: events per project in the last N days.
/// "Which projects am I actively working on?"
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectPulse {
    pub project_id: String,
    pub project_name: Option<String>,
    pub session_count: u64,
    pub event_count: u64,
    pub last_activity: Option<String>,
}

/// Query project activity over the last N days.
pub fn project_pulse(conn: &Connection, days: u32) -> Vec<ProjectPulse> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = format_ts(cutoff);

    let mut stmt = conn
        .prepare(
            "SELECT s.project_id, s.project_name,
                    COUNT(DISTINCT s.id) as session_count,
                    SUM(s.event_count) as total_events,
                    MAX(s.last_event) as last_activity
             FROM sessions s
             WHERE s.project_id IS NOT NULL
               AND s.last_event >= ?1
             GROUP BY s.project_id
             ORDER BY total_events DESC",
        )
        .unwrap();

    stmt.query_map([&cutoff_str], |row| {
        Ok(ProjectPulse {
            project_id: row.get(0)?,
            project_name: row.get(1)?,
            session_count: row.get(2)?,
            event_count: row.get(3)?,
            last_activity: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Tool evolution: tool mix over time.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolEvolution {
    pub week: String,
    pub tool: String,
    pub count: u64,
}

/// Query tool usage by week.
pub fn tool_evolution(conn: &Connection, days: u32) -> Vec<ToolEvolution> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = format_ts(cutoff);

    let mut stmt = conn
        .prepare(
            "SELECT strftime('%Y-W%W', timestamp) as week,
                    json_extract(payload, '$.data.agent_payload.tool') as tool,
                    COUNT(*) as cnt
             FROM events
             WHERE subtype = 'message.assistant.tool_use'
               AND tool IS NOT NULL
               AND timestamp >= ?1
             GROUP BY week, tool
             ORDER BY week, cnt DESC",
        )
        .unwrap();

    stmt.query_map([&cutoff_str], |row| {
        Ok(ToolEvolution {
            week: row.get(0)?,
            tool: row.get(1)?,
            count: row.get(2)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Session efficiency metrics.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionEfficiency {
    pub session_id: String,
    pub label: Option<String>,
    pub event_count: u64,
    pub tool_count: u64,
    pub error_count: u64,
    pub duration_secs: Option<i64>,
}

/// Query efficiency metrics across sessions.
pub fn session_efficiency(conn: &Connection) -> Vec<SessionEfficiency> {
    let mut stmt = conn
        .prepare(
            "SELECT s.id, s.label, s.event_count, s.first_event, s.last_event
             FROM sessions s
             ORDER BY s.last_event DESC
             LIMIT 50",
        )
        .unwrap();

    #[allow(clippy::type_complexity)]
    let sessions: Vec<(String, Option<String>, u64, Option<String>, Option<String>)> = stmt
        .query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    sessions
        .into_iter()
        .map(|(id, label, event_count, first_event, last_event)| {
            let tool_count: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND subtype = 'message.assistant.tool_use'",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let error_count: u64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM events WHERE session_id = ?1 AND subtype = 'system.error'",
                    [&id],
                    |r| r.get(0),
                )
                .unwrap_or(0);

            let duration_secs = match (&first_event, &last_event) {
                (Some(f), Some(l)) => {
                    let f = chrono::DateTime::parse_from_rfc3339(f).ok();
                    let l = chrono::DateTime::parse_from_rfc3339(l).ok();
                    match (f, l) {
                        (Some(f), Some(l)) => Some((l - f).num_seconds()),
                        _ => None,
                    }
                }
                _ => None,
            };

            SessionEfficiency {
                session_id: id,
                label,
                event_count,
                tool_count,
                error_count,
                duration_secs,
            }
        })
        .collect()
}

// ── Agentic Queries ─────────────────────────────────────────────────

/// Project context: recent sessions for a project.
/// "Pick up where the last agent left off."
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ProjectSession {
    pub session_id: String,
    pub label: Option<String>,
    pub event_count: u64,
    pub first_event: Option<String>,
    pub last_event: Option<String>,
}

/// Query last N sessions for a project.
pub fn project_context(conn: &Connection, project_id: &str, limit: usize) -> Vec<ProjectSession> {
    let mut stmt = conn
        .prepare(
            "SELECT id, label, event_count, first_event, last_event
             FROM sessions
             WHERE project_id = ?1
             ORDER BY last_event DESC
             LIMIT ?2",
        )
        .unwrap();

    stmt.query_map(rusqlite::params![project_id, limit], |row| {
        Ok(ProjectSession {
            session_id: row.get(0)?,
            label: row.get(1)?,
            event_count: row.get(2)?,
            first_event: row.get(3)?,
            last_event: row.get(4)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

/// Recent files: files modified in last N sessions for a project.
/// "Focus on active files, not the whole repo."
pub fn recent_files(conn: &Connection, project_id: &str, session_limit: usize) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT DISTINCT COALESCE(
                        json_extract(e.payload, '$.data.agent_payload.args.file_path'),
                        json_extract(e.payload, '$.data.agent_payload.args.file'),
                        json_extract(e.payload, '$.data.agent_payload.args.path')
                    ) as target
             FROM events e
             JOIN sessions s ON e.session_id = s.id
             WHERE s.project_id = ?1
               AND e.subtype IN ('message.assistant.tool_use')
               AND json_extract(e.payload, '$.data.agent_payload.tool') IN ('Edit', 'Write', 'NotebookEdit')
               AND target IS NOT NULL
             ORDER BY e.timestamp DESC
             LIMIT ?2",
        )
        .unwrap();

    stmt.query_map(rusqlite::params![project_id, session_limit * 20], |row| {
        row.get(0)
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ── Meta-Cognitive ──────────────────────────────────────────────────

/// Productivity by hour: event density by time of day.
/// "When should I schedule deep agent work?"
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HourlyActivity {
    pub hour: u32,
    pub event_count: u64,
}

/// Query activity density by hour of day.
pub fn productivity_by_hour(conn: &Connection, days: u32) -> Vec<HourlyActivity> {
    let cutoff = chrono::Utc::now() - chrono::Duration::days(days as i64);
    let cutoff_str = format_ts(cutoff);

    let mut stmt = conn
        .prepare(
            "SELECT CAST(strftime('%H', timestamp) AS INTEGER) as hour,
                    COUNT(*) as cnt
             FROM events
             WHERE timestamp >= ?1
             GROUP BY hour
             ORDER BY hour",
        )
        .unwrap();

    stmt.query_map([&cutoff_str], |row| {
        Ok(HourlyActivity {
            hour: row.get(0)?,
            event_count: row.get(1)?,
        })
    })
    .unwrap()
    .filter_map(|r| r.ok())
    .collect()
}

// ── Token Usage Queries ───────────────────────────────────────────

/// Token usage for a session or aggregate scope.
#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_creation_tokens: u64,
    pub message_count: u64,
    pub total_tokens: u64,
}

/// Token usage for a single session, with metadata.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SessionTokenUsage {
    pub session_id: String,
    pub label: Option<String>,
    pub project_name: Option<String>,
    pub first_event: Option<String>,
    pub last_event: Option<String>,
    #[serde(flatten)]
    pub usage: TokenUsage,
}

/// Cost estimate in USD.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CostEstimate {
    pub input: f64,
    pub output: f64,
    pub cache_read: f64,
    pub cache_creation: f64,
    pub total: f64,
    pub model: String,
}

/// Token usage summary with cost estimate.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TokenUsageSummary {
    pub session_count: u64,
    #[serde(flatten)]
    pub usage: TokenUsage,
    pub cost: CostEstimate,
    pub sessions: Vec<SessionTokenUsage>,
}

/// Daily token usage.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DailyTokenUsage {
    pub date: String,
    #[serde(flatten)]
    pub usage: TokenUsage,
}

fn estimate_cost(usage: &TokenUsage, model: &str) -> CostEstimate {
    let (input_rate, output_rate, cache_read_rate, cache_creation_rate) = match model {
        "opus" => (15.0, 75.0, 1.50, 18.75),
        "haiku" => (0.80, 4.0, 0.08, 1.0),
        _ => (3.0, 15.0, 0.30, 3.75), // sonnet
    };
    let input = usage.input_tokens as f64 * input_rate / 1_000_000.0;
    let output = usage.output_tokens as f64 * output_rate / 1_000_000.0;
    let cache_read = usage.cache_read_tokens as f64 * cache_read_rate / 1_000_000.0;
    let cache_creation = usage.cache_creation_tokens as f64 * cache_creation_rate / 1_000_000.0;
    CostEstimate {
        input,
        output,
        cache_read,
        cache_creation,
        total: input + output + cache_read + cache_creation,
        model: model.to_string(),
    }
}

/// Extract token usage from a single event payload JSON string.
fn extract_usage_from_payload(payload: &str) -> Option<(u64, u64, u64, u64)> {
    let d: serde_json::Value = serde_json::from_str(payload).ok()?;
    let usage = d.get("data")?.get("raw")?.get("message")?.get("usage")?;
    let input = usage.get("input_tokens")?.as_u64()?;
    let output = usage.get("output_tokens")?.as_u64().unwrap_or(0);
    let cache_read = usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    let cache_creation = usage.get("cache_creation_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0);
    Some((input, output, cache_read, cache_creation))
}

/// Query token usage across all sessions, optionally filtered by days or session_id.
pub fn token_usage(conn: &Connection, days: Option<u32>, session_id: Option<&str>, model: &str) -> TokenUsageSummary {
    // Fetch sessions
    let (session_sql, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match (days, session_id) {
        (_, Some(sid)) => (
            "SELECT id, label, project_name, first_event, last_event FROM sessions WHERE id = ?1".into(),
            vec![Box::new(sid.to_string())],
        ),
        (Some(d), None) => {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(d as i64);
            (
                "SELECT id, label, project_name, first_event, last_event FROM sessions WHERE last_event > ?1 ORDER BY last_event DESC".into(),
                vec![Box::new(format_ts(cutoff))],
            )
        }
        (None, None) => (
            "SELECT id, label, project_name, first_event, last_event FROM sessions ORDER BY last_event DESC".into(),
            vec![],
        ),
    };

    let mut stmt = conn.prepare(&session_sql).unwrap();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    #[allow(clippy::type_complexity)]
    let sessions: Vec<(String, Option<String>, Option<String>, Option<String>, Option<String>)> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    if sessions.is_empty() {
        return TokenUsageSummary {
            session_count: 0,
            usage: TokenUsage::default(),
            cost: estimate_cost(&TokenUsage::default(), model),
            sessions: Vec::new(),
        };
    }

    // Query usage events for these sessions
    let session_ids: Vec<&str> = sessions.iter().map(|s| s.0.as_str()).collect();
    let placeholders: String = session_ids.iter().enumerate().map(|(i, _)| format!("?{}", i + 1)).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT session_id, payload FROM events WHERE session_id IN ({}) AND subtype IN ('message.assistant.text', 'message.assistant.tool_use', 'message.assistant.thinking') AND payload LIKE '%input_tokens%'",
        placeholders
    );

    let mut stmt = conn.prepare(&sql).unwrap();
    let id_params: Vec<&dyn rusqlite::types::ToSql> = session_ids.iter().map(|s| s as &dyn rusqlite::types::ToSql).collect();
    let rows: Vec<(String, String)> = stmt
        .query_map(id_params.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    // Aggregate per session
    let mut session_usages: std::collections::HashMap<String, TokenUsage> = std::collections::HashMap::new();
    for (sid, payload) in &rows {
        if let Some((input, output, cache_read, cache_creation)) = extract_usage_from_payload(payload) {
            let u = session_usages.entry(sid.clone()).or_default();
            u.input_tokens += input;
            u.output_tokens += output;
            u.cache_read_tokens += cache_read;
            u.cache_creation_tokens += cache_creation;
            u.message_count += 1;
        }
    }
    // Compute total_tokens for each session
    for u in session_usages.values_mut() {
        u.total_tokens = u.input_tokens + u.output_tokens + u.cache_read_tokens + u.cache_creation_tokens;
    }

    // Build result
    let mut total = TokenUsage::default();
    let mut session_results: Vec<SessionTokenUsage> = Vec::new();
    for (sid, label, project_name, first_event, last_event) in &sessions {
        let usage = session_usages.remove(sid).unwrap_or_default();
        total.input_tokens += usage.input_tokens;
        total.output_tokens += usage.output_tokens;
        total.cache_read_tokens += usage.cache_read_tokens;
        total.cache_creation_tokens += usage.cache_creation_tokens;
        total.message_count += usage.message_count;
        if usage.message_count > 0 {
            session_results.push(SessionTokenUsage {
                session_id: sid.clone(),
                label: label.clone(),
                project_name: project_name.clone(),
                first_event: first_event.clone(),
                last_event: last_event.clone(),
                usage,
            });
        }
    }
    total.total_tokens = total.input_tokens + total.output_tokens + total.cache_read_tokens + total.cache_creation_tokens;

    // Sort sessions by output tokens descending
    session_results.sort_by(|a, b| b.usage.output_tokens.cmp(&a.usage.output_tokens));

    TokenUsageSummary {
        session_count: sessions.len() as u64,
        usage: total.clone(),
        cost: estimate_cost(&total, model),
        sessions: session_results,
    }
}

/// Query daily token usage trend.
pub fn daily_token_usage(conn: &Connection, days: Option<u32>) -> Vec<DailyTokenUsage> {
    let (where_clause, params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) = match days {
        Some(d) => {
            let cutoff = chrono::Utc::now() - chrono::Duration::days(d as i64);
            ("AND e.timestamp > ?1".into(), vec![Box::new(format_ts(cutoff))])
        }
        None => (String::new(), vec![]),
    };

    let sql = format!(
        "SELECT e.timestamp, e.payload FROM events e WHERE e.subtype IN ('message.assistant.text', 'message.assistant.tool_use', 'message.assistant.thinking') AND e.payload LIKE '%input_tokens%' {} ORDER BY e.timestamp",
        where_clause
    );

    let mut stmt = conn.prepare(&sql).unwrap();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows: Vec<(String, String)> = stmt
        .query_map(param_refs.as_slice(), |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();

    let mut by_day: std::collections::BTreeMap<String, TokenUsage> = std::collections::BTreeMap::new();
    for (timestamp, payload) in &rows {
        if let Some((input, output, cache_read, cache_creation)) = extract_usage_from_payload(payload) {
            let day = &timestamp[..10.min(timestamp.len())];
            let u = by_day.entry(day.to_string()).or_default();
            u.input_tokens += input;
            u.output_tokens += output;
            u.cache_read_tokens += cache_read;
            u.cache_creation_tokens += cache_creation;
            u.message_count += 1;
        }
    }

    by_day
        .into_iter()
        .map(|(date, mut usage)| {
            usage.total_tokens = usage.input_tokens + usage.output_tokens + usage.cache_read_tokens + usage.cache_creation_tokens;
            DailyTokenUsage { date, usage }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA journal_mode=WAL;").unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS events (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                subtype     TEXT NOT NULL DEFAULT '',
                timestamp   TEXT NOT NULL DEFAULT '',
                agent_id    TEXT,
                parent_uuid TEXT,
                payload     TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_events_session ON events(session_id, timestamp);
            CREATE INDEX IF NOT EXISTS idx_events_subtype ON events(subtype);

            CREATE TABLE IF NOT EXISTS sessions (
                id           TEXT PRIMARY KEY,
                project_id   TEXT,
                project_name TEXT,
                label        TEXT,
                branch       TEXT,
                event_count  INTEGER DEFAULT 0,
                first_event  TEXT,
                last_event   TEXT
            );

            CREATE TABLE IF NOT EXISTS patterns (
                id          TEXT PRIMARY KEY,
                session_id  TEXT NOT NULL,
                type        TEXT NOT NULL,
                start_time  TEXT NOT NULL DEFAULT '',
                end_time    TEXT NOT NULL DEFAULT '',
                summary     TEXT NOT NULL DEFAULT '',
                event_ids   TEXT NOT NULL DEFAULT '[]',
                metadata    TEXT
            );",
        )
        .unwrap();
        conn
    }

    fn insert_session(conn: &Connection, id: &str, project_id: &str, label: &str, event_count: u64) {
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, event_count, first_event, last_event)
             VALUES (?1, ?2, ?3, ?4, ?5, '2025-01-15T10:00:00Z', '2025-01-15T11:00:00Z')",
            rusqlite::params![id, project_id, project_id, label, event_count],
        )
        .unwrap();
    }

    fn insert_tool_event(conn: &Connection, id: &str, session_id: &str, tool: &str, file: Option<&str>, ts: &str) {
        let args = match file {
            Some(f) => format!(r#"{{"file_path": "{}"}}"#, f),
            None => r#"{"command": "cargo test"}"#.to_string(),
        };
        // Production queries read from $.data.agent_payload.tool and
        // $.data.agent_payload.args.* — wrap test data in the typed
        // AgentPayload::ClaudeCode shape so the JSON-extract paths match.
        let payload = format!(
            r#"{{"data": {{"agent_payload": {{"_variant": "claude-code", "meta": {{"agent": "claude-code"}}, "tool": "{}", "args": {}}}}}}}"#,
            tool, args
        );
        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload)
             VALUES (?1, ?2, 'message.assistant.tool_use', ?3, ?4)",
            rusqlite::params![id, session_id, ts, payload],
        )
        .unwrap();
    }

    fn insert_error_event(conn: &Connection, id: &str, session_id: &str, msg: &str, ts: &str) {
        // system.error events store text inside agent_payload.text under the
        // typed payload model (matches translator output for hook errors).
        let payload = format!(
            r#"{{"data": {{"agent_payload": {{"_variant": "claude-code", "meta": {{"agent": "claude-code"}}, "text": "{}"}}}}}}"#,
            msg
        );
        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload)
             VALUES (?1, ?2, 'system.error', ?3, ?4)",
            rusqlite::params![id, session_id, ts, payload],
        )
        .unwrap();
    }

    // ── session_synopsis tests ──────────────────────────────────────

    #[test]
    fn synopsis_returns_none_for_missing_session() {
        let conn = setup_test_db();
        assert!(session_synopsis(&conn, "nonexistent").is_none());
    }

    #[test]
    fn synopsis_returns_session_metadata() {
        let conn = setup_test_db();
        insert_session(&conn, "sess-1", "my-project", "Fix the bug", 42);
        insert_tool_event(&conn, "t1", "sess-1", "Read", Some("src/main.rs"), "2025-01-15T10:00:00Z");
        insert_tool_event(&conn, "t2", "sess-1", "Edit", Some("src/main.rs"), "2025-01-15T10:01:00Z");
        insert_tool_event(&conn, "t3", "sess-1", "Bash", None, "2025-01-15T10:02:00Z");

        let s = session_synopsis(&conn, "sess-1").unwrap();
        assert_eq!(s.session_id, "sess-1");
        assert_eq!(s.label.as_deref(), Some("Fix the bug"));
        assert_eq!(s.project_id.as_deref(), Some("my-project"));
        assert_eq!(s.event_count, 42);
        assert_eq!(s.tool_count, 3);
        assert_eq!(s.error_count, 0);
        assert_eq!(s.duration_secs, Some(3600)); // 1 hour
        assert_eq!(s.top_tools.len(), 3);
        // Top tools should be ordered by count (all 1 here)
        assert!(s.top_tools.iter().all(|t| t.count == 1));
    }

    #[test]
    fn synopsis_counts_errors() {
        let conn = setup_test_db();
        insert_session(&conn, "sess-err", "proj", "errors", 5);
        insert_error_event(&conn, "e1", "sess-err", "connection refused", "2025-01-15T10:00:00Z");
        insert_error_event(&conn, "e2", "sess-err", "timeout", "2025-01-15T10:01:00Z");

        let s = session_synopsis(&conn, "sess-err").unwrap();
        assert_eq!(s.error_count, 2);
    }

    // ── tool_journey tests ──────────────────────────────────────────

    #[test]
    fn tool_journey_returns_sequence() {
        let conn = setup_test_db();
        insert_tool_event(&conn, "j1", "sess-j", "Read", Some("lib.rs"), "2025-01-15T10:00:00Z");
        insert_tool_event(&conn, "j2", "sess-j", "Edit", Some("lib.rs"), "2025-01-15T10:01:00Z");
        insert_tool_event(&conn, "j3", "sess-j", "Bash", None, "2025-01-15T10:02:00Z");

        let journey = tool_journey(&conn, "sess-j");
        assert_eq!(journey.len(), 3);
        assert_eq!(journey[0].tool, "Read");
        assert_eq!(journey[0].file.as_deref(), Some("lib.rs"));
        assert_eq!(journey[1].tool, "Edit");
        assert_eq!(journey[2].tool, "Bash");
        assert_eq!(journey[2].file.as_deref(), Some("cargo test"));
    }

    #[test]
    fn tool_journey_empty_session() {
        let conn = setup_test_db();
        assert!(tool_journey(&conn, "nonexistent").is_empty());
    }

    // ── file_impact tests ───────────────────────────────────────────

    #[test]
    fn file_impact_separates_reads_and_writes() {
        let conn = setup_test_db();
        insert_tool_event(&conn, "fi1", "sess-fi", "Read", Some("src/main.rs"), "2025-01-15T10:00:00Z");
        insert_tool_event(&conn, "fi2", "sess-fi", "Read", Some("src/main.rs"), "2025-01-15T10:01:00Z");
        insert_tool_event(&conn, "fi3", "sess-fi", "Edit", Some("src/main.rs"), "2025-01-15T10:02:00Z");
        insert_tool_event(&conn, "fi4", "sess-fi", "Write", Some("src/new.rs"), "2025-01-15T10:03:00Z");

        let impact = file_impact(&conn, "sess-fi");
        assert_eq!(impact.len(), 2);

        let main = impact.iter().find(|i| i.file == "src/main.rs").unwrap();
        assert_eq!(main.reads, 2);
        assert_eq!(main.writes, 1);

        let new = impact.iter().find(|i| i.file == "src/new.rs").unwrap();
        assert_eq!(new.reads, 0);
        assert_eq!(new.writes, 1);
    }

    // ── session_errors tests ────────────────────────────────────────

    #[test]
    fn session_errors_returns_error_messages() {
        let conn = setup_test_db();
        insert_error_event(&conn, "se1", "sess-se", "fail 1", "2025-01-15T10:00:00Z");
        insert_error_event(&conn, "se2", "sess-se", "fail 2", "2025-01-15T10:01:00Z");

        let errors = session_errors(&conn, "sess-se");
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].message, "fail 1");
        assert_eq!(errors[1].message, "fail 2");
    }

    // ── project_pulse tests ─────────────────────────────────────────

    #[test]
    fn project_pulse_aggregates_by_project() {
        let conn = setup_test_db();
        // Use recent timestamps so the 30-day filter includes them.
        // Use the canonical translator format (Z suffix) — see §6.8.
        let now = format_ts(chrono::Utc::now());
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, event_count, first_event, last_event)
             VALUES ('s1', 'proj-a', 'proj-a', 'session 1', 100, ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, event_count, first_event, last_event)
             VALUES ('s2', 'proj-a', 'proj-a', 'session 2', 50, ?1, ?1)",
            [&now],
        ).unwrap();
        conn.execute(
            "INSERT INTO sessions (id, project_id, project_name, label, event_count, first_event, last_event)
             VALUES ('s3', 'proj-b', 'proj-b', 'session 3', 30, ?1, ?1)",
            [&now],
        ).unwrap();

        let pulse = project_pulse(&conn, 30);
        assert_eq!(pulse.len(), 2);
        assert_eq!(pulse[0].project_id, "proj-a");
        assert_eq!(pulse[0].session_count, 2);
        assert_eq!(pulse[0].event_count, 150);
        assert_eq!(pulse[1].project_id, "proj-b");
        assert_eq!(pulse[1].session_count, 1);
    }

    // ── project_context tests ───────────────────────────────────────

    #[test]
    fn project_context_returns_recent_sessions() {
        let conn = setup_test_db();
        insert_session(&conn, "pc1", "proj-x", "first", 10);
        insert_session(&conn, "pc2", "proj-x", "second", 20);
        insert_session(&conn, "pc3", "proj-y", "other", 5);

        let ctx = project_context(&conn, "proj-x", 5);
        assert_eq!(ctx.len(), 2);
        assert!(ctx.iter().all(|s| s.session_id.starts_with("pc")));
    }

    #[test]
    fn project_context_respects_limit() {
        let conn = setup_test_db();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO sessions (id, project_id, label, event_count, first_event, last_event)
                 VALUES (?1, 'proj-lim', 'sess', 10, '2025-01-15T10:00:00Z', ?2)",
                rusqlite::params![format!("lim-{i}"), format!("2025-01-15T1{}:00:00Z", i)],
            ).unwrap();
        }
        let ctx = project_context(&conn, "proj-lim", 3);
        assert_eq!(ctx.len(), 3);
    }

    // ── recent_files tests ──────────────────────────────────────────

    #[test]
    fn recent_files_returns_modified_files() {
        let conn = setup_test_db();
        insert_session(&conn, "rf-s1", "proj-rf", "edit files", 10);
        insert_tool_event(&conn, "rf1", "rf-s1", "Edit", Some("src/lib.rs"), "2025-01-15T10:00:00Z");
        insert_tool_event(&conn, "rf2", "rf-s1", "Write", Some("src/new.rs"), "2025-01-15T10:01:00Z");
        insert_tool_event(&conn, "rf3", "rf-s1", "Read", Some("src/other.rs"), "2025-01-15T10:02:00Z");

        let files = recent_files(&conn, "proj-rf", 5);
        assert!(files.contains(&"src/lib.rs".to_string()));
        assert!(files.contains(&"src/new.rs".to_string()));
        // Read should not be in the list
        assert!(!files.contains(&"src/other.rs".to_string()));
    }

    // ── session_efficiency tests ────────────────────────────────────

    #[test]
    fn session_efficiency_returns_metrics() {
        let conn = setup_test_db();
        insert_session(&conn, "eff-1", "proj-e", "test session", 25);
        insert_tool_event(&conn, "eff-t1", "eff-1", "Read", Some("x.rs"), "2025-01-15T10:00:00Z");
        insert_tool_event(&conn, "eff-t2", "eff-1", "Edit", Some("x.rs"), "2025-01-15T10:01:00Z");
        insert_error_event(&conn, "eff-e1", "eff-1", "oops", "2025-01-15T10:02:00Z");

        let eff = session_efficiency(&conn);
        assert_eq!(eff.len(), 1);
        assert_eq!(eff[0].session_id, "eff-1");
        assert_eq!(eff[0].event_count, 25);
        assert_eq!(eff[0].tool_count, 2);
        assert_eq!(eff[0].error_count, 1);
        assert_eq!(eff[0].duration_secs, Some(3600));
    }

    // ── productivity_by_hour tests ──────────────────────────────────

    #[test]
    fn productivity_by_hour_groups_by_hour() {
        let conn = setup_test_db();
        // Insert events at different hours
        let now = chrono::Utc::now();
        let ts = now.format("%Y-%m-%dT10:00:00Z").to_string();
        let ts2 = now.format("%Y-%m-%dT10:30:00Z").to_string();
        let ts3 = now.format("%Y-%m-%dT14:00:00Z").to_string();

        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload) VALUES ('h1', 'sh', 'message.user.prompt', ?1, '{}')",
            [&ts],
        ).unwrap();
        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload) VALUES ('h2', 'sh', 'message.user.prompt', ?1, '{}')",
            [&ts2],
        ).unwrap();
        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload) VALUES ('h3', 'sh', 'message.user.prompt', ?1, '{}')",
            [&ts3],
        ).unwrap();

        let hourly = productivity_by_hour(&conn, 30);
        assert!(!hourly.is_empty());
        let hour_10 = hourly.iter().find(|h| h.hour == 10);
        assert!(hour_10.is_some());
        assert_eq!(hour_10.unwrap().event_count, 2);
    }

    // ── tool_evolution tests ───────────────────────────────────────

    #[test]
    fn tool_evolution_returns_weekly_tool_counts() {
        let conn = setup_test_db();
        let now = chrono::Utc::now();
        let ts1 = now.format("%Y-%m-%dT10:00:00Z").to_string();
        let ts2 = now.format("%Y-%m-%dT10:30:00Z").to_string();
        let ts3 = now.format("%Y-%m-%dT11:00:00Z").to_string();

        insert_tool_event(&conn, "te1", "sess-te", "Read", Some("a.rs"), &ts1);
        insert_tool_event(&conn, "te2", "sess-te", "Read", Some("b.rs"), &ts2);
        insert_tool_event(&conn, "te3", "sess-te", "Edit", Some("a.rs"), &ts3);

        let evo = tool_evolution(&conn, 30);
        assert!(!evo.is_empty());
        // Should have entries for Read and Edit in the current week
        let read_entry = evo.iter().find(|e| e.tool == "Read");
        assert!(read_entry.is_some());
        assert_eq!(read_entry.unwrap().count, 2);
        let edit_entry = evo.iter().find(|e| e.tool == "Edit");
        assert!(edit_entry.is_some());
        assert_eq!(edit_entry.unwrap().count, 1);
    }

    #[test]
    fn tool_evolution_empty_when_no_tool_events() {
        let conn = setup_test_db();
        let evo = tool_evolution(&conn, 30);
        assert!(evo.is_empty());
    }

    // ── token usage tests ────────────────────────────────────────────

    fn insert_assistant_event_with_usage(
        conn: &Connection,
        id: &str,
        session_id: &str,
        ts: &str,
        input: u64,
        output: u64,
        cache_read: u64,
        cache_creation: u64,
    ) {
        let payload = format!(
            r#"{{"data": {{"raw": {{"message": {{"usage": {{"input_tokens": {}, "output_tokens": {}, "cache_read_input_tokens": {}, "cache_creation_input_tokens": {}}}}}}}}}}}"#,
            input, output, cache_read, cache_creation
        );
        conn.execute(
            "INSERT INTO events (id, session_id, subtype, timestamp, payload)
             VALUES (?1, ?2, 'message.assistant.text', ?3, ?4)",
            rusqlite::params![id, session_id, ts, payload],
        ).unwrap();
    }

    #[test]
    fn token_usage_empty_db() {
        let conn = setup_test_db();
        let result = token_usage(&conn, None, None, "sonnet");
        assert_eq!(result.session_count, 0);
        assert_eq!(result.usage.total_tokens, 0);
        assert!(result.sessions.is_empty());
    }

    #[test]
    fn token_usage_aggregates_across_sessions() {
        let conn = setup_test_db();
        insert_session(&conn, "s1", "proj-a", "session 1", 10);
        insert_session(&conn, "s2", "proj-a", "session 2", 5);

        insert_assistant_event_with_usage(&conn, "u1", "s1", "2025-01-15T10:00:00Z", 100, 200, 1000, 50);
        insert_assistant_event_with_usage(&conn, "u2", "s1", "2025-01-15T10:01:00Z", 100, 300, 2000, 50);
        insert_assistant_event_with_usage(&conn, "u3", "s2", "2025-01-15T10:00:00Z", 50, 100, 500, 25);

        let result = token_usage(&conn, None, None, "sonnet");
        assert_eq!(result.session_count, 2);
        assert_eq!(result.usage.input_tokens, 250);
        assert_eq!(result.usage.output_tokens, 600);
        assert_eq!(result.usage.cache_read_tokens, 3500);
        assert_eq!(result.usage.cache_creation_tokens, 125);
        assert_eq!(result.usage.message_count, 3);
        assert_eq!(result.usage.total_tokens, 250 + 600 + 3500 + 125);
        assert_eq!(result.sessions.len(), 2);
    }

    #[test]
    fn token_usage_filters_by_session_id() {
        let conn = setup_test_db();
        insert_session(&conn, "s1", "proj", "first", 10);
        insert_session(&conn, "s2", "proj", "second", 5);

        insert_assistant_event_with_usage(&conn, "u1", "s1", "2025-01-15T10:00:00Z", 100, 200, 0, 0);
        insert_assistant_event_with_usage(&conn, "u2", "s2", "2025-01-15T10:00:00Z", 50, 100, 0, 0);

        let result = token_usage(&conn, None, Some("s1"), "sonnet");
        assert_eq!(result.session_count, 1);
        assert_eq!(result.usage.input_tokens, 100);
        assert_eq!(result.usage.output_tokens, 200);
    }

    #[test]
    fn token_usage_cost_estimate_sonnet() {
        let conn = setup_test_db();
        insert_session(&conn, "s1", "proj", "test", 1);
        insert_assistant_event_with_usage(&conn, "u1", "s1", "2025-01-15T10:00:00Z", 1_000_000, 1_000_000, 0, 0);

        let result = token_usage(&conn, None, None, "sonnet");
        assert!((result.cost.input - 3.0).abs() < 0.01);
        assert!((result.cost.output - 15.0).abs() < 0.01);
        assert!((result.cost.total - 18.0).abs() < 0.01);
    }

    #[test]
    fn token_usage_sessions_sorted_by_output_desc() {
        let conn = setup_test_db();
        insert_session(&conn, "lo", "proj", "low output", 1);
        insert_session(&conn, "hi", "proj", "high output", 1);

        insert_assistant_event_with_usage(&conn, "u1", "lo", "2025-01-15T10:00:00Z", 10, 50, 0, 0);
        insert_assistant_event_with_usage(&conn, "u2", "hi", "2025-01-15T10:00:00Z", 10, 500, 0, 0);

        let result = token_usage(&conn, None, None, "sonnet");
        assert_eq!(result.sessions[0].session_id, "hi");
        assert_eq!(result.sessions[1].session_id, "lo");
    }

    #[test]
    fn extract_usage_from_payload_valid() {
        let payload = r#"{"data":{"raw":{"message":{"usage":{"input_tokens":42,"output_tokens":99,"cache_read_input_tokens":100,"cache_creation_input_tokens":50}}}}}"#;
        let (input, output, cache_read, cache_creation) = extract_usage_from_payload(payload).unwrap();
        assert_eq!(input, 42);
        assert_eq!(output, 99);
        assert_eq!(cache_read, 100);
        assert_eq!(cache_creation, 50);
    }

    #[test]
    fn extract_usage_from_payload_invalid() {
        assert!(extract_usage_from_payload("not json").is_none());
        assert!(extract_usage_from_payload("{}").is_none());
        assert!(extract_usage_from_payload(r#"{"data":{}}"#).is_none());
    }

    #[test]
    fn daily_token_usage_groups_by_day() {
        let conn = setup_test_db();
        insert_assistant_event_with_usage(&conn, "d1", "s1", "2025-01-15T10:00:00Z", 100, 200, 0, 0);
        insert_assistant_event_with_usage(&conn, "d2", "s1", "2025-01-15T14:00:00Z", 50, 100, 0, 0);
        insert_assistant_event_with_usage(&conn, "d3", "s1", "2025-01-16T10:00:00Z", 75, 150, 0, 0);

        let result = daily_token_usage(&conn, None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].date, "2025-01-15");
        assert_eq!(result[0].usage.input_tokens, 150);
        assert_eq!(result[0].usage.output_tokens, 300);
        assert_eq!(result[0].usage.message_count, 2);
        assert_eq!(result[1].date, "2025-01-16");
        assert_eq!(result[1].usage.input_tokens, 75);
    }

    #[test]
    fn daily_token_usage_empty_db() {
        let conn = setup_test_db();
        let result = daily_token_usage(&conn, None);
        assert!(result.is_empty());
    }
}
