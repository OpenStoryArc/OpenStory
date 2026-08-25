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
        ip_limiter: Arc::new(Mutex::new(RateLimiter::new(20, Duration::from_secs(60)))),
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

#[tokio::test]
async fn register_sets_a_fully_scoped_session_cookie() {
    let app = arena::routes::build_router(test_state());
    let resp = post_form(
        &app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("arena_session="), "{set_cookie}");
    // `session::set_session` sets `Domain=".arena.test"` (leading dot), but
    // the `cookie` crate (0.18.2, vendored by axum-extra 0.10) normalizes
    // the leading dot away when rendering `Set-Cookie` — see
    // `Cookie::domain()`'s getter, which does
    // `domain.strip_prefix(".").or(Some(domain))`, and which
    // `fmt_parameters`'s `Display` impl reads from. This is RFC
    // 6265-correct: a leading dot in the Domain attribute is semantically
    // redundant (subdomain matching applies either way), so
    // `Domain=arena.test` and `Domain=.arena.test` are equivalent on the
    // wire. Assert what's actually emitted.
    assert!(set_cookie.contains("Domain=arena.test"), "{set_cookie}");
    assert!(set_cookie.contains("Path=/"), "{set_cookie}");
    assert!(set_cookie.contains("HttpOnly"), "{set_cookie}");
    assert!(set_cookie.contains("Secure"), "{set_cookie}");
    assert!(set_cookie.contains("SameSite=Lax"), "{set_cookie}");
    assert!(set_cookie.contains("Max-Age=43200"), "{set_cookie}");
}

#[tokio::test]
async fn logout_with_a_valid_session_emits_a_removal_cookie() {
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

    // The removal Set-Cookie is only emitted when the jar the handler sees
    // has an *original* cookie under this name — i.e. one that arrived on
    // the incoming request, as it does here. Calling `clear_session` on a
    // bare jar with nothing ever added as "original" (e.g. in a unit test
    // that only ever calls `set_session` on the same in-memory jar) does
    // not exercise this path.
    let resp = post_form(&app, "/logout", "", Some(&cookie)).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);

    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(set_cookie.starts_with("arena_session="), "{set_cookie}");
    // See the comment in `register_sets_a_fully_scoped_session_cookie` on
    // why this is `Domain=arena.test` (no leading dot) despite
    // `clear_session` passing `.arena.test` — the `cookie` crate normalizes
    // it away when rendering, and both forms are RFC 6265-equivalent.
    assert!(set_cookie.contains("Domain=arena.test"), "{set_cookie}");
    assert!(set_cookie.contains("Path=/"), "{set_cookie}");
    assert!(
        set_cookie.contains("Max-Age=0") || set_cookie.contains("1970"),
        "expected a removal cookie (Max-Age=0 or an expiry in the past), got: {set_cookie}"
    );
}

#[tokio::test]
async fn register_with_invalid_username_is_422_and_does_not_create_a_user() {
    let state = test_state();
    let db = state.db.clone();
    let app = arena::routes::build_router(state);

    let resp = post_form(
        &app,
        "/register",
        "join_code=code-1&username=Bad_Name&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(resp.headers().get(header::SET_COOKIE).is_none());
    assert!(db.get_user("Bad_Name").unwrap().is_none());
}

#[tokio::test]
async fn register_with_reserved_username_is_422() {
    let app = arena::routes::build_router(test_state());
    let resp = post_form(
        &app,
        "/register",
        "join_code=code-1&username=admin&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNPROCESSABLE_ENTITY);
}
