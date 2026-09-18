//! Read hands over the story layer (memory hands, requirements group B).
//!
//! Every test seeds a temp SqliteStore with a golden's events and folded
//! patterns, then calls the tool through the JSON-RPC harness.

mod common;

use common::story_fixture::{golden_expected, seed_golden};
use common::{call_tool, make_test_store, unwrap_tool_result, LoopbackSubscriber};
use open_story_mcp::server::Server;
use serde_json::json;

async fn server_with(
    goldens: &[&str],
) -> (Server<LoopbackSubscriber>, Vec<String>, tempfile::TempDir) {
    let (store, plan_store, tmp) = make_test_store();
    let mut sids = Vec::new();
    for g in goldens {
        sids.push(seed_golden(&store, g).await);
    }
    (
        Server::new(LoopbackSubscriber::new(), store, plan_store),
        sids,
        tmp,
    )
}

mod when_story_list_is_called_on_seeded_store {
    use super::*;

    #[tokio::test]
    async fn it_returns_one_line_per_arc() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let response = call_tool(server, "story_list", json!({ "session_id": sids[0] })).await;
        let lines = unwrap_tool_result(&response).expect("story_list ok");
        let lines = lines.as_array().expect("array of lines");
        assert_eq!(lines.len(), 2, "two_arcs_gap has two arcs");
        for (line, want) in lines.iter().zip(expected["arcs"].as_array().unwrap()) {
            assert_eq!(line["handle"], want["handle"]);
            assert_eq!(line["session_id"], json!(sids[0]));
            assert_eq!(
                line["exchanges"],
                json!(want["exchanges"].as_array().unwrap().len())
            );
            assert!(line["question"]
                .as_str()
                .unwrap()
                .starts_with("golden prompt"));
        }
        assert_eq!(lines[0]["arc_index"], 0);
        assert_eq!(lines[1]["arc_index"], 1);
    }

    #[tokio::test]
    async fn without_a_session_it_lists_every_session_newest_first() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap", "single_arc_plain"]).await;
        let response = call_tool(server, "story_list", json!({})).await;
        let lines = unwrap_tool_result(&response).expect("story_list ok");
        let lines = lines.as_array().unwrap();
        assert_eq!(lines.len(), 3, "2 + 1 arcs across two sessions");
        let sessions: std::collections::BTreeSet<&str> = lines
            .iter()
            .map(|l| l["session_id"].as_str().unwrap())
            .collect();
        assert_eq!(sessions.len(), 2);
        assert!(sids.iter().all(|s| sessions.contains(s.as_str())));
    }
}

mod when_story_summary_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_skeleton_fields_and_omits_enrichment_fields() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let arc0 = &expected["arcs"][0];
        let response = call_tool(
            server,
            "story_summary",
            json!({ "handle": arc0["handle"], "session_id": sids[0] }),
        )
        .await;
        let summary = unwrap_tool_result(&response).expect("story_summary ok");
        assert_eq!(summary["handle"], arc0["handle"]);
        assert_eq!(summary["session_id"], json!(sids[0]));
        assert!(summary["question"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 0"));
        assert!(summary["resolution"].is_string());
        assert_eq!(
            summary["down"], arc0["exchanges"],
            "down points at exchange handles"
        );
        assert_eq!(summary["entities"], arc0["entities"]);
        assert_eq!(summary["tools"], arc0["tools"]);
        assert!(summary["across"].is_array());
        assert!(
            summary.get("title").is_none(),
            "no enrichment yet: no title"
        );
        assert!(
            summary.get("slots").is_none(),
            "no enrichment yet: no slots"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════
// B-03 descend · B-04 surface · B-05 context · B-08 prefixes
// ═══════════════════════════════════════════════════════════════════

use common::story_fixture::seed_golden_as;

fn handles(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|n| n["handle"].as_str().unwrap().to_string())
        .collect()
}

async fn call(
    server: Server<LoopbackSubscriber>,
    name: &str,
    args: serde_json::Value,
) -> Result<serde_json::Value, String> {
    unwrap_tool_result(&call_tool(server, name, args).await)
}

mod when_descend_is_called_on_an_arc {
    use super::*;

    #[tokio::test]
    async fn it_returns_its_exchanges_in_order() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let arc0 = &expected["arcs"][0];
        let children = call(
            server,
            "story_descend",
            json!({ "node": arc0["handle"], "session_id": sids[0] }),
        )
        .await
        .unwrap();
        assert_eq!(json!(handles(&children)), arc0["exchanges"]);
        assert!(children
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["kind"] == "exchange"));
        assert!(children[0]["user_prompt"]
            .as_str()
            .unwrap()
            .starts_with("golden prompt 0"));
    }
}

mod when_descend_is_called_on_an_exchange {
    use super::*;

    #[tokio::test]
    async fn it_returns_its_sentences_whose_events_lie_inside_it() {
        let (server, sids, _tmp) = server_with(&["single_arc_plain"]).await;
        let expected = golden_expected("single_arc_plain");
        let ex1 = &expected["exchanges"][1];
        let children = call(
            server,
            "story_descend",
            json!({ "node": ex1["handle"], "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let sentences = children.as_array().unwrap();
        assert_eq!(sentences.len(), 1, "one turn, one sentence");
        assert_eq!(sentences[0]["kind"], "sentence");
        assert_eq!(sentences[0]["verb"], "edited");
        let ex_ids: std::collections::BTreeSet<&str> = ex1["event_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        for id in sentences[0]["event_ids"].as_array().unwrap() {
            assert!(
                ex_ids.contains(id.as_str().unwrap()),
                "sentence event inside the exchange"
            );
        }
        assert_eq!(
            sentences[0]["handle"].as_str().unwrap().len(),
            16,
            "sentences get a derived handle"
        );
    }
}

mod when_descend_is_called_on_a_sentence {
    use super::*;

    #[tokio::test]
    async fn it_returns_its_events_in_sequence() {
        let (server, sids, _tmp) = server_with(&["single_arc_plain"]).await;
        let expected = golden_expected("single_arc_plain");
        let ex1 = &expected["exchanges"][1];
        let (s2, s2ids, _t2) = server_with(&["single_arc_plain"]).await;
        let sentences = call(
            s2,
            "story_descend",
            json!({ "node": ex1["handle"], "session_id": s2ids[0] }),
        )
        .await
        .unwrap();
        let sentence_handle = sentences[0]["handle"].as_str().unwrap().to_string();
        let sentence_ids: Vec<String> = sentences[0]["event_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        let events = call(
            server,
            "story_descend",
            json!({ "node": sentence_handle, "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let ids: Vec<String> = events
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(ids, sentence_ids);
        assert!(events
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["kind"] == "event"));
        assert!(events[0]["subtype"].is_string());
    }
}

mod when_surface_is_called_on_an_event {
    use super::*;

    #[tokio::test]
    async fn it_returns_sentence_exchange_arc() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let ex2 = &expected["exchanges"][2];
        let arc1 = &expected["arcs"][1];
        let event_id = ex2["event_ids"][1].as_str().unwrap();
        let ancestors = call(
            server,
            "story_surface",
            json!({ "node": event_id, "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let kinds: Vec<&str> = ancestors
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, vec!["sentence", "exchange", "arc"]);
        assert_eq!(ancestors[1]["handle"], ex2["handle"]);
        assert_eq!(ancestors[2]["handle"], arc1["handle"]);
    }
}

mod when_context_is_called {
    use super::*;

    #[tokio::test]
    async fn it_equals_surface_plus_siblings() {
        let expected = golden_expected("two_arcs_gap");
        let ex0 = &expected["exchanges"][0];
        let arc0 = &expected["arcs"][0];

        let (s1, sids, _t1) = server_with(&["two_arcs_gap"]).await;
        let context = call(
            s1,
            "story_context",
            json!({ "node": ex0["handle"], "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let (s2, sids2, _t2) = server_with(&["two_arcs_gap"]).await;
        let surface = call(
            s2,
            "story_surface",
            json!({ "node": ex0["handle"], "session_id": sids2[0] }),
        )
        .await
        .unwrap();
        let (s3, sids3, _t3) = server_with(&["two_arcs_gap"]).await;
        let siblings = call(
            s3,
            "story_descend",
            json!({ "node": arc0["handle"], "session_id": sids3[0] }),
        )
        .await
        .unwrap();

        assert_eq!(context["node"]["handle"], ex0["handle"]);
        assert_eq!(context["node"]["kind"], "exchange");
        assert_eq!(context["ancestors"], surface);
        assert_eq!(context["siblings"], siblings);
    }
}

mod when_a_prefix_is_ambiguous {
    use super::*;

    #[tokio::test]
    async fn it_errors_with_candidates() {
        let (store, plan_store, _tmp) = make_test_store();
        let a = seed_golden(&store, "single_arc_plain").await;
        let b = seed_golden_as(&store, "single_arc_plain", "golden-single-arc-plain-copy").await;
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);
        let arc = &golden_expected("single_arc_plain")["arcs"][0];
        let prefix = &arc["handle"].as_str().unwrap()[..6];
        let err = call(server, "story_summary", json!({ "handle": prefix }))
            .await
            .unwrap_err();
        assert!(err.contains("ambiguous"), "{err}");
        assert!(
            err.contains(&a) && err.contains(&b),
            "candidates name both sessions: {err}"
        );
    }

    #[tokio::test]
    async fn a_short_prefix_is_rejected() {
        let (server, sids, _tmp) = server_with(&["single_arc_plain"]).await;
        let err = call(
            server,
            "story_summary",
            json!({ "handle": "ab", "session_id": sids[0] }),
        )
        .await
        .unwrap_err();
        assert!(err.contains("at least 4"), "{err}");
    }
}
