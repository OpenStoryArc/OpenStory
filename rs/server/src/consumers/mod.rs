//! Independent actor-consumers for the decomposed ingest pipeline.
//!
//! Each consumer subscribes to NATS subjects and owns its own state.
//! They communicate through message-passing, not shared mutable state.
//!
//! Architecture (from CLAUDE.md):
//! > "The system is a network of independent actors communicating through messages.
//! > Each actor has a single responsibility and its own lifecycle."

pub mod admin_broadcaster;
pub mod broadcast;
#[cfg(test)]
pub mod broadcast_proptest;
pub mod patterns;
pub mod persist;
pub mod presence;
pub mod projections;
pub mod supervision;

/// The durable consumer each actor reads through, and where it starts the
/// first time it exists (B-11). Named per host and actor, so a restart finds
/// its own consumer and resumes from what it had not acknowledged.
///
/// - `persist`: the whole stream. It is the only way the fleet's events (the
///   mirror) and anything the watcher published but this store never wrote
///   reach the store; its primary-key dedup makes re-reading what the store
///   already holds a cheap no-op, and the bus bounds how much is in flight.
/// - `patterns`: new messages only. Its state grows with every session it
///   sees (a pipeline per session and every pattern it ever detected, kept
///   twice), so a whole-stream drain is memory the box does not have: inside
///   512 MiB it was OOM-killed seven seconds into a 0.8 GB backlog, where
///   persist alone drained it at a falling rss. Before B-11 every boot
///   re-read the whole stream through it into the store (the inserts are
///   idempotent), so an upgraded node already holds the history's patterns;
///   what persist's first drain adds for the first time (fleet events a
///   crash-looping node never stored) has none until they are re-derived.
/// - `projections`: new messages only. Boot replay rebuilds the read model
///   from the store, and the cache rebuilds any session it does not hold
///   from the store on first access; folding the stream again would hydrate
///   every session it touches, one after the other.
/// - `broadcast`: new messages only. It serves live WebSocket readers;
///   history reaches them over REST.
/// - `presence`: the last beat on each subject. The table keeps one latest
///   beat per node; the week of beats behind it changes nothing.
pub fn durable_spec(actor: &str, host: &str) -> open_story_bus::DurableSpec {
    use open_story_bus::StartFrom;
    let first_start = match actor {
        "persist" => StartFrom::All,
        "presence" => StartFrom::LastPerSubject,
        _ => StartFrom::New,
    };
    open_story_bus::DurableSpec {
        name: open_story_bus::durable_name(actor, host),
        first_start,
    }
}

#[cfg(test)]
mod durable_spec_tests {
    use super::durable_spec;
    use open_story_bus::StartFrom;

    #[test]
    fn each_actor_has_its_own_stable_durable_and_first_start() {
        let host = "mac-mini";
        let cases = [
            ("persist", StartFrom::All),
            ("patterns", StartFrom::New),
            ("projections", StartFrom::New),
            ("broadcast", StartFrom::New),
            ("presence", StartFrom::LastPerSubject),
        ];
        let mut names = std::collections::HashSet::new();
        for (actor, start) in cases {
            let spec = durable_spec(actor, host);
            assert_eq!(spec.first_start, start, "{actor}");
            assert_eq!(spec.name, format!("os-{actor}-{host}"));
            assert_eq!(spec, durable_spec(actor, host), "stable across calls");
            names.insert(spec.name);
        }
        assert_eq!(names.len(), 5, "one durable per actor");
    }
}
