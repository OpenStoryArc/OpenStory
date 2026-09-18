//! MCP prompts — the intelligence instruction rendered per datum
//! (memory hands, requirement E-01). `prompts/list` advertises the
//! functions; `prompts/get { name, arguments }` returns the instruction
//! text plus the node's context so a host can act with nothing else.

mod common;

use common::story_fixture::{golden_expected, seed_golden};
use common::{make_test_store, LoopbackSubscriber};
use open_story_mcp::server::Server;
use open_story_mcp::stdio;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::time::timeout;

/// Send one JSON-RPC request over stdio and return its response.
async fn rpc(server: Server<LoopbackSubscriber>, request: Value) -> Value {
    let (mut client_write, server_read) = tokio::io::duplex(1 << 16);
    let (server_write, client_read) = tokio::io::duplex(1 << 16);
    let task =
        tokio::spawn(async move { stdio::run(server_read, server_write, server).await.unwrap() });
    client_write
        .write_all(format!("{request}\n").as_bytes())
        .await
        .unwrap();
    let mut reader = BufReader::new(client_read).lines();
    let line = timeout(Duration::from_secs(5), reader.next_line())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(client_write);
    timeout(Duration::from_millis(500), task)
        .await
        .unwrap()
        .unwrap();
    serde_json::from_str(&line).unwrap()
}

async fn seeded(golden: &str) -> (Server<LoopbackSubscriber>, String, tempfile::TempDir) {
    let (store, plan_store, tmp) = make_test_store();
    let sid = seed_golden(&store, golden).await;
    (
        Server::new(LoopbackSubscriber::new(), store, plan_store),
        sid,
        tmp,
    )
}

mod when_the_host_initializes {
    use super::*;

    #[tokio::test]
    async fn capabilities_advertise_prompts() {
        let (server, _sid, _tmp) = seeded("two_arcs_gap").await;
        let resp = rpc(
            server,
            json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {} }),
        )
        .await;
        assert!(
            resp["result"]["capabilities"]["prompts"].is_object(),
            "{resp}"
        );
    }

    #[tokio::test]
    async fn prompts_list_names_the_memory_functions() {
        let (server, _sid, _tmp) = seeded("two_arcs_gap").await;
        let resp = rpc(
            server,
            json!({ "jsonrpc": "2.0", "id": 2, "method": "prompts/list", "params": {} }),
        )
        .await;
        let names: Vec<&str> = resp["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["name"].as_str().unwrap())
            .collect();
        for want in [
            "narrate_arc",
            "segment_arc",
            "read_exchange",
            "adjudicate_seam",
            "remember",
            "curate",
        ] {
            assert!(
                names.contains(&want),
                "prompts/list names {want}: {names:?}"
            );
        }
        let narrate = resp["result"]["prompts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["name"] == "narrate_arc")
            .unwrap();
        let args: Vec<&str> = narrate["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert_eq!(args[0], "handle");
        assert_eq!(narrate["arguments"][0]["required"], true);
    }
}

mod when_prompts_get_narrate_arc_is_called {
    use super::*;

    #[tokio::test]
    async fn it_returns_instructions_and_the_arc_context() {
        let (server, sid, _tmp) = seeded("two_arcs_gap").await;
        let expected = golden_expected("two_arcs_gap");
        let arc0 = &expected["arcs"][0];
        let resp = rpc(
            server,
            json!({ "jsonrpc": "2.0", "id": 3, "method": "prompts/get",
                "params": { "name": "narrate_arc", "arguments": { "handle": arc0["handle"], "session_id": sid } } }),
        )
        .await;
        assert!(resp.get("error").is_none(), "{resp}");
        let messages = resp["result"]["messages"].as_array().unwrap();
        assert!(messages.len() >= 2, "instruction and context: {resp}");

        // The instruction: what to produce, in what shape, under what budget.
        let instruction = messages[0]["content"]["text"].as_str().unwrap();
        for word in [
            "title",
            "question",
            "resolution",
            "slots",
            "author",
            "handle",
        ] {
            assert!(instruction.contains(word), "instruction mentions {word}");
        }
        assert!(
            instruction.contains("never") && instruction.contains("handle"),
            "says handles must not be invented"
        );

        // The context: the arc's story_context as an embedded resource, naming its exchanges.
        let ctx = messages
            .iter()
            .find(|m| m["content"]["type"] == "resource")
            .expect("embedded resource");
        let text = ctx["content"]["resource"]["text"].as_str().unwrap();
        let ctx_json: Value = serde_json::from_str(text).unwrap();
        assert_eq!(ctx_json["context"]["node"]["handle"], arc0["handle"]);
        assert!(
            ctx_json["beneath"].is_array(),
            "the arc's exchanges ride along"
        );
        for h in arc0["exchanges"].as_array().unwrap() {
            assert!(
                text.contains(h.as_str().unwrap()),
                "context names exchange {h}"
            );
        }
        assert!(
            text.contains("golden prompt 0"),
            "context carries the opening prompt"
        );
    }

    #[tokio::test]
    async fn an_unknown_prompt_is_an_error() {
        let (server, _sid, _tmp) = seeded("two_arcs_gap").await;
        let resp = rpc(server, json!({ "jsonrpc": "2.0", "id": 4, "method": "prompts/get", "params": { "name": "nope", "arguments": {} } })).await;
        assert!(resp.get("error").is_some(), "{resp}");
    }
}

mod when_a_host_reads_the_schema_resources {
    use super::*;

    #[tokio::test]
    async fn resources_list_names_the_three_and_read_returns_json_schema() {
        let (server, _sid, _tmp) = seeded("two_arcs_gap").await;
        let resp = rpc(
            server,
            json!({ "jsonrpc": "2.0", "id": 5, "method": "resources/list", "params": {} }),
        )
        .await;
        let uris: Vec<&str> = resp["result"]["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["uri"].as_str().unwrap())
            .collect();
        for want in [
            "openstory://schemas/reading",
            "openstory://schemas/enrichment",
            "openstory://schemas/verdict",
        ] {
            assert!(uris.contains(&want), "resources/list has {want}: {uris:?}");
        }
        let (server, _sid, _tmp) = seeded("two_arcs_gap").await;
        let resp = rpc(server, json!({ "jsonrpc": "2.0", "id": 6, "method": "resources/read", "params": { "uri": "openstory://schemas/reading" } })).await;
        let content = &resp["result"]["contents"][0];
        assert_eq!(content["mimeType"], "application/schema+json");
        let schema: Value = serde_json::from_str(content["text"].as_str().unwrap()).unwrap();
        assert!(schema["properties"]["paragraphs"].is_object(), "{schema}");
    }
}
