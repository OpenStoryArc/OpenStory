use axum::extract::{Form, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use axum_extra::extract::cookie::SignedCookieJar;
use serde::Deserialize;

use crate::authz::AuthzDecision;
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
/// logout, the `/authz` forward-auth endpoint, and the static form pages.
/// `/launch` is intentionally not registered here — that's Task 8.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/", get(root))
        .route("/register", get(get_register).post(post_register))
        .route("/login", get(get_login).post(post_login))
        .route("/logout", post(post_logout))
        .route("/authz", get(get_authz))
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

async fn post_register(
    State(state): State<AppState>,
    jar: SignedCookieJar,
    Form(form): Form<RegisterForm>,
) -> Response {
    let allowed = {
        let mut limiter = state.limiter.lock().unwrap();
        limiter.check(&format!("register:{}", form.username))
    };
    if !allowed {
        return (StatusCode::TOO_MANY_REQUESTS, "rate limited").into_response();
    }

    let event_name = match state.db.event_by_join_code(&form.join_code) {
        Ok(Some(name)) => name,
        Ok(None) => return (StatusCode::FORBIDDEN, "bad join code").into_response(),
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "storage error").into_response(),
    };

    if let Err(msg) = naming::validate_username(&form.username) {
        return (StatusCode::UNPROCESSABLE_ENTITY, msg).into_response();
    }

    let pass_hash = match auth::hash_password(&form.password) {
        Ok(h) => h,
        Err(_) => return (StatusCode::INTERNAL_SERVER_ERROR, "hashing error").into_response(),
    };

    if state
        .db
        .create_user(&form.username, &event_name, &pass_hash)
        .is_err()
    {
        return (StatusCode::CONFLICT, "username taken").into_response();
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
