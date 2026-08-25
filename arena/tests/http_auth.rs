use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use axum_extra::extract::cookie::Key;
use tower::ServiceExt;

use arena::auth::RateLimiter;
use arena::db::Db;
use arena::driver::FakeDriver;
use arena::keys::FakeMinter;
use arena::manifest::EventManifest;
use arena::state::{AppState, ArenaConfig};

const BASE_DOMAIN: &str = "arena.test";

fn test_state() -> AppState {
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

    AppState {
        db: Arc::new(db),
        driver: FakeDriver::new(),
        minter: FakeMinter::new(),
        cfg: Arc::new(cfg),
        limiter: Arc::new(Mutex::new(RateLimiter::new(5, Duration::from_secs(60)))),
    }
}

async fn post_form(
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

async fn get_with_host(
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

fn session_cookie(resp: &axum::response::Response) -> String {
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

#[tokio::test]
async fn register_with_valid_join_code_sets_session_and_redirects() {
    let app = arena::routes::build_router(test_state());
    let resp = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert!(resp
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .contains("arena_session"));
}

#[tokio::test]
async fn register_with_wrong_join_code_is_403() {
    let app = arena::routes::build_router(test_state());
    let resp = post_form(
        &app,
        "/register",
        "join_code=nope&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    assert!(resp.headers().get(header::SET_COOKIE).is_none());
}

#[tokio::test]
async fn register_duplicate_username_is_409() {
    let app = arena::routes::build_router(test_state());
    let first = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    let second = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=other-password",
        None,
    )
    .await;
    assert_eq!(second.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn login_roundtrip_and_bad_password_is_401() {
    let app = arena::routes::build_router(test_state());
    let reg = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(reg.status(), StatusCode::SEE_OTHER);

    let good = post_form(&app, "/login", "username=katie&password=pw-123456", None).await;
    assert_eq!(good.status(), StatusCode::SEE_OTHER);
    assert!(good.headers().get(header::SET_COOKIE).is_some());

    let bad = post_form(&app, "/login", "username=katie&password=wrong-pw", None).await;
    assert_eq!(bad.status(), StatusCode::UNAUTHORIZED);

    let unknown = post_form(
        &app,
        "/login",
        "username=nobody&password=wrong-pw",
        None,
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn sixth_rapid_login_attempt_is_rate_limited_429() {
    let app = arena::routes::build_router(test_state());
    for _ in 0..5 {
        let resp = post_form(
            &app,
            "/login",
            "username=rate-limited&password=wrong-pw",
            None,
        )
        .await;
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
    let sixth = post_form(
        &app,
        "/login",
        "username=rate-limited&password=wrong-pw",
        None,
    )
    .await;
    assert_eq!(sixth.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn authz_allows_own_subdomain_denies_others_redirects_anonymous() {
    let app = arena::routes::build_router(test_state());
    let reg = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(reg.status(), StatusCode::SEE_OTHER);
    let cookie = session_cookie(&reg);

    let own = get_with_host(&app, "/authz", "katie.arena.test", Some(&cookie)).await;
    assert_eq!(own.status(), StatusCode::OK);
    assert_eq!(
        own.headers().get("X-Arena-User").unwrap().to_str().unwrap(),
        "katie"
    );

    let own_story = get_with_host(&app, "/authz", "katie-story.arena.test", Some(&cookie)).await;
    assert_eq!(own_story.status(), StatusCode::OK);

    let others = get_with_host(&app, "/authz", "bob.arena.test", Some(&cookie)).await;
    assert_eq!(others.status(), StatusCode::FORBIDDEN);

    let anon = get_with_host(&app, "/authz", "katie.arena.test", None).await;
    assert_eq!(anon.status(), StatusCode::FOUND);
    assert_eq!(
        anon.headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap(),
        "https://arena.test/"
    );
}
