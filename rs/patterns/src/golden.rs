//! GoldenSpec — the typed, immutable description a story golden derives from.
//!
//! Memory hands (docs: openstory-research/memory/hands/REQUIREMENTS.md, group X).
//! A golden is never copied from a real session. It is *generated* from a
//! `GoldenSpec`, so the expected exchanges, arcs, and handles are known by
//! construction. Real sessions inform shape parameters by hand (how many
//! exchanges, how many injected, gap distribution) and nothing else.
//!
//! Everything here is plain data: serde for the committed `spec.json`,
//! schemars for `schemas/golden_spec.schema.json`. No behaviour lives in
//! this module; `generate` and `expect` are pure functions over it.

use serde::{Deserialize, Serialize};

/// Whether a user-role message was typed by the human or injected by the
/// harness (a skill body, a tool-loaded notice, a task notification).
/// Injected messages arrive while the assistant's turn is still open and
/// must not open an exchange (requirement A-03).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExchangeKind {
    Human,
    Injected,
}

/// Length class of the prompt text the generator writes. Content is
/// synthetic either way; the class only shapes byte counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PromptClass {
    Short,
    Long,
}

/// One assistant turn inside an exchange.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TurnSpec {
    /// Sentence verb the turn should fold to (`read`, `edited`, `committed`, ...).
    pub verb: String,
    /// Objects the turn acts on; become entities in the expected output.
    pub objects: Vec<String>,
    /// Tool name and call count, in emission order.
    pub tools: Vec<(String, u32)>,
    /// Whether the turn is rich enough to produce a `turn.sentence`.
    /// `false` yields a thin turn (requirement A-04).
    pub has_sentence: bool,
}

/// One user-role message and the turns that answer it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ExchangeSpec {
    pub kind: ExchangeKind,
    pub prompt_class: PromptClass,
    /// Event-time gap between the previous exchange's last event and this
    /// exchange's first event. Compared against `gap_threshold_secs` to
    /// place arc boundaries (requirement A-06).
    pub gap_before_secs: u64,
    pub turns: Vec<TurnSpec>,
}

/// The whole spec for one synthetic session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GoldenSpec {
    pub session_id: String,
    /// RFC 3339 timestamp of the first event.
    pub started_at: String,
    /// Arc gap threshold in seconds (default in production: 1800).
    pub gap_threshold_secs: u64,
    pub exchanges: Vec<ExchangeSpec>,
}

// ═══════════════════════════════════════════════════════════════════
// generate — pure: GoldenSpec → Vec<CloudEvent>
// ═══════════════════════════════════════════════════════════════════

use chrono::{DateTime, Duration, SecondsFormat, Utc};
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::{AgentPayload, ClaudeCodePayload, EventData};
use uuid::Uuid;

/// Source string stamped on every generated event.
pub const GOLDEN_SOURCE: &str = "openstory-golden";

/// Filler sentence for Long prompts and injected bodies. Synthetic on purpose.
const FILLER: &str = "This line is synthetic golden content and carries no real session text. ";

/// Deterministic event id: uuid v5 of (session, exchange, turn, seq).
fn event_id(session_id: &str, exchange: usize, turn: usize, seq: u64) -> String {
    let seed = format!("{session_id}/{exchange}/{turn}/{seq}");
    Uuid::new_v5(&Uuid::NAMESPACE_URL, seed.as_bytes()).to_string()
}

fn rfc3339(t: DateTime<Utc>) -> String {
    t.to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// The text a user-role message carries, shaped by kind and class.
fn prompt_text(spec: &GoldenSpec, i: usize) -> String {
    let ex = &spec.exchanges[i];
    let verb = ex.turns.first().map(|t| t.verb.as_str()).unwrap_or("look");
    let object = ex
        .turns
        .first()
        .and_then(|t| t.objects.first())
        .map(String::as_str)
        .unwrap_or("the project");
    let base = match ex.kind {
        ExchangeKind::Human => format!("golden prompt {i}: please {verb} {object}"),
        ExchangeKind::Injected => {
            format!(
                "Base directory for this skill: /synthetic/skills/{i}\n\n# synthetic skill {i}\n\n"
            )
        }
    };
    match ex.prompt_class {
        PromptClass::Short => base,
        PromptClass::Long => format!("{base}\n{}", FILLER.repeat(12)),
    }
}

fn tool_args(tool: &str, object: &str) -> serde_json::Value {
    match tool {
        "Read" | "Edit" | "Write" | "Glob" | "Grep" => serde_json::json!({ "file_path": object }),
        "Bash" => serde_json::json!({ "command": format!("ls {object}") }),
        "Skill" => serde_json::json!({ "skill": object }),
        _ => serde_json::json!({ "input": object }),
    }
}

/// Accumulator for the generator: immutable spec in, events out, one
/// cursor for time and one for seq. Private; `generate` is the API.
struct Gen<'a> {
    spec: &'a GoldenSpec,
    events: Vec<CloudEvent>,
    t: DateTime<Utc>,
    seq: u64,
}

impl Gen<'_> {
    fn push(
        &mut self,
        exchange: usize,
        turn: usize,
        subtype: &str,
        f: impl FnOnce(&mut ClaudeCodePayload),
    ) {
        let mut payload = ClaudeCodePayload::new();
        f(&mut payload);
        self.seq += 1;
        let data = EventData::with_payload(
            serde_json::json!({ "golden": true }),
            self.seq,
            self.spec.session_id.clone(),
            AgentPayload::ClaudeCode(payload),
        );
        self.events.push(CloudEvent::new(
            GOLDEN_SOURCE.to_string(),
            "io.arc.event".to_string(),
            data,
            Some(subtype.to_string()),
            Some(event_id(&self.spec.session_id, exchange, turn, self.seq)),
            Some(rfc3339(self.t)),
            Some(self.spec.session_id.clone()),
            None,
            Some("claude-code".to_string()),
        ));
        self.t += Duration::seconds(1);
    }
}

/// Pure: lay a GoldenSpec out as the CloudEvent stream the translator
/// would have produced. Same spec, same bytes.
///
/// Shape rules (requirements X-02, A-03):
/// - a Human exchange's prompt is the first event after a closed turn;
/// - an Injected exchange's prompt arrives while the previous turn is
///   still open, after that turn's last assistant event, before its
///   `system.turn.complete`;
/// - every turn emits `tool_use`/`tool_result` pairs per tool count, then
///   `message.assistant.text` and `system.turn.complete`, unless the next
///   exchange is Injected, in which case the close is deferred;
/// - time starts at `started_at`, advances one second per event, and
///   jumps by `gap_before_secs` before each exchange.
pub fn generate(spec: &GoldenSpec) -> Vec<CloudEvent> {
    let started = DateTime::parse_from_rfc3339(&spec.started_at)
        .map(|t| t.with_timezone(&Utc))
        .unwrap_or_else(|_| DateTime::<Utc>::from_timestamp(0, 0).expect("epoch"));
    let mut g = Gen {
        spec,
        events: Vec::new(),
        t: started,
        seq: 0,
    };

    for (i, ex) in spec.exchanges.iter().enumerate() {
        g.t += Duration::seconds(ex.gap_before_secs as i64);
        let text = prompt_text(spec, i);
        g.push(i, 0, "message.user.prompt", |p| p.text = Some(text));

        let next_is_injected = spec
            .exchanges
            .get(i + 1)
            .map(|n| n.kind == ExchangeKind::Injected)
            .unwrap_or(false);

        for (j, turn) in ex.turns.iter().enumerate() {
            let mut emitted_assistant = false;
            let mut objects = turn.objects.iter().cycle();
            for (tool, count) in &turn.tools {
                for _ in 0..*count {
                    let object = objects
                        .next()
                        .cloned()
                        .unwrap_or_else(|| "the project".to_string());
                    let args = tool_args(tool, &object);
                    let tool_name = tool.clone();
                    g.push(i, j + 1, "message.assistant.tool_use", |p| {
                        p.text = Some(format!("Claude will {} {}", turn.verb, object));
                        p.tool = Some(tool_name);
                        p.args = Some(args);
                        p.stop_reason = Some(serde_json::json!("tool_use"));
                    });
                    g.push(i, j + 1, "message.user.tool_result", |p| {
                        p.text = Some("ok".to_string());
                    });
                    emitted_assistant = true;
                }
            }

            let last_turn = j + 1 == ex.turns.len();
            let leave_open = last_turn && next_is_injected;
            let closing_text = if turn.has_sentence {
                format!("Claude {} {}", turn.verb, turn.objects.join(", "))
            } else {
                "ok".to_string()
            };
            if leave_open {
                if !emitted_assistant {
                    g.push(i, j + 1, "message.assistant.text", |p| {
                        p.text = Some(closing_text);
                        p.stop_reason = Some(serde_json::json!("tool_use"));
                    });
                }
            } else {
                g.push(i, j + 1, "message.assistant.text", |p| {
                    p.text = Some(closing_text);
                    p.stop_reason = Some(serde_json::json!("end_turn"));
                });
                g.push(i, j + 1, "system.turn.complete", |p| {
                    p.duration_ms = Some(1000.0)
                });
            }
        }
    }
    g.events
}
