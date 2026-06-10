//! Bearer token authentication middleware.
//!
//! When `api_token` is configured, requests must present the token via one
//! of two channels:
//!   1. `Authorization: Bearer <token>` header (preferred — REST API, curl).
//!   2. `?token=<token>` query parameter (fallback — browser WebSocket
//!      upgrades, which cannot set custom request headers).
//!
//! Empty config = pass-through (no auth).
//!
//! **Caveat on `?token=`:** URLs land in proxy access logs and the browser's
//! `Referer` header. The fallback exists only because the browser WebSocket
//! API gives no other way to pass a credential on upgrade. Prefer the Bearer
//! header from any non-browser caller.

use axum::extract::Request;
use axum::http::{header, StatusCode};
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

/// Extract `token=...` from a query string. Returns the raw token without
/// URL-decoding — tokens are opaque bytes and decoding could mask malformed
/// input. Compared in constant time by the caller.
fn extract_query_token(query: &str) -> Option<&str> {
    query.split('&').find_map(|pair| pair.strip_prefix("token="))
}

/// Axum middleware that validates token authentication via the
/// `Authorization: Bearer` header or the `?token=` query param.
///
/// If `expected_token` is empty, all requests pass through (no auth configured).
/// Otherwise, a request is authorized if EITHER channel presents the correct
/// token. See module docs for why the query-param fallback exists.
pub async fn auth_middleware(
    request: Request,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
    // No auth configured — pass through
    if expected_token.is_empty() {
        return Ok(next.run(request).await);
    }

    // Channel precedence: if an Authorization header is present it is
    // authoritative — a present-but-wrong Bearer short-circuits to 401 and
    // does NOT fall through to the query param. An attacker who can supply
    // one channel must not get a second guess via the other. The `?token=`
    // fallback is consulted ONLY when no Authorization header is present
    // (the browser-WebSocket case).
    let candidate = match request.headers().get(header::AUTHORIZATION) {
        Some(value) => value.to_str().ok().and_then(|v| v.strip_prefix("Bearer ")),
        None => request.uri().query().and_then(extract_query_token),
    };

    match candidate {
        Some(token) if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) => {
            Ok(next.run(request).await)
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
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

    use axum::body::Body;
    use axum::middleware;
    use axum::routing::get;
    use axum::Router;
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
        let req = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

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
        let req = Request::builder()
            .uri("/test")
            .body(Body::empty())
            .unwrap();

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

    // ── ?token= query-param fallback (browser WebSocket upgrades) ────────

    #[test]
    fn extract_query_token_finds_token_anywhere_in_query() {
        assert_eq!(extract_query_token("token=abc"), Some("abc"));
        assert_eq!(extract_query_token("foo=1&token=abc"), Some("abc"));
        assert_eq!(extract_query_token("token=abc&bar=2"), Some("abc"));
        assert_eq!(extract_query_token("foo=1&bar=2"), None);
        // must not match a param that merely ends in `token`
        assert_eq!(extract_query_token("auth_token=abc"), None);
    }

    #[tokio::test]
    async fn valid_query_token_returns_200() {
        let app = test_app("my-secret");
        let req = Request::builder()
            .uri("/test?token=my-secret")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn wrong_query_token_returns_401() {
        let app = test_app("my-secret");
        let req = Request::builder()
            .uri("/test?token=wrong")
            .body(Body::empty())
            .unwrap();

        let resp = app.oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }
}
