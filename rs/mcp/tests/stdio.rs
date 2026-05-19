//! Integration tests for the stdio transport.
//!
//! Tests use in-memory pipes (DuplexStream) rather than spawning the
//! binary — same code path as the binary's main(), zero process spawn.
//! The subscriber argument is a no-op `LoopbackSubscriber` since these
//! tests don't exercise streaming tools.

mod common;

use common::LoopbackSubscriber;
use open_story_mcp::stdio;
use tokio::io::AsyncWriteExt;

mod when_a_client_pipes_an_initialize_request_over_stdio {
    use super::*;

    #[tokio::test]
    async fn it_writes_the_response_to_stdout_on_one_line() {
        let (mut client_write, server_read) = tokio::io::duplex(4096);
        let (server_write, mut client_read) = tokio::io::duplex(4096);

        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, LoopbackSubscriber::new())
                .await
                .unwrap();
        });

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {}, "clientInfo": {"name": "test", "version": "0"}}
        });
        let mut line = serde_json::to_string(&request).unwrap();
        line.push('\n');
        client_write.write_all(line.as_bytes()).await.unwrap();
        // Closing stdin tells the server we're done.
        drop(client_write);

        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(&mut client_read).lines();
        let response_line = reader.next_line().await.unwrap().expect("expected one response line");

        let response: serde_json::Value = serde_json::from_str(&response_line).unwrap();
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["serverInfo"]["name"], "open-story-mcp");

        server.await.unwrap();
    }
}

mod when_a_client_pipes_multiple_requests_back_to_back {
    use super::*;

    #[tokio::test]
    async fn each_request_gets_its_own_response_line_in_order() {
        let (mut client_write, server_read) = tokio::io::duplex(4096);
        let (server_write, mut client_read) = tokio::io::duplex(4096);

        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, LoopbackSubscriber::new())
                .await
                .unwrap();
        });

        for id in 1..=3 {
            let req = serde_json::json!({"jsonrpc": "2.0", "id": id, "method": "tools/list"});
            let mut line = serde_json::to_string(&req).unwrap();
            line.push('\n');
            client_write.write_all(line.as_bytes()).await.unwrap();
        }
        drop(client_write);

        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(&mut client_read).lines();
        let mut got_ids: Vec<i64> = Vec::new();
        while let Some(line) = reader.next_line().await.unwrap() {
            let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
            got_ids.push(resp["id"].as_i64().unwrap());
        }
        assert_eq!(got_ids, vec![1, 2, 3]);

        server.await.unwrap();
    }
}

mod when_a_client_sends_a_notification_then_a_request {
    use super::*;

    #[tokio::test]
    async fn the_notification_emits_no_line_and_the_request_emits_one() {
        let (mut client_write, server_read) = tokio::io::duplex(4096);
        let (server_write, mut client_read) = tokio::io::duplex(4096);

        let server = tokio::spawn(async move {
            stdio::run(server_read, server_write, LoopbackSubscriber::new())
                .await
                .unwrap();
        });

        let notif = serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        let req = serde_json::json!({"jsonrpc": "2.0", "id": 42, "method": "tools/list"});
        for msg in [notif, req] {
            let mut line = serde_json::to_string(&msg).unwrap();
            line.push('\n');
            client_write.write_all(line.as_bytes()).await.unwrap();
        }
        drop(client_write);

        use tokio::io::AsyncBufReadExt;
        let mut reader = tokio::io::BufReader::new(&mut client_read).lines();
        let mut got_ids: Vec<i64> = Vec::new();
        while let Some(line) = reader.next_line().await.unwrap() {
            let resp: serde_json::Value = serde_json::from_str(&line).unwrap();
            if let Some(id) = resp["id"].as_i64() {
                got_ids.push(id);
            }
        }
        assert_eq!(got_ids, vec![42], "notification must not produce a response line");

        server.await.unwrap();
    }
}
