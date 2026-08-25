use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::cookie::SignedCookieJar;
use chrono::{Duration, Utc};
use serde::Deserialize;

use crate::authz::AuthzDecision;
use crate::db::{DbError, SandboxRow};
use crate::driver::SandboxSpec;
use crate::state::AppState;
use crate::{authz, auth, naming, pages, session};

#[derive(Debug, Deserialize)]
pub struct RegisterForm {
    pub join_code: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginForm {
    pub username: String,
    pub password: String,
}

/// Build the axum router for the arena HTTP surface: registration, login,
/// logout, the `/authz` forward-auth endpoint, `/launch` (sandbox
/// provisioning), and the static form pages.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(root))
        .route("/register", get(get_register).post(post_register))
        .route("/login", get(get_login).post(post_login))
        .route("/logout", post(post_logout))
        .route("/authz", get(get_authz))
        .route("/launch", post(post_launch))
        .with_state(state)
}

async fn healthz() -> &'static str {
    "ok"
}

async fn root(jar: SignedCookieJar) -> Html<String> {
    match session::session_user(&jar) {
        Some(username) => Html(pages::landing_page(&username)),
        None => Html(pages::login_page()),
    }
}

async fn get_register() -> Html<String> {
    Html(pages::register_page())
}

async fn get_login() -> Html<String> {
    Html(pages::login_page())
}

/// First comma-separated token of `X-Forwarded-For`, or `""` if absent.
///
/// This header is client-suppliable until Caddy overwrites it
/// unconditionally (a later task, same as the `X-Forwarded-Host` trust
/// precondition on `/authz` below) — until then, an attacker can put
/// anything here, including a value chosen to collide with a real client's
/// IP. It's still useful as a second, coarser rate-limit dimension once
/// that guarantee is in place; `RateLimiter::check`'s per-key pruning keeps
/// even an attacker-chosen flood of distinct values from growing this
/// limiter's map without bound.
fn client_ip(headers: &HeaderMap) -> String {
    headers
        .get("X-Forwarded-For")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

async fn post_register(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    headers: HeaderMap,
    Form(form): Form<RegisterForm>,
) -> Response {
    // Validate the username *before* touching either rate limiter, so both
    // limiters' keys are always bounded to `[a-z0-9-]{2,31}` /
    // `register-ip:{ip}` — never an attacker-controlled arbitrary string
    // used as `register:{username}`.
    if let Err(msg) = naming::validate_username(&form.username) {
        return (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response();
    }

    let allowed = {
        let mut limiter = state.limiter.lock().unwrap();
        limiter.check(&format!("register:{}", form.username))
    };
    if !allowed {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    let ip = client_ip(&headers);
    let ip_allowed = {
        let mut ip_limiter = state.ip_limiter.lock().unwrap();
        ip_limiter.check(&format!("register-ip:{ip}"))
    };
    if !ip_allowed {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    let event_name = match state.db.event_by_join_code(&form.join_code) {
        Ok(Some(name)) => name,
        Ok(None) => return (StatusCode::FORBIDDEN, "bad join code").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    let pass_hash = match auth::hash_password(&form.password) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "hashing error").into_response(),
    };

    match state.db.create_user(&form.username, &event_name, &pass_hash) {
        Ok(()) => {}
        Err(DbError::Duplicate) => return (StatusCode::CONFLICT, "username taken").into_response(),
        Err(DbError::Other(_)) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response()
        }
    }

    let jar = session::set_session(jar, &form.username, &state.cfg.base_domain);
    (jar, Redirect::to("/")).into_response()
}

async fn post_login(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Form(form): Form<LoginForm>,
) -> Response {
    let allowed = {
        let mut limiter = state.limiter.lock().unwrap();
        limiter.check(&format!("login:{}", form.username))
    };
    if !allowed {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    // Same-shaped failure for "unknown user" and "wrong password": no
    // user-enumeration oracle.
    let user = match state.db.get_user(&form.username) {
        Ok(Some(u)) => u,
        Ok(None) => return (StatusCode::UNAUTHORIZED, "bad credentials").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    if !auth::verify_password(&form.password, &user.pass_hash) {
        return (StatusCode::UNAUTHORIZED, "bad credentials").into_response();
    }

    let jar = session::set_session(jar, &user.username, &state.cfg.base_domain);
    (jar, Redirect::to("/")).into_response()
}

async fn post_logout(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let jar = session::clear_session(jar, &state.cfg.base_domain);
    (jar, Redirect::to("/")).into_response()
}

async fn get_authz(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    headers: HeaderMap,
) -> Response {
    // TRUST PRECONDITION: this endpoint's entire decision hinges on `host`.
    // Caddy MUST overwrite `X-Forwarded-Host` unconditionally on every
    // request it proxies to `/authz` (`header_up X-Forwarded-Host {host}`
    // in the forward_auth config — a later task), and `/authz` itself must
    // never be reachable directly from outside Caddy. Until both of those
    // are true, this header is client-suppliable: a request that reaches
    // this handler directly (bypassing Caddy) can set `X-Forwarded-Host` to
    // anything, including a value chosen to spoof `Allow` for a host it
    // doesn't actually own.
    let host = headers
        .get("X-Forwarded-Host")
        .or_else(|| headers.get(header::HOST))
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();

    let session_user = session::session_user(&jar);
    let decision = authz::authorize_host(session_user.as_deref(), host, &state.cfg.base_domain);

    match decision {
        AuthzDecision::Allow => {
            let mut resp = StatusCode::OK.into_response();
            let user_header = session_user.unwrap_or_default();
            resp.headers_mut().insert(
                "X-Arena-User",
                header::HeaderValue::from_str(&user_header).unwrap_or(header::HeaderValue::from_static("")),
            );
            resp
        }
        AuthzDecision::LoginRedirect => {
            let location = format!("https://{}/", state.cfg.base_domain);
            (StatusCode::FOUND, [(header::LOCATION, location)]).into_response()
        }
        AuthzDecision::Deny => (StatusCode::FORBIDDEN, "forbidden").into_response(),
    }
}

/// Idempotent sandbox provisioning.
///
/// - No session → redirect to `/` (the caller isn't logged in).
/// - Session but the event manifest is gone → 404 (an operator deleted the
///   event out from under a live user).
/// - An existing sandbox row whose container is still running → redirect
///   straight to the terminal host. No new mint, no new create — this is
///   the common "I already have a session, take me back" path.
/// - An existing row whose container is NOT running (crashed / reaped) →
///   recreate the container, reusing the *same* LiteLLM key so a crash
///   never mints a second key for one user.
/// - No row at all → mint a fresh key, create the container, persist the
///   row.
async fn post_launch(State(state): State<AppState>, jar: SignedCookieJar) -> Response {
    let username = match session::session_user(&jar) {
        Some(u) => u,
        None => return Redirect::to("/").into_response(),
    };

    let user = match state.db.get_user(&username) {
        Ok(Some(u)) => u,
        // A valid session cookie for a user row that no longer exists is
        // effectively "not logged in" from the app's perspective.
        Ok(None) => return Redirect::to("/").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    let event = match state.db.get_event(&user.event) {
        Ok(Some(e)) => e,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    let location = format!(
        "https://{}",
        naming::terminal_host(&username, &state.cfg.base_domain)
    );

    let existing = match state.db.get_sandbox(&username) {
        Ok(row) => row,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    // Reuse the existing LiteLLM key on a recreate (crashed sandbox); mint a
    // fresh one only when there's no prior row at all.
    let api_key = if let Some(row) = &existing {
        let running = state
            .driver
            .is_running(&row.container_id)
            .await
            .unwrap_or(false);
        if running {
            return redirect_to(location);
        }
        row.litellm_key.clone()
    } else {
        let alias = naming::key_alias(&user.event, &username);
        match state.minter.mint(&alias, event.budget_usd).await {
            Ok(key) => key,
            Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "mint error").into_response(),
        }
    };

    let expires_at = Utc::now() + Duration::hours(event.ttl_hours as i64);
    let spec = SandboxSpec {
        username: username.clone(),
        event: user.event.clone(),
        image: event.image.clone(),
        api_key: api_key.clone(),
        expires_at,
    };

    let container_id = match state.driver.create(&spec).await {
        Ok(id) => id,
        Err(_) => {
            return (StatusCode::INTERNAL_SERVER_ERROR, "sandbox creation error").into_response()
        }
    };

    let row = SandboxRow {
        username,
        container_id,
        litellm_key: api_key,
        expires_at,
    };
    if state.db.upsert_sandbox(&row).is_err() {
        return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response();
    }

    redirect_to(location)
}

fn redirect_to(location: String) -> Response {
    (StatusCode::SEE_OTHER, [(header::LOCATION, location)]).into_response()
}
