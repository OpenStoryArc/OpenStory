//! Ops proposals and commands (REQUIREMENTS M-06): the authored events a
//! tier-1 hand leaves on the bus. A proposal says what an agent wants to
//! do and why (evidence: finding ids from the node's verdict); a command
//! says what the node did and how it went. Both ride `ops.>`, never
//! `events.*`: they are not agent history, they are ops history.

use serde_json::{json, Value};

use crate::cloud_event::CloudEvent;
use crate::event_data::EventData;

/// The tier-1 hands: they change only what is derived.
pub const HANDS: [&str; 5] = ["reproject", "verify", "catch_up", "prune", "converge"];

pub const AGENT: &str = "openstory";
pub const SOURCE: &str = "openstory-ops";

pub fn proposal_subject(hand: &str) -> String {
    format!("ops.proposal.{hand}")
}

pub fn command_subject(hand: &str) -> String {
    format!("ops.command.{hand}")
}

fn session_id(hand: &str) -> String {
    format!("ops:{hand}")
}

fn event(subtype: String, hand: &str, raw: Value) -> CloudEvent {
    let data = EventData::new(raw, 0, session_id(hand));
    CloudEvent::new(
        SOURCE.to_string(),
        "io.arc.event".to_string(),
        data,
        Some(subtype),
        None,
        None,
        None,
        None,
        Some(AGENT.to_string()),
    )
    .with_host(crate::host::host())
}

/// What an agent proposes: the hand, its arguments, who asks, the finding
/// ids that justify it, and the key that makes a retry harmless.
pub fn proposal_event(
    hand: &str,
    author: &str,
    evidence: &[String],
    idempotency_key: &str,
    args: Value,
) -> CloudEvent {
    event(
        proposal_subject(hand),
        hand,
        json!({
            "hand": hand,
            "author": author,
            "evidence": evidence,
            "idempotency_key": idempotency_key,
            "args": args,
        }),
    )
}

/// What the node did with a proposal, keyed the same way.
pub fn command_event(
    hand: &str,
    author: &str,
    evidence: &[String],
    idempotency_key: &str,
    ok: bool,
    result: Value,
) -> CloudEvent {
    event(
        command_subject(hand),
        hand,
        json!({
            "hand": hand,
            "author": author,
            "evidence": evidence,
            "idempotency_key": idempotency_key,
            "ok": ok,
            "result": result,
        }),
    )
}

/// The session id ops events ride under: one per hand, never an agent session.
pub fn ops_session_id(hand: &str) -> String {
    session_id(hand)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subjects_are_authored_never_observed() {
        assert_eq!(proposal_subject("reproject"), "ops.proposal.reproject");
        assert_eq!(command_subject("prune"), "ops.command.prune");
    }

    #[test]
    fn a_proposal_carries_who_why_and_key() {
        let ce = proposal_event(
            "verify",
            "mcp",
            &["projections_stale".into()],
            "k1",
            json!({"session_id": "s"}),
        );
        assert_eq!(ce.subtype.as_deref(), Some("ops.proposal.verify"));
        assert_eq!(ce.agent.as_deref(), Some(AGENT));
        assert_eq!(ce.data.raw["author"], "mcp");
        assert_eq!(ce.data.raw["evidence"][0], "projections_stale");
        assert_eq!(ce.data.raw["idempotency_key"], "k1");
        assert_eq!(ce.data.raw["args"]["session_id"], "s");
        assert_eq!(ce.data.session_id, "ops:verify");
    }

    #[test]
    fn a_command_carries_the_outcome() {
        let ce = command_event("prune", "mcp", &[], "k2", true, json!({"deleted": 3}));
        assert_eq!(ce.subtype.as_deref(), Some("ops.command.prune"));
        assert_eq!(ce.data.raw["ok"], true);
        assert_eq!(ce.data.raw["result"]["deleted"], 3);
        assert_eq!(ops_session_id("prune"), "ops:prune");
    }
}
