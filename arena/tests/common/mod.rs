//! Shared HTTP integration-test helpers for the arena crate's `tests/*.rs`
//! binaries. Each `tests/*.rs` file is compiled as its own crate, so not
//! every helper here is used by every consumer — `dead_code` is allowed
//! wholesale rather than per-item.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request};
use axum_extra::extract::cookie::Key;
use tower::ServiceExt;

use arena::auth::RateLimiter;
use arena::db::Db;
use arena::driver::FakeDriver;
use arena::keys::FakeMinter;
use arena::manifest::EventManifest;
use arena::state::{AppState, ArenaConfig};

pub const BASE_DOMAIN: &str = "arena.test";

/// Build an `AppState` wired to in-memory fakes, seeded with one event `"e"`
/// (join code `"code-1"`, image `"img:1"`, `ttl_hours = 6`,
/// `budget_usd = 5.0`). Convenience wrapper over
/// [`test_state_with_fakes`] for tests that don't need to inspect the fake
/// driver/minter's call history.
pub fn test_state() -> AppState {
    test_state_with_fakes().0
}

/// Same as [`test_state`], but also hands back the concrete
/// `Arc<FakeDriver>` / `Arc<FakeMinter>` so a test can inspect `created`,
/// `destroyed`, and `minted` after driving the router, or call
/// `driver.destroy(...)` directly to simulate a crashed sandbox.
pub fn test_state_with_fakes() -> (AppState, Arc<FakeDriver>, Arc<FakeMinter>) {
    let db = Db::open_in_memory().unwrap();
    let manifest = EventManifest::from_toml(
        "name = \"e\"\nimage = \"img:1\"\njoin_code = \"code-1\"\nttl_hours = 6\nbudget_usd = 5.0",
    )
    .unwrap();
    db.insert_event(&manifest).unwrap();

    let cfg = ArenaConfig {
        base_domain: BASE_DOMAIN.to_string(),
        cookie_key: Key::from(&[7u8; 64]),
        docker_runtime: None,
        litellm_url: "http://arena-litellm:4000".to_string(),
        listen: "0.0.0.0:8080".to_string(),
        db_path: PathBuf::from(":memory:"),
    };

    let driver = FakeDriver::new();
    let minter = FakeMinter::new();

    let state = AppState {
        db: Arc::new(db),
        driver: driver.clone(),
        minter: minter.clone(),
        cfg: Arc::new(cfg),
        limiter: Arc::new(Mutex::new(RateLimiter::new(5, Duration::from_secs(60)))),
        ip_limiter: Arc::new(Mutex::new(RateLimiter::new(20, Duration::from_secs(60)))),
    };

    (state, driver, minter)
}

pub async fn post_form(
    app: &axum::Router,
    path: &str,
    body: &str,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("POST")
        .uri(path)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let req = builder.body(Body::from(body.to_string())).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

pub async fn get_with_host(
    app: &axum::Router,
    path: &str,
    host: &str,
    cookie: Option<&str>,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method("GET")
        .uri(path)
        .header("X-Forwarded-Host", host);
    if let Some(c) = cookie {
        builder = builder.header(header::COOKIE, c);
    }
    let req = builder.body(Body::empty()).unwrap();
    app.clone().oneshot(req).await.unwrap()
}

pub fn session_cookie(resp: &axum::response::Response) -> String {
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("expected a Set-Cookie header")
        .to_str()
        .unwrap();
    set_cookie
        .split(';')
        .next()
        .expect("Set-Cookie header should have at least one part")
        .to_string()
}
