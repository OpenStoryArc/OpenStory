//! Story layer primitives shared by the golden generator and the fold.
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
