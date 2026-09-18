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

// ═══════════════════════════════════════════════════════════════════
// B-06 search · B-07 related · B-09 golden parity · B-10 ceilings
// ═══════════════════════════════════════════════════════════════════

use common::story_fixture::seed_spec;
use open_story_patterns::golden::{ExchangeKind, ExchangeSpec, GoldenSpec, PromptClass, TurnSpec};

fn t(verb: &str, objects: &[&str], tools: &[(&str, u32)]) -> TurnSpec {
    TurnSpec {
        verb: verb.to_string(),
        objects: objects.iter().map(|s| s.to_string()).collect(),
        tools: tools.iter().map(|(n, c)| (n.to_string(), *c)).collect(),
        rich: true,
    }
}

fn h(gap: u64, turns: Vec<TurnSpec>) -> ExchangeSpec {
    ExchangeSpec {
        kind: ExchangeKind::Human,
        prompt_class: PromptClass::Short,
        gap_before_secs: gap,
        turns,
    }
}

/// Three arcs: the first two share src/a.rs, the third shares nothing.
fn spec_shared_entities() -> GoldenSpec {
    GoldenSpec {
        session_id: "story-shared-entities".to_string(),
        started_at: "2026-01-01T09:00:00Z".to_string(),
        gap_threshold_secs: 1800,
        exchanges: vec![
            h(0, vec![t("edited", &["src/a.rs"], &[("Edit", 1)])]),
            h(
                2400,
                vec![t("edited", &["src/a.rs", "src/b.rs"], &[("Edit", 2)])],
            ),
            h(2400, vec![t("read", &["docs/z.md"], &[("Read", 1)])]),
        ],
    }
}

mod when_search_matches_an_entity {
    use super::*;

    #[tokio::test]
    async fn it_returns_the_arc_handle() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let arc1 = &expected["arcs"][1];
        let hits = call(
            server,
            "story_search",
            json!({ "query": "src/app.rs", "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let arcs: Vec<&serde_json::Value> = hits
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "arc")
            .collect();
        assert_eq!(arcs.len(), 1, "one arc names src/app.rs: {hits}");
        assert_eq!(arcs[0]["handle"], arc1["handle"]);
        assert!(arcs[0]["matched"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "entities"));
    }

    #[tokio::test]
    async fn a_prompt_phrase_finds_the_exchange() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let expected = golden_expected("two_arcs_gap");
        let hits = call(
            server,
            "story_search",
            json!({ "query": "GOLDEN PROMPT 3", "session_id": sids[0] }),
        )
        .await
        .unwrap();
        let ex: Vec<&serde_json::Value> = hits
            .as_array()
            .unwrap()
            .iter()
            .filter(|x| x["kind"] == "exchange")
            .collect();
        assert_eq!(ex.len(), 1, "case-insensitive match on the prompt: {hits}");
        assert_eq!(ex[0]["handle"], expected["exchanges"][3]["handle"]);
    }
}

mod when_two_arcs_share_entities {
    use super::*;

    #[tokio::test]
    async fn related_returns_the_other_first() {
        let (store, plan_store, _tmp) = make_test_store();
        let sid = seed_spec(&store, &spec_shared_entities()).await;
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .unwrap();
        assert_eq!(arcs.len(), 3);
        let h0 = arcs[0].metadata["handle"].as_str().unwrap().to_string();
        let h1 = arcs[1].metadata["handle"].as_str().unwrap().to_string();
        let h2 = arcs[2].metadata["handle"].as_str().unwrap().to_string();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);
        let related = call(
            server,
            "story_related",
            json!({ "handle": h0, "session_id": sid }),
        )
        .await
        .unwrap();
        let got = handles(&related);
        assert_eq!(
            got,
            vec![h1.clone()],
            "only the arc sharing src/a.rs, not {h2}"
        );
        assert_eq!(related[0]["shared"], json!(["src/a.rs"]));
    }
}

mod when_golden_is_walked {
    use super::*;

    #[tokio::test]
    async fn every_hand_matches_expected() {
        for name in [
            "single_arc_plain",
            "two_arcs_gap",
            "thin_turn_only",
            "ambiguous_closure_opening",
            "injected_skill_messages",
            "long_session_shape",
        ] {
            let expected = golden_expected(name);
            let want_arcs = expected["arcs"].as_array().unwrap();
            let want_ex = expected["exchanges"].as_array().unwrap();

            let (s, sids, _t) = server_with(&[name]).await;
            let list = call(s, "story_list", json!({ "session_id": sids[0] }))
                .await
                .unwrap();
            assert_eq!(
                handles(&list),
                want_arcs
                    .iter()
                    .map(|a| a["handle"].as_str().unwrap().to_string())
                    .collect::<Vec<_>>(),
                "{name}: list"
            );

            for arc in want_arcs {
                let (s, sids, _t) = server_with(&[name]).await;
                let summary = call(
                    s,
                    "story_summary",
                    json!({ "handle": arc["handle"], "session_id": sids[0] }),
                )
                .await
                .unwrap();
                assert_eq!(summary["down"], arc["exchanges"], "{name}: summary.down");
                let (s, sids, _t) = server_with(&[name]).await;
                let kids = call(
                    s,
                    "story_descend",
                    json!({ "node": arc["handle"], "session_id": sids[0] }),
                )
                .await
                .unwrap();
                assert_eq!(
                    json!(handles(&kids)),
                    arc["exchanges"],
                    "{name}: descend(arc)"
                );
            }

            for (i, ex) in want_ex.iter().enumerate() {
                let arc = want_arcs
                    .iter()
                    .find(|a| {
                        a["exchanges"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .any(|h| h == &ex["handle"])
                    })
                    .unwrap();
                let event_id = ex["event_ids"][0].as_str().unwrap();
                let (s, sids, _t) = server_with(&[name]).await;
                let up = call(
                    s,
                    "story_surface",
                    json!({ "node": event_id, "session_id": sids[0] }),
                )
                .await
                .unwrap();
                let up = up.as_array().unwrap();
                let ex_up = up
                    .iter()
                    .find(|a| a["kind"] == "exchange")
                    .unwrap_or_else(|| {
                        panic!("{name}: exchange {i} surfaces from its first event")
                    });
                assert_eq!(
                    ex_up["handle"], ex["handle"],
                    "{name}: surface → exchange {i}"
                );
                assert_eq!(
                    up.last().unwrap()["handle"],
                    arc["handle"],
                    "{name}: surface → arc"
                );
            }
        }
    }
}

mod when_hands_answer_on_a_golden {
    use super::*;

    async fn text_len(name: &str, args: serde_json::Value, golden: &str) -> usize {
        let (s, sids, _t) = server_with(&[golden]).await;
        let mut args = args;
        args["session_id"] = json!(sids[0]);
        let response = call_tool(s, name, args).await;
        response["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .len()
    }

    #[tokio::test]
    async fn responses_stay_under_the_byte_ceilings() {
        let expected = golden_expected("two_arcs_gap");
        let arc0 = expected["arcs"][0]["handle"].clone();
        let ex0 = expected["exchanges"][0]["handle"].clone();
        let ev0 = expected["exchanges"][0]["event_ids"][0].clone();
        let cases: Vec<(&str, serde_json::Value, usize)> = vec![
            ("story_list", json!({}), 1000),
            ("story_search", json!({ "query": "src/app.rs" }), 800),
            ("story_summary", json!({ "handle": arc0 }), 1200),
            ("story_descend", json!({ "node": arc0 }), 2400),
            ("story_surface", json!({ "node": ev0 }), 1200),
            ("story_context", json!({ "node": ex0 }), 3000),
            ("story_related", json!({ "handle": arc0 }), 600),
        ];
        for (name, args, ceiling) in cases {
            let n = text_len(name, args, "two_arcs_gap").await;
            assert!(n <= ceiling, "{name}: {n} bytes > ceiling {ceiling}");
        }
    }
}

mod when_related_is_asked_with_a_session_id {
    use super::*;

    /// A second session whose only arc shares src/a.rs with the first spec's arcs.
    fn spec_other_session() -> GoldenSpec {
        GoldenSpec {
            session_id: "story-other-session".to_string(),
            started_at: "2026-01-02T09:00:00Z".to_string(),
            gap_threshold_secs: 1800,
            exchanges: vec![h(0, vec![t("read", &["src/a.rs"], &[("Read", 1)])])],
        }
    }

    #[tokio::test]
    async fn it_still_looks_across_other_sessions() {
        let (store, plan_store, _tmp) = make_test_store();
        let sid = seed_spec(&store, &spec_shared_entities()).await;
        let other = seed_spec(&store, &spec_other_session()).await;
        let arcs = store
            .session_patterns(&sid, Some("story.arc"))
            .await
            .unwrap();
        let h0 = arcs[0].metadata["handle"].as_str().unwrap().to_string();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);
        let related = call(
            server,
            "story_related",
            json!({ "handle": h0, "session_id": sid }),
        )
        .await
        .unwrap();
        let sessions: Vec<&str> = related
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["session_id"].as_str().unwrap())
            .collect();
        assert!(
            sessions.contains(&other.as_str()),
            "pointers across reach other sessions even when the arc's session is given: {related}"
        );
        assert_eq!(sessions[0], sid, "the arc's own session comes first");
    }
}

// ═══════════════════════════════════════════════════════════════════
// D-05: story_summary carries what a host wrote
// ═══════════════════════════════════════════════════════════════════

mod when_an_enrichment_exists {
    use super::*;
    use open_story_patterns::story::{Author, MemoryKind, MemoryRecord, Standing};

    fn rec(
        sid: &str,
        handle: &str,
        kind: MemoryKind,
        standing: Option<Standing>,
        host: &str,
        payload: serde_json::Value,
    ) -> MemoryRecord {
        MemoryRecord::new(
            sid,
            handle,
            kind,
            standing,
            Author {
                host: host.into(),
                model: "m".into(),
            },
            "2026-09-18T20:00:00Z",
            payload,
        )
    }

    #[tokio::test]
    async fn story_summary_carries_the_title_and_author() {
        let (store, plan_store, _tmp) = make_test_store();
        let sid = seed_golden(&store, "two_arcs_gap").await;
        let expected = golden_expected("two_arcs_gap");
        let arc = &expected["arcs"][0];
        let h = arc["handle"].as_str().unwrap();
        store.insert_memory(&rec(&sid, h, MemoryKind::Enrichment, None, "claude-code",
            json!({ "handle": h, "title": "Plan, then build", "question": "q", "resolution": "r", "summary": "s",
                    "slots": { "decisions": ["hands, not agents"] } }))).await.unwrap();
        store.insert_memory(&rec(&sid, h, MemoryKind::Enrichment, None, "codex",
            json!({ "handle": h, "title": "A different title", "question": "q", "resolution": "r", "summary": "s" }))).await.unwrap();
        store.insert_memory(&rec(&sid, h, MemoryKind::Reading, Some(Standing::Final), "claude-code",
            json!({ "handle": h, "standing": "final", "paragraphs": [ { "exchanges": arc["exchanges"], "intent": "one" } ] }))).await.unwrap();
        store.insert_memory(&rec(&sid, h, MemoryKind::Verdict, None, "claude-code",
            json!({ "handle": h, "seam": 1, "verdict": "same_theme", "reason": "answers the first" }))).await.unwrap();
        let server = Server::new(LoopbackSubscriber::new(), store, plan_store);

        let summary = call(
            server,
            "story_summary",
            json!({ "handle": h, "session_id": sid }),
        )
        .await
        .unwrap();
        assert_eq!(
            summary["title"], "Plan, then build",
            "the first author's title is the headline"
        );
        assert_eq!(summary["slots"]["decisions"][0], "hands, not agents");
        let enrichments = summary["enrichments"].as_array().unwrap();
        assert_eq!(enrichments.len(), 2, "one per author");
        assert_eq!(enrichments[1]["author"]["host"], "codex");
        assert_eq!(enrichments[1]["title"], "A different title");
        let readings = summary["readings"].as_array().unwrap();
        assert_eq!(readings.len(), 1);
        assert_eq!(readings[0]["standing"], "final");
        assert_eq!(readings[0]["paragraphs"][0]["intent"], "one");
        assert_eq!(summary["verdicts"][0]["verdict"], "same_theme");
    }

    #[tokio::test]
    async fn without_one_the_fields_stay_absent() {
        let (server, sids, _tmp) = server_with(&["two_arcs_gap"]).await;
        let arc = golden_expected("two_arcs_gap")["arcs"][0].clone();
        let summary = call(
            server,
            "story_summary",
            json!({ "handle": arc["handle"], "session_id": sids[0] }),
        )
        .await
        .unwrap();
        assert!(summary.get("title").is_none());
        assert!(summary.get("enrichments").is_none());
        assert!(summary.get("readings").is_none());
    }
}
