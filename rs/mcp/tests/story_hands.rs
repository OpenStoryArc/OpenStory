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
