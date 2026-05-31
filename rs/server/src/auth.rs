//! Bearer token authentication middleware.
//!
//! When `api_token` is configured, requests must present the token via one
//! of two channels:
//!   1. `Authorization: Bearer <token>` header (preferred — used by the
//!      REST API and tooling like curl).
//!   2. `?token=<token>` query parameter (fallback — used by browser
//!      WebSocket upgrades, which cannot set custom request headers).
//!
//! Empty config = pass-through (no auth).
//!
//! **Caveat on `?token=`:** URLs land in proxy access logs and the
//! browser's `Referer` header. The fallback exists only because the
//! browser WebSocket API gives no other way to pass a credential on
//! upgrade. Prefer the Bearer header from any non-browser caller.

use axum::extract::Request;
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::Response;

/// Constant-time comparison to prevent timing attacks on token values.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut result = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        result |= x ^ y;
    }
    result == 0
}

/// Extract `token=...` from a query string. Returns the raw token bytes
/// without URL-decoding — tokens are opaque bytes and decoding could
/// mask malformed input.
fn extract_query_token<'a>(query: &'a str) -> Option<&'a str> {
    for pair in query.split('&') {
        if let Some(rest) = pair.strip_prefix("token=") {
            return Some(rest);
        }
    }
    None
}

/// Axum middleware that validates token authentication via Bearer header
/// or `?token=` query param. See module docs for the channel rationale.
pub async fn auth_middleware(
    request: Request,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
    // No auth configured — pass through
    if expected_token.is_empty() {
        return Ok(next.run(request).await);
    }

    // Channel 1: Authorization: Bearer <token>
    let header_token = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    if let Some(token) = header_token {
        if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
            return Ok(next.run(request).await);
        }
        // Header present but wrong — don't fall through to query-param
        // check. An attacker who supplies both should never be helped
        // by trying multiple channels in one request.
        return Err(StatusCode::UNAUTHORIZED);
    }

    // Channel 2: ?token= query param (browser WS fallback)
    let query_token = request.uri().query().and_then(extract_query_token);
    if let Some(token) = query_token {
        if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
            return Ok(next.run(request).await);
        }
    }

    Err(StatusCode::UNAUTHORIZED)
}

/// Phase 6.1 — admin-only middleware. Distinct credential from `api_token`
/// so a read-only API caller can't escalate to policy writes.
///
/// Semantics:
///   - `admin_token` empty: fall back to `api_token` (backwards-compat for
///     single-token deployments — admin routes behave like any other auth'd
///     route).
///   - `admin_token` set: ONLY `admin_token` is accepted. Presenting
///     `api_token` on an admin route returns 403, never 200. This is the
///     escalation surface the test pins.
///
/// Same Bearer-or-query channels as `auth_middleware`, same constant-time
/// comparison. 403 (not 401) when a valid `api_token` is presented — the
/// caller IS authenticated, they're just not authorized for this surface.
pub async fn admin_only_middleware(
    request: Request,
    next: Next,
    api_token: String,
    admin_token: String,
) -> Result<Response, StatusCode> {
    // No admin separation configured: delegate to the api_token check.
    if admin_token.is_empty() {
        return auth_middleware(request, next, api_token).await;
    }

    let presented = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(|s| s.to_string())
        .or_else(|| {
            request
                .uri()
                .query()
                .and_then(extract_query_token)
                .map(|s| s.to_string())
        });

    let Some(token) = presented else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    if constant_time_eq(token.as_bytes(), admin_token.as_bytes()) {
        return Ok(next.run(request).await);
    }
    // Distinguish "valid api_token but wrong tier" from "garbage token":
    // both fail, but the former is a 403 (authorized, not permitted) and
    // the latter is a 401 (couldn't authenticate at all). The test on
    // line 6.1 pins this distinction.
    if !api_token.is_empty()
        && constant_time_eq(token.as_bytes(), api_token.as_bytes())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    Err(StatusCode::UNAUTHORIZED)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── constant_time_eq tests ──────────────────────────────────────────

    #[test]
    fn constant_time_eq_identical_strings() {
        assert!(constant_time_eq(b"secret-token-123", b"secret-token-123"));
    }

    #[test]
    fn constant_time_eq_different_strings() {
        assert!(!constant_time_eq(b"secret-token-123", b"wrong-token-456"));
    }

    #[test]
    fn constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"much-longer-string"));
    }

    #[test]
    fn constant_time_eq_empty_strings() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn constant_time_eq_one_empty() {
        assert!(!constant_time_eq(b"notempty", b""));
    }

    // ── Integration tests using axum test helpers ────────────────────────

    use axum::Router;
    use axum::body::Body;
    use axum::middleware;
    use axum::routing::get;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn test_app(token: &str) -> Router {
        let token = token.to_string();
        Router::new()
            .route("/test", get(|| async { "ok" }))
            .layer(middleware::from_fn(move |req, next| {
                let t = token.clone();
                async move { auth_middleware(req, next, t).await }
            }))
    }

    #[tokio::test]
    async fn no_token_configured_passes_through() {
        let app = test_app("");
        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = resp.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    #[tokio::test]
    async fn valid_bearer_token_returns_200() {
        let app = test_app("my-secret");
        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer my-secret")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn missing_auth_header_returns_401() {
        let app = test_app("my-secret");
        let req = Request::builder().uri("/test").body(Body::empty()).unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn wrong_token_returns_401() {
        let app = test_app("my-secret");
        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Bearer wrong-token")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn non_bearer_scheme_returns_401() {
        let app = test_app("my-secret");
        let req = Request::builder()
            .uri("/test")
            .header("Authorization", "Basic dXNlcjpwYXNz")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
