//! B-06: give memory back (docs/research/openstory-as-node/
//! 2026-09-25-boot-pass-memory.md). The node says which allocator it runs
//! on `/api/health` (`process.allocator`), so a B-00 run and a fleet
//! roll can tell a jemalloc binary from a glibc one. The CLI crate's own
//! specs cover the jemalloc build; here the server default is `system`.
//!
//! Run with: cargo test -p open-story --test test_allocator

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story_server::node_health;

mod when_health_reports_the_process {
    use super::*;

    #[tokio::test]
    async fn it_names_the_allocator() {
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        assert_eq!(
            node_health::allocator(),
            "system",
            "nothing in this test binary installs an allocator"
        );

        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state, req).await).await;
        assert_eq!(body["process"]["allocator"], "system", "{body}");
        assert!(body["process"]["rss_bytes"].is_u64());
    }
}
