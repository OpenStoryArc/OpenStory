//! Bearer token authentication middleware.
//!
//! When `api_token` is configured, all API requests must include
//! `Authorization: Bearer <token>`. Empty config = pass-through (no auth).
//! WebSocket auth uses `?token=` query param (browsers can't set WS headers).

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

/// Axum middleware that validates Bearer token authentication.
///
/// If `expected_token` is empty, all requests pass through (no auth configured).
/// Otherwise, requests must include `Authorization: Bearer <token>` header.
pub async fn auth_middleware(
    request: Request,
    next: Next,
    expected_token: String,
) -> Result<Response, StatusCode> {
    // No auth configured — pass through
    if expected_token.is_empty() {
        return Ok(next.run(request).await);
    }

    // Check Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(value) if value.starts_with("Bearer ") => {
            let token = &value[7..];
            if constant_time_eq(token.as_bytes(), expected_token.as_bytes()) {
                Ok(next.run(request).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
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
}
