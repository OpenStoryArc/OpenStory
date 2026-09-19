//! Story fold dogfood (memory hands, requirement X-05).
//!
//! Runs only when OPENSTORY_HOST points at a live OpenStory (for example
//! http://localhost:3002). Pulls one real session over REST, folds it, and
//! asserts the partition invariants from A-11 hold on real data. Nothing is
//! written anywhere; nothing real is committed (G-01).

use open_story_server::story_backfill::fold_session;
use std::collections::BTreeSet;

fn host() -> Option<String> {
    std::env::var("OPENSTORY_HOST")
        .ok()
        .filter(|h| !h.is_empty())
}

async fn get_json(client: &reqwest::Client, url: &str) -> serde_json::Value {
    client
        .get(url)
        .send()
        .await
        .unwrap_or_else(|e| panic!("GET {url}: {e}"))
        .json()
        .await
        .unwrap_or_else(|e| panic!("GET {url} json: {e}"))
}

mod when_a_real_store_is_reachable {
    use super::*;

    #[tokio::test]
    async fn the_fold_holds_the_partition_invariants() {
        let Some(host) = host() else {
            eprintln!("skipped: OPENSTORY_HOST not set");
            return;
        };
        let client = reqwest::Client::new();
        let sessions = get_json(&client, &format!("{host}/api/sessions")).await;
        let list = sessions
            .get("sessions")
            .and_then(|v| v.as_array())
            .cloned()
            .or_else(|| sessions.as_array().cloned())
            .expect("sessions list");

        // One busy session per agent: each transcript dialect is its own test.
        let agents = ["claude-code", "codex", "grok", "pi-mono"];
        let mut checked = 0;
        for agent in agents {
            let Some(pick) = list
                .iter()
                .filter(|s| s["origin_agent"].as_str() == Some(agent))
                .filter(|s| {
                    let n = s["event_count"].as_u64().unwrap_or(0);
                    n > 200 && n < 20_000
                })
                .max_by_key(|s| s["event_count"].as_u64().unwrap_or(0))
            else {
                eprintln!("dogfood {agent}: no session between 200 and 20k events, skipped");
                continue;
            };
            let sid = pick["session_id"]
                .as_str()
                .or(pick["id"].as_str())
                .unwrap()
                .to_string();
            let events = get_json(&client, &format!("{host}/api/sessions/{sid}/events")).await;
            let events = events
                .as_array()
                .cloned()
                .expect("bare array of CloudEvents");
            let prompts = events
                .iter()
                .filter(|e| e["subtype"].as_str() == Some("message.user.prompt"))
                .count();
            let patterns = fold_session(&events, 1800);

            let exchanges: Vec<_> = patterns
                .iter()
                .filter(|p| p.pattern_type == "story.exchange")
                .collect();
            let arcs: Vec<_> = patterns
                .iter()
                .filter(|p| p.pattern_type == "story.arc")
                .collect();
            assert!(
                !exchanges.is_empty(),
                "{agent}: a busy session folds to at least one exchange"
            );
            assert!(!arcs.is_empty(), "{agent}");
            if prompts >= 10 {
                assert!(exchanges.len() > 1, "{agent}: {prompts} prompts but a single exchange; the human-prompt rule is broken for this dialect");
            }

            // Partition: no event id in two exchanges; every exchange in exactly one arc, in order.
            let mut seen = BTreeSet::new();
            for ex in &exchanges {
                for id in &ex.event_ids {
                    assert!(
                        seen.insert(id.clone()),
                        "{agent}: event {id} appears in two exchanges"
                    );
                }
            }
            let handles: Vec<String> = exchanges
                .iter()
                .map(|p| p.metadata["handle"].as_str().unwrap().to_string())
                .collect();
            let unique: BTreeSet<&String> = handles.iter().collect();
            assert_eq!(
                unique.len(),
                handles.len(),
                "{agent}: exchange handles are unique"
            );
            let in_arcs: Vec<String> = arcs
                .iter()
                .flat_map(|a| {
                    a.metadata["exchanges"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|h| h.as_str().unwrap().to_string())
                        .collect::<Vec<_>>()
                })
                .collect();
            assert_eq!(
                in_arcs, handles,
                "{agent}: arcs partition the exchanges in order"
            );

            // Events no exchange covers: they never reached the fold (parse
            // failures) or fell outside every turn. Name their subtypes.
            let mut uncovered: std::collections::BTreeMap<String, usize> =
                std::collections::BTreeMap::new();
            for e in &events {
                let id = e["id"].as_str().unwrap_or("");
                if !seen.contains(id) {
                    *uncovered
                        .entry(e["subtype"].as_str().unwrap_or("?").to_string())
                        .or_insert(0) += 1;
                }
            }
            if !uncovered.is_empty() {
                eprintln!("dogfood {agent} uncovered subtypes: {uncovered:?}");
            }
            // Session-level enrichment (grok L2 recap/signals/summary) and
            // trailing file hunks live outside every turn by design; a
            // conversation message must never be left out of an exchange.
            for subtype in uncovered.keys() {
                assert!(
                    !subtype.starts_with("message."),
                    "{agent}: conversation event {subtype} is outside every exchange"
                );
            }
            let ambiguous = arcs
                .iter()
                .filter(|a| !a.metadata["ambiguous_seams"].as_array().unwrap().is_empty())
                .count();
            let injected: u64 = exchanges
                .iter()
                .map(|e| e.metadata["injected_count"].as_u64().unwrap_or(0))
                .sum();
            eprintln!(
                "dogfood {agent} {}: events={} prompts={} exchanges={} arcs={} ambiguous_arcs={} injected={} covered={}",
                &sid[..8],
                events.len(),
                prompts,
                exchanges.len(),
                arcs.len(),
                ambiguous,
                injected,
                seen.len()
            );
            checked += 1;
        }
        assert!(checked >= 1, "at least one agent had a sample session");
    }
}
