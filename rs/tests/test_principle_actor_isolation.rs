//! PRINCIPLE TEST — "Actor systems and message-passing."
//!
//! CLAUDE.md (principle #3):
//!
//!   > The system is a network of independent actors communicating through
//!   > messages. Each actor has a single responsibility and its own
//!   > lifecycle: the watcher observes files, the translator converts
//!   > formats, the ingester deduplicates and persists, the broadcaster
//!   > pushes to subscribers.
//!   >
//!   > Actors don't share mutable state. They send events.
//!
//! The cheapest testable shape of this principle: **actor modules don't
//! import each other**. The consumer actors (persist, patterns,
//! projections, broadcast) are defined in `rs/server/src/consumers/`.
//! If any of them imports another, the architectural boundary has
//! been violated — they'd be communicating through direct function
//! calls or shared state rather than the bus.
//!
//! A secondary shape: actor consumers don't import the shared
//! `AppState` or `SharedState`. Those belong to the composition
//! layer (`rs/src/server/mod.rs`), not to the actors themselves.
//! The existing exception is Actor 4 (broadcast) which is the last
//! consumer to decompose — the BroadcastConsumer struct in
//! `consumers/broadcast.rs` is correct (no AppState import); the
//! spawn wiring at `rs/src/server/mod.rs` still routes through
//! ingest_events + shared state, which is documented in the
//! decomposition plan. This test audits the STRUCTS, not the wiring.

use std::path::PathBuf;

/// Files that define actor consumers. Listed as workspace-relative paths.
const ACTOR_MODULES: &[&str] = &[
    "server/src/consumers/persist.rs",
    "server/src/consumers/patterns.rs",
    "server/src/consumers/projections.rs",
    "server/src/consumers/broadcast.rs",
];

/// Forbidden import patterns in an actor module.
/// Each pair: (pattern, reason).
const FORBIDDEN_IMPORTS: &[(&str, &str)] = &[
    ("use crate::consumers::", "actor modules must not import other actor modules"),
    ("use super::persist", "actor modules must not import each other"),
    ("use super::patterns", "actor modules must not import each other"),
    ("use super::projections", "actor modules must not import each other"),
    ("use super::broadcast", "actor modules must not import each other"),
    ("use crate::state::", "actors must not depend on shared AppState directly"),
    ("use crate::AppState", "actors must not depend on shared AppState directly"),
    ("use crate::SharedState", "actors must not depend on shared AppState directly"),
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn actor_isolation_consumer_modules_do_not_cross_reference() {
    let mut findings: Vec<(&str, usize, String, &'static str)> = Vec::new();

    for rel in ACTOR_MODULES {
        let path = workspace_root().join(rel);
        assert!(
            path.is_file(),
            "ACTOR_MODULES contains a nonexistent path: {rel}. \
             If a consumer was renamed, update the list."
        );
        let text = std::fs::read_to_string(&path).expect("read actor module");

        let mut in_test_block = false;
        for (i, line) in text.lines().enumerate() {
            if line.contains("#[cfg(test)]") || line.trim_start().starts_with("mod tests") {
                in_test_block = true;
            }
            if in_test_block {
                continue;
            }
            let trimmed = line.trim_start();
            // Only look at `use ...` lines — avoids false positives on
            // comments that mention "consumers::" or similar.
            if !trimmed.starts_with("use ") {
                continue;
            }
            for (pat, reason) in FORBIDDEN_IMPORTS {
                if line.contains(pat) {
                    findings.push((*rel, i + 1, line.trim().to_string(), *reason));
                    break;
                }
            }
        }
    }

    eprintln!(
        "\nscanned {} actor modules; {} forbidden-import sites found",
        ACTOR_MODULES.len(),
        findings.len()
    );

    if !findings.is_empty() {
        eprintln!("\n❌ actor-isolation violations:");
        for (rel, lineno, line, reason) in &findings {
            eprintln!("  {rel}:{lineno}  [{reason}]");
            eprintln!("      {line}");
        }
        panic!(
            "{} cross-actor or shared-state import(s) in consumer modules — \
             if intentional, document the exception here; if not, refactor \
             the call site to go through the bus instead",
            findings.len()
        );
    }
}
