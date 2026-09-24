//! H-02 / H-03: `/api/health` reports the boot phase with replay progress
//! and answers 503 until the node is serving; `/health` is liveness and
//! stays 200 whenever the process is up. Scoreboard: REQUIREMENTS.md

mod helpers;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request, test_state};
use open_story_server::boot;
use std::sync::Mutex;

/// The boot state is process-wide; these specs set it and must not overlap.
fn serial() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

mod when_replay_is_running {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_reports_phase_and_progress() {
        let _serial = serial();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        boot::set_replaying(7, 20, 1500);

        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let resp = send_request(state.clone(), req).await;
        let body = body_json(resp).await;
        assert_eq!(body["boot"]["phase"], "replaying", "{body}");
        assert_eq!(body["boot"]["replay"]["done"], 7);
        assert_eq!(body["boot"]["replay"]["total"], 20);
        assert_eq!(body["boot"]["replay"]["elapsed_ms"], 1500);

        boot::set_serving();
        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state, req).await).await;
        assert_eq!(body["boot"]["phase"], "serving");
        assert_eq!(
            body["boot"]["replay"]["done"], 20,
            "done equals total once serving"
        );
    }
}

mod when_replaying {
    use super::*;

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn it_returns_503_for_readiness_and_200_for_liveness() {
        let _serial = serial();
        let tmp = tempfile::tempdir().unwrap();
        let state = test_state(&tmp);
        boot::set_replaying(1, 10, 10);

        let ready = send_request(
            state.clone(),
            Request::get("/api/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(ready.status(), 503, "not ready while replaying");
        let body = body_json(ready).await;
        assert_eq!(
            body["boot"]["phase"], "replaying",
            "the body still explains itself"
        );

        let live = send_request(
            state.clone(),
            Request::get("/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(live.status(), 200, "alive is alive");

        boot::set_serving();
        let ready = send_request(
            state,
            Request::get("/api/health").body(Body::empty()).unwrap(),
        )
        .await;
        assert_eq!(ready.status(), 200);
    }
}
