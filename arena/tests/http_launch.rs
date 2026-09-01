mod common;

use axum::http::{header, StatusCode};
use chrono::{Duration as ChronoDuration, Utc};

use arena::driver::SandboxDriver;
use common::{post_form, session_cookie, test_state_with_fakes, BASE_DOMAIN};

async fn register_katie(app: &axum::Router) -> String {
    let resp = post_form(
        app,
        "/register",
        "join_code=code-1&username=katie&password=pw-123456",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    session_cookie(&resp)
}

fn location_of(resp: &axum::response::Response) -> String {
    resp.headers()
        .get(header::LOCATION)
        .expect("expected a Location header")
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn launch_creates_sandbox_mints_key_and_redirects_to_terminal_host() {
    let (state, driver, minter) = test_state_with_fakes();
    let db = state.db.clone();
    let app = arena::routes::build_router(state);

    let cookie = register_katie(&app).await;

    let resp = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    assert_eq!(location_of(&resp), format!("https://katie.{BASE_DOMAIN}"));

    let created = driver.created.lock().unwrap();
    assert_eq!(created.len(), 1);
    assert_eq!(created[0].username, "katie");
    assert_eq!(created[0].image, "img:1");
    drop(created);

    assert_eq!(
        *minter.minted.lock().unwrap(),
        vec![("e/katie".to_string(), 5.0)]
    );

    let row = db.get_sandbox("katie").unwrap().unwrap();
    let expected_expiry = Utc::now() + ChronoDuration::hours(6);
    let diff_secs = (row.expires_at - expected_expiry).num_seconds().abs();
    assert!(
        diff_secs < 60,
        "expires_at should be ~6h from now, diff was {diff_secs}s"
    );
}

#[tokio::test]
async fn second_launch_reuses_running_sandbox_and_does_not_mint_again() {
    let (state, driver, minter) = test_state_with_fakes();
    let app = arena::routes::build_router(state);
    let cookie = register_katie(&app).await;

    let first = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);
    let first_location = location_of(&first);

    let second = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(second.status(), StatusCode::SEE_OTHER);
    let second_location = location_of(&second);

    assert_eq!(first_location, second_location);
    assert_eq!(driver.created.lock().unwrap().len(), 1);
    assert_eq!(minter.minted.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn launch_after_crash_recreates_container_but_keeps_key() {
    let (state, driver, minter) = test_state_with_fakes();
    let app = arena::routes::build_router(state);
    let cookie = register_katie(&app).await;

    let first = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(first.status(), StatusCode::SEE_OTHER);

    // Simulate a crashed/reaped sandbox: the driver now reports not-running
    // for katie's container.
    driver.destroy("katie", true).await.unwrap();

    let second = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(second.status(), StatusCode::SEE_OTHER);

    assert_eq!(
        driver.created.lock().unwrap().len(),
        2,
        "a not-running sandbox must be recreated"
    );
    assert_eq!(
        minter.minted.lock().unwrap().len(),
        1,
        "the existing LiteLLM key must be reused, not re-minted"
    );
}

#[tokio::test]
async fn launch_without_session_is_redirected_to_login() {
    let (state, _driver, _minter) = test_state_with_fakes();
    let app = arena::routes::build_router(state);

    let resp = post_form(&app, "/launch", "", None).await;
    let status = resp.status();
    assert!(
        status == StatusCode::FOUND || status == StatusCode::SEE_OTHER,
        "expected a redirect (302 or 303), got {status}"
    );
    assert_eq!(location_of(&resp), "/");
}

#[tokio::test]
async fn launch_revokes_freshly_minted_key_when_container_create_fails() {
    let (state, driver, minter) = test_state_with_fakes();
    let db = state.db.clone();
    driver
        .fail_create
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let app = arena::routes::build_router(state);
    let cookie = register_katie(&app).await;

    let resp = post_form(&app, "/launch", "", Some(&cookie)).await;
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);

    assert_eq!(
        minter.minted.lock().unwrap().len(),
        1,
        "the key must still have been minted before the create failure"
    );
    assert!(
        minter
            .revoked
            .lock()
            .unwrap()
            .contains(&"sk-fake-e-katie".to_string()),
        "the orphaned key must be revoked when create fails after a fresh mint"
    );
    assert!(
        db.get_sandbox("katie").unwrap().is_none(),
        "no sandbox row should exist when create never succeeded"
    );
}

#[tokio::test]
async fn concurrent_launches_for_same_user_mint_and_create_exactly_once() {
    let (state, driver, minter) = test_state_with_fakes();
    let app = arena::routes::build_router(state);
    let cookie = register_katie(&app).await;

    let (first, second) = tokio::join!(
        post_form(&app, "/launch", "", Some(&cookie)),
        post_form(&app, "/launch", "", Some(&cookie)),
    );

    assert_eq!(first.status(), StatusCode::SEE_OTHER);
    assert_eq!(second.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        minter.minted.lock().unwrap().len(),
        1,
        "two concurrent launches for the same user must mint exactly once"
    );
    assert_eq!(
        driver.created.lock().unwrap().len(),
        1,
        "two concurrent launches for the same user must create exactly once"
    );
}
