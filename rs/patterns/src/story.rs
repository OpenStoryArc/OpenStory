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

use uuid::Uuid;

/// Verbs that close a piece of work. A seam where one of these is
/// followed by an opening verb on new entities is `Ambiguous` (A-07).
pub const CLOSURE_VERBS: &[&str] = &["committed", "pushed"];

/// Verbs that open a new line of work.
pub const OPENING_VERBS: &[&str] = &["explored", "read", "searched for"];

/// Content-addressed handle of a node: 16 hex characters derived from the
/// sorted event ids beneath it. Order-independent; stable across
/// re-narration because summaries and readings never feed into it (G-06).
pub fn handle(event_ids: &[String]) -> String {
    let mut ids: Vec<&str> = event_ids.iter().map(String::as_str).collect();
    ids.sort_unstable();
    let joined = ids.join("\n");
    let full = Uuid::new_v5(&Uuid::NAMESPACE_OID, joined.as_bytes());
    full.simple().to_string()[..16].to_string()
}

pub fn is_closure_verb(verb: &str) -> bool {
    CLOSURE_VERBS.contains(&verb)
}

pub fn is_opening_verb(verb: &str) -> bool {
    OPENING_VERBS.contains(&verb)
}
