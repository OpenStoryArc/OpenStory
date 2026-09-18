//! Story layer primitives shared by the golden generator and the fold.
//!
//! # A-01 decision (2026-09-18): how the detector receives its inputs
//!
//! 1. **Sentences.** `sentence::build_sentence(&StructuralTurn)` is pure and
//!    public, so `StoryDetector` implements `TurnDetector` and calls it per
//!    turn. No pipeline signature change; phase 3 reads phase 2's function,
//!    not its patterns.
//! 2. **Human prompts.** Real data (session b0a56730, 2026-09-17): an
//!    injected user-role message (skill body, tool-loaded notice) arrives
//!    while the assistant's turn is still open, and `eval_apply` folds it
//!    into that same `StructuralTurn`, overwriting `human`. The fix lives in
//!    `eval_apply`: keep the *first* prompt of a turn as `human`, record any
//!    later prompts in `StructuralTurn::injected`. Then an exchange opens at
//!    every turn with `human: Some(_)` and injected prompts can never open
//!    one, because they never start a turn (A-03).
//! 3. **Thin turns.** The Rust pipeline emits a `turn.sentence` for every
//!    turn, so "a turn with no sentence" does not exist here. A thin turn is
//!    a turn with no tool applies: it contributes events and time to its
//!    exchange, nothing to entities or tools (A-04). Goldens say `rich`
//!    instead of `has_sentence`, and expected output counts `rich_turns`.
//!
//! Memory hands, layers 6 and 7 (exchange, arc). Everything here is pure.
//! The detector itself lands with requirement A-01; this module holds
//! what both the detector and `golden::expect` must agree on so they
//! cannot drift: the content-address function and the verb classes that
//! mark an ambiguous seam.

use crate::sentence::{build_sentence, classify_tool, TurnSentence};
use crate::{PatternEvent, StructuralTurn, TurnDetector};
use open_story_core::strings::truncate_at_char_boundary;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Production arc gap threshold: 30 minutes of event time.
pub const DEFAULT_GAP_THRESHOLD_SECS: u64 = 1800;

/// Verbs that close a piece of work. A seam where one of these is
/// followed by an opening verb on new entities is `Ambiguous` (A-07).
pub const CLOSURE_VERBS: &[&str] = &["committed", "pushed"];

/// Verbs that open a new line of work.
pub const OPENING_VERBS: &[&str] = &["explored", "read", "searched for"];

/// Tools whose summarized input names a thing worth remembering. Bash
/// commands and skill loads are not entities. `golden::expect` uses the
/// same list so goldens and the fold cannot disagree.
pub const ENTITY_TOOLS: &[&str] = &[
    "Read",
    "Edit",
    "Write",
    "Glob",
    "Grep",
    "Agent",
    "WebFetch",
    "WebSearch",
];

/// Content-addressed handle of a node: 16 hex characters derived from the
/// layer name and the sorted event ids beneath it. Order-independent;
/// stable across re-narration because summaries and readings never feed
/// into it (G-06). The layer is part of the address because a one-turn
/// exchange, its sentence, and a one-exchange arc cover the same events.
pub fn handle(layer: &str, event_ids: &[String]) -> String {
    let mut ids: Vec<&str> = event_ids.iter().map(String::as_str).collect();
    ids.sort_unstable();
    let joined = format!("{layer}\n{}", ids.join("\n"));
    let full = Uuid::new_v5(&Uuid::NAMESPACE_OID, joined.as_bytes());
    full.simple().to_string()[..16].to_string()
}

pub fn is_closure_verb(verb: &str) -> bool {
    CLOSURE_VERBS.contains(&verb)
}

pub fn is_opening_verb(verb: &str) -> bool {
    OPENING_VERBS.contains(&verb)
}

// ═══════════════════════════════════════════════════════════════════
// StoryDetector — phase 3: StructuralTurn → story.exchange / story.arc
// ═══════════════════════════════════════════════════════════════════
//
// Two monoidal folds. An exchange folds turns; an arc folds exchanges.
// Boundaries: a turn with a human prompt opens an exchange (A-03); an
// event-time gap over the threshold closes an arc (A-06); a closure verb
// followed by an opening verb on new entities inside the threshold marks
// the seam ambiguous and does not split (A-07). Handles are content
// addresses of event ids (A-09). Nothing here reads a clock.

fn secs(rfc3339: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(rfc3339)
        .map(|t| t.timestamp())
        .unwrap_or(0)
}

fn clip(s: &str, max_bytes: usize) -> String {
    truncate_at_char_boundary(s, max_bytes).to_string()
}

fn bump(map: &mut BTreeMap<String, u32>, key: impl Into<String>, by: u32) {
    *map.entry(key.into()).or_insert(0) += by;
}

/// The count monoid: associative, with the empty map as identity (A-11).
pub fn merge_counts(into: &mut BTreeMap<String, u32>, from: &BTreeMap<String, u32>) {
    for (k, v) in from {
        bump(into, k.clone(), *v);
    }
}

/// Entities a turn names: the summarized inputs of entity-bearing tools.
fn turn_entities(turn: &StructuralTurn) -> BTreeMap<String, u32> {
    let mut out = BTreeMap::new();
    for a in &turn.applies {
        if ENTITY_TOOLS.contains(&a.tool_name.as_str()) && !a.input_summary.is_empty() {
            bump(&mut out, a.input_summary.clone(), 1);
        }
    }
    out
}

/// The end of a turn as far as the turn itself knows: its last assistant
/// output, else its start. (`system.turn.complete` is not on the turn.)
fn turn_ended_at(turn: &StructuralTurn) -> String {
    turn.eval
        .as_ref()
        .map(|e| e.timestamp.clone())
        .unwrap_or_else(|| turn.timestamp.clone())
}

/// Accumulator for one exchange.
struct ExchangeAcc {
    started_at: String,
    ended_at: String,
    event_ids: Vec<String>,
    entities: BTreeMap<String, u32>,
    tools: BTreeMap<String, u32>,
    tools_by_role: BTreeMap<String, u32>,
    turns: u32,
    rich_turns: u32,
    injected_count: u32,
    user_prompt: Option<String>,
    eval_result: Option<String>,
    first_verb: Option<String>,
    last_verb: Option<String>,
}

/// What an arc keeps of a closed exchange.
#[derive(Clone)]
struct ExchangeSummary {
    handle: String,
    event_ids: Vec<String>,
    ended_at: String,
    entities: BTreeMap<String, u32>,
    tools: BTreeMap<String, u32>,
    user_prompt: Option<String>,
    eval_result: Option<String>,
    last_verb: Option<String>,
}

impl ExchangeAcc {
    fn open(turn: &StructuralTurn, sentence: &TurnSentence) -> Self {
        let mut acc = ExchangeAcc {
            started_at: turn.timestamp.clone(),
            ended_at: turn_ended_at(turn),
            event_ids: Vec::new(),
            entities: BTreeMap::new(),
            tools: BTreeMap::new(),
            tools_by_role: BTreeMap::new(),
            turns: 0,
            rich_turns: 0,
            injected_count: 0,
            user_prompt: turn.human.as_ref().map(|h| h.content.clone()),
            eval_result: None,
            first_verb: Some(sentence.verb.clone()),
            last_verb: None,
        };
        acc.fold(turn, sentence);
        acc
    }

    fn fold(&mut self, turn: &StructuralTurn, sentence: &TurnSentence) {
        self.event_ids.extend(turn.event_ids.iter().cloned());
        self.ended_at = turn_ended_at(turn);
        merge_counts(&mut self.entities, &turn_entities(turn));
        for a in &turn.applies {
            bump(&mut self.tools, a.tool_name.clone(), 1);
            let role = classify_tool(&a.tool_name, &a.input_summary);
            bump(&mut self.tools_by_role, format!("{role:?}"), 1);
        }
        self.turns += 1;
        if !turn.applies.is_empty() {
            self.rich_turns += 1;
        }
        self.injected_count += turn.injected.len() as u32;
        if let Some(e) = &turn.eval {
            if !e.content.is_empty() {
                self.eval_result = Some(e.content.clone());
            }
        }
        self.last_verb = Some(sentence.verb.clone());
    }

    fn close(self, session_id: &str) -> (PatternEvent, ExchangeSummary) {
        let handle = handle("exchange", &self.event_ids);
        let summary_line = self
            .user_prompt
            .as_deref()
            .map(|p| clip(p, 80))
            .unwrap_or_else(|| "(continuation)".to_string());
        let pattern = PatternEvent {
            pattern_type: "story.exchange".to_string(),
            session_id: session_id.to_string(),
            event_ids: self.event_ids.clone(),
            started_at: self.started_at.clone(),
            ended_at: self.ended_at.clone(),
            summary: summary_line,
            metadata: serde_json::json!({
                "handle": handle,
                "user_prompt": self.user_prompt,
                "eval_result": self.eval_result.as_deref().map(|e| clip(e, 400)),
                "entities": self.entities,
                "tools": self.tools,
                "tools_by_role": self.tools_by_role,
                "turns": self.turns,
                "rich_turns": self.rich_turns,
                "injected_count": self.injected_count,
                "first_verb": self.first_verb,
                "last_verb": self.last_verb,
            }),
        };
        let summary = ExchangeSummary {
            handle,
            event_ids: self.event_ids,
            ended_at: self.ended_at,
            entities: self.entities,
            tools: self.tools,
            user_prompt: self.user_prompt,
            eval_result: self.eval_result,
            last_verb: self.last_verb,
        };
        (pattern, summary)
    }
}

/// How an arc closed. Mirrors `golden::ArcClose` on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closed {
    Gap,
    EndOfStream,
}

/// Accumulator for one arc.
struct ArcAcc {
    started_at: String,
    exchanges: Vec<ExchangeSummary>,
    ambiguous_seams: Vec<usize>,
}

impl ArcAcc {
    fn last(&self) -> &ExchangeSummary {
        self.exchanges
            .last()
            .expect("an open arc holds at least one exchange")
    }

    fn close(self, session_id: &str, arc_index: u32, closed_by: Closed) -> PatternEvent {
        let mut entities = BTreeMap::new();
        let mut tools = BTreeMap::new();
        let mut event_ids = Vec::new();
        for ex in &self.exchanges {
            merge_counts(&mut entities, &ex.entities);
            merge_counts(&mut tools, &ex.tools);
            event_ids.extend(ex.event_ids.iter().cloned());
        }
        let question = self.exchanges.first().and_then(|e| e.user_prompt.clone());
        let resolution = self
            .exchanges
            .iter()
            .rev()
            .find_map(|e| e.eval_result.clone());
        let handle = handle("arc", &event_ids);
        PatternEvent {
            pattern_type: "story.arc".to_string(),
            session_id: session_id.to_string(),
            event_ids,
            started_at: self.started_at.clone(),
            ended_at: self.last().ended_at.clone(),
            summary: question
                .as_deref()
                .map(|q| clip(q, 80))
                .unwrap_or_else(|| "(arc)".to_string()),
            metadata: serde_json::json!({
                "handle": handle,
                "arc_index": arc_index,
                "exchanges": self.exchanges.iter().map(|e| e.handle.clone()).collect::<Vec<_>>(),
                "question": question,
                "resolution": resolution.as_deref().map(|r| clip(r, 400)),
                "entities": entities,
                "tools": tools,
                "closed_by": match closed_by { Closed::Gap => "gap", Closed::EndOfStream => "end_of_stream" },
                "ambiguous_seams": self.ambiguous_seams,
            }),
        }
    }
}

/// Folds turns into exchanges and exchanges into arcs. State is only the
/// open accumulators and two counters; inputs are turns, outputs patterns.
pub struct StoryDetector {
    gap_threshold_secs: u64,
    session_id: Option<String>,
    exchange: Option<ExchangeAcc>,
    arc: Option<ArcAcc>,
    exchanges_closed: usize,
    arcs_closed: u32,
}

impl StoryDetector {
    pub fn new(gap_threshold_secs: u64) -> Self {
        Self {
            gap_threshold_secs,
            session_id: None,
            exchange: None,
            arc: None,
            exchanges_closed: 0,
            arcs_closed: 0,
        }
    }

    pub fn gap_threshold_secs(&self) -> u64 {
        self.gap_threshold_secs
    }

    fn session(&self) -> &str {
        self.session_id.as_deref().unwrap_or("")
    }

    /// Close the open exchange, emit it, and fold it into the open arc.
    fn close_exchange(&mut self) -> Option<PatternEvent> {
        let acc = self.exchange.take()?;
        let (pattern, summary) = acc.close(self.session());
        self.exchanges_closed += 1;
        match &mut self.arc {
            Some(arc) => arc.exchanges.push(summary),
            None => {
                self.arc = Some(ArcAcc {
                    started_at: pattern.started_at.clone(),
                    exchanges: vec![summary],
                    ambiguous_seams: Vec::new(),
                })
            }
        }
        Some(pattern)
    }

    fn close_arc(&mut self, closed_by: Closed) -> Option<PatternEvent> {
        let arc = self.arc.take()?;
        let pattern = arc.close(self.session(), self.arcs_closed, closed_by);
        self.arcs_closed += 1;
        Some(pattern)
    }
}

impl TurnDetector for StoryDetector {
    fn feed_turn(&mut self, turn: &StructuralTurn) -> Vec<PatternEvent> {
        if self.session_id.is_none() {
            self.session_id = Some(turn.session_id.clone());
        }
        let sentence = build_sentence(turn);
        let opens_exchange = turn.human.is_some();

        if !opens_exchange {
            match &mut self.exchange {
                Some(acc) => acc.fold(turn, &sentence),
                None => self.exchange = Some(ExchangeAcc::open(turn, &sentence)),
            }
            return Vec::new();
        }

        let mut out = Vec::new();
        out.extend(self.close_exchange());

        if let Some(arc) = &mut self.arc {
            let gap = secs(&turn.timestamp) - secs(&arc.last().ended_at);
            if gap > self.gap_threshold_secs as i64 {
                out.extend(self.close_arc(Closed::Gap));
            } else {
                let prev = arc.last();
                let closure = prev
                    .last_verb
                    .as_deref()
                    .map(is_closure_verb)
                    .unwrap_or(false);
                let opening = is_opening_verb(&sentence.verb);
                let new_entity = turn_entities(turn)
                    .keys()
                    .any(|k| !prev.entities.contains_key(k));
                if closure && opening && new_entity {
                    let seam = self.exchanges_closed;
                    arc.ambiguous_seams.push(seam);
                }
            }
        }

        self.exchange = Some(ExchangeAcc::open(turn, &sentence));
        out
    }

    fn flush(&mut self) -> Vec<PatternEvent> {
        let mut out = Vec::new();
        out.extend(self.close_exchange());
        out.extend(self.close_arc(Closed::EndOfStream));
        out
    }

    fn name(&self) -> &str {
        "story"
    }
}

// ═══════════════════════════════════════════════════════════════════
// Output shapes a host hands back (memory hands, E-03)
// ═══════════════════════════════════════════════════════════════════
//
// These are the contracts for judgment. They are committed as JSON
// schemas (schemas/*.schema.json), served by the MCP as resources, and
// embedded in the prompts. The write hands (group D) validate against
// them and against the laws in `validate_reading` before anything lands.

use serde::{Deserialize, Serialize};

/// Who read: the host that ran the prompt and the model it used.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Author {
    pub host: String,
    pub model: String,
}

/// A reading's standing: provisional readings come from the streaming
/// fold and are superseded by the final reading when the arc closes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Standing {
    Provisional,
    Final,
}

/// One paragraph of a reading: consecutive exchanges sharing an intent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Paragraph {
    /// Exchange handles, in arc order. Never a handle the arc does not hold.
    pub exchanges: Vec<String>,
    pub intent: String,
}

/// A reading: a grouping of an arc's exchanges into paragraphs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Reading {
    /// The arc handle this reading is of.
    pub handle: String,
    pub standing: Standing,
    pub paragraphs: Vec<Paragraph>,
    pub author: Author,
}

/// The slots the twelve creator-question schemas need.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Slots {
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub deferrals: Vec<String>,
    #[serde(default)]
    pub tradeoffs: Vec<String>,
    #[serde(default)]
    pub failures: Vec<String>,
    /// The human's positions, quoted, never strengthened.
    #[serde(default)]
    pub stance: Vec<String>,
}

/// An enrichment: the top-node text for an arc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Enrichment {
    pub handle: String,
    pub title: String,
    pub question: String,
    pub resolution: String,
    pub summary: String,
    #[serde(default)]
    pub slots: Slots,
    pub author: Author,
}

/// A ruling on an ambiguous seam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerdictKind {
    SameTheme,
    NewTheme,
}

/// A verdict on the seam before exchange index `seam` of an arc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Verdict {
    pub handle: String,
    pub seam: usize,
    pub verdict: VerdictKind,
    pub reason: String,
    pub author: Author,
}

// ═══════════════════════════════════════════════════════════════════
// The reading laws (E-04) — enforced here, never trusted to the model
// ═══════════════════════════════════════════════════════════════════

/// Why a reading is not a valid grouping of an arc's exchanges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadingError {
    /// Exchange handles of the arc that appear in no group.
    Missing(Vec<String>),
    /// Handles that appear in more than one group.
    Duplicated(Vec<String>),
    /// Handles that the arc does not hold: a reading never introduces one.
    Unknown(Vec<String>),
    /// The groups, concatenated, must follow the arc's order.
    OutOfOrder { expected: String, found: String },
    /// A paragraph with no exchanges (index into `paragraphs`).
    EmptyGroup(usize),
}

impl std::fmt::Display for ReadingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadingError::Missing(h) => write!(f, "exchanges in no group: {}", h.join(", ")),
            ReadingError::Duplicated(h) => {
                write!(f, "exchanges in more than one group: {}", h.join(", "))
            }
            ReadingError::Unknown(h) => {
                write!(f, "handles the arc does not hold: {}", h.join(", "))
            }
            ReadingError::OutOfOrder { expected, found } => {
                write!(
                    f,
                    "groups must follow the arc's order: expected {expected}, found {found}"
                )
            }
            ReadingError::EmptyGroup(i) => write!(f, "paragraph {i} has no exchanges"),
        }
    }
}

impl std::error::Error for ReadingError {}

/// Pure: check a reading against the arc's exchange handles, in arc order.
/// Every handle exactly once, in order, no strangers, no empty groups.
pub fn validate_reading(reading: &Reading, arc_exchanges: &[String]) -> Result<(), ReadingError> {
    use std::collections::{BTreeSet, HashSet};

    if let Some(i) = reading
        .paragraphs
        .iter()
        .position(|p| p.exchanges.is_empty())
    {
        return Err(ReadingError::EmptyGroup(i));
    }
    let arc: HashSet<&str> = arc_exchanges.iter().map(String::as_str).collect();
    let flat: Vec<&str> = reading
        .paragraphs
        .iter()
        .flat_map(|p| p.exchanges.iter().map(String::as_str))
        .collect();

    let unknown: Vec<String> = {
        let mut seen = BTreeSet::new();
        flat.iter()
            .filter(|h| !arc.contains(*h) && seen.insert(**h))
            .map(|h| h.to_string())
            .collect()
    };
    if !unknown.is_empty() {
        return Err(ReadingError::Unknown(unknown));
    }
    let duplicated: Vec<String> = {
        let mut seen = HashSet::new();
        let mut dups = BTreeSet::new();
        for h in &flat {
            if !seen.insert(*h) {
                dups.insert(h.to_string());
            }
        }
        dups.into_iter().collect()
    };
    if !duplicated.is_empty() {
        return Err(ReadingError::Duplicated(duplicated));
    }
    let present: HashSet<&str> = flat.iter().copied().collect();
    let missing: Vec<String> = arc_exchanges
        .iter()
        .filter(|h| !present.contains(h.as_str()))
        .cloned()
        .collect();
    if !missing.is_empty() {
        return Err(ReadingError::Missing(missing));
    }
    for (expected, found) in arc_exchanges.iter().zip(&flat) {
        if expected != found {
            return Err(ReadingError::OutOfOrder {
                expected: expected.clone(),
                found: found.to_string(),
            });
        }
    }
    Ok(())
}
