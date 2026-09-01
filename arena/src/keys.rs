use async_trait::async_trait;

#[async_trait]
pub trait KeyMinter: Send + Sync {
    async fn mint(&self, alias: &str, budget_usd: f64) -> anyhow::Result<String>;
    async fn revoke(&self, key: &str) -> anyhow::Result<()>;
}

pub struct FakeMinter {
    pub minted: std::sync::Mutex<Vec<(String, f64)>>,
    pub revoked: std::sync::Mutex<Vec<String>>,
    /// Test hook: when set, `revoke` fails without recording anything in
    /// `revoked` — simulates a LiteLLM-side revoke failure. Defaults to
    /// `false`, so existing callers are unaffected.
    pub fail_revoke: std::sync::atomic::AtomicBool,
}

impl FakeMinter {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(FakeMinter {
            minted: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
            fail_revoke: std::sync::atomic::AtomicBool::new(false),
        })
    }
}

impl Default for FakeMinter {
    fn default() -> Self {
        FakeMinter {
            minted: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
            fail_revoke: std::sync::atomic::AtomicBool::new(false),
        }
    }
}

#[async_trait]
impl KeyMinter for FakeMinter {
    async fn mint(&self, alias: &str, budget_usd: f64) -> anyhow::Result<String> {
        // A real minter awaits a LiteLLM HTTP call here — a genuine
        // suspension point that concurrent callers can interleave across.
        // Yield once so tests exercising that interleaving (see
        // `concurrent_launches_for_same_user_mint_and_create_exactly_once`
        // in `tests/http_launch.rs`) see the same scheduling behavior with
        // this fake as they would against the real minter.
        tokio::task::yield_now().await;
        let sanitized_alias = alias.replace('/', "-");
        let key = format!("sk-fake-{}", sanitized_alias);
        let mut minted = self.minted.lock().unwrap();
        minted.push((alias.to_string(), budget_usd));
        Ok(key)
    }

    async fn revoke(&self, key: &str) -> anyhow::Result<()> {
        if self.fail_revoke.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("fake minter: forced revoke failure"));
        }
        let mut revoked = self.revoked.lock().unwrap();
        revoked.push(key.to_string());
        Ok(())
    }
}

/// A `KeyMinter` backed by a real LiteLLM proxy over HTTP.
///
/// Talks to LiteLLM's virtual-key endpoints directly: `POST /key/generate`
/// to mint a scoped virtual key with a budget, `POST /key/delete` to revoke
/// one. Both calls authenticate with the LiteLLM master key as a bearer
/// token.
pub struct LiteLlmMinter {
    base_url: String,
    master_key: String,
    http: reqwest::Client,
}

/// Default per-request timeout. A hung LiteLLM proxy must not hold the
/// `/launch` per-user lock forever — bound every mint/revoke call.
const DEFAULT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

impl LiteLlmMinter {
    pub fn new(base_url: String, master_key: String) -> Self {
        Self::with_timeout(base_url, master_key, DEFAULT_TIMEOUT)
    }

    /// Same as `new`, but with an explicit per-request timeout. Exists so
    /// tests can exercise the timeout path without waiting out the real
    /// 15s default.
    pub fn with_timeout(
        base_url: String,
        master_key: String,
        timeout: std::time::Duration,
    ) -> Self {
        LiteLlmMinter {
            // Trim a trailing slash once here so `format!("{base_url}/key/...")`
            // never produces a double slash for a base URL like
            // "http://litellm:4000/".
            base_url: base_url.trim_end_matches('/').to_string(),
            master_key,
            http: reqwest::Client::builder()
                .timeout(timeout)
                // The builder only fails on TLS backend initialization,
                // which is an unrecoverable environment failure — fail
                // loudly at boot rather than silently degrading to an
                // unbounded client.
                .build()
                .expect("reqwest client: TLS backend init failed"),
        }
    }
}

#[async_trait]
impl KeyMinter for LiteLlmMinter {
    async fn mint(&self, alias: &str, budget_usd: f64) -> anyhow::Result<String> {
        let resp = self
            .http
            .post(format!("{}/key/generate", self.base_url))
            .bearer_auth(&self.master_key)
            .json(&serde_json::json!({
                "key_alias": alias,
                "max_budget": budget_usd,
            }))
            .send()
            .await?;

        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(anyhow::anyhow!(
                "litellm /key/generate failed: {} — {}",
                status,
                body
            ));
        }

        let parsed: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            anyhow::anyhow!(
                "litellm /key/generate returned non-JSON body: {e} — {}",
                body
            )
        })?;
        match parsed.get("key").and_then(|v| v.as_str()) {
            Some(key) => Ok(key.to_string()),
            None => Err(anyhow::anyhow!(
                "litellm /key/generate response missing \"key\" string field: {}",
                body
            )),
        }
    }

    async fn revoke(&self, key: &str) -> anyhow::Result<()> {
        let resp = self
            .http
            .post(format!("{}/key/delete", self.base_url))
            .bearer_auth(&self.master_key)
            .json(&serde_json::json!({ "keys": [key] }))
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            // A key that's already gone (deleted by a previous revoke that
            // crashed after LiteLLM applied it but before we recorded
            // success, or deleted out-of-band) is not a revoke failure —
            // the end state ("this key no longer exists") is exactly what
            // revoke was asked to achieve. The reaper keeps the sandbox row
            // around to retry revoke on error, so treating "not found" as
            // an error here means retrying it every 60s forever. LiteLLM
            // signals this as a 404, or as a 4xx body naming the key as
            // not found — check both since the exact status LiteLLM uses
            // for this case isn't guaranteed stable across versions.
            let not_found = status == reqwest::StatusCode::NOT_FOUND
                || (status.is_client_error() && body.to_lowercase().contains("not found"));
            if not_found {
                return Ok(());
            }
            return Err(anyhow::anyhow!(
                "litellm /key/delete failed: {} — {}",
                status,
                body
            ));
        }
        // Success body (e.g. {"deleted_keys": [...]}) is intentionally
        // unread — revoke only needs to know the delete succeeded.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_minter_mints_and_revokes() {
        let m = FakeMinter::new();
        let key = m.mint("e/katie", 100.0).await.unwrap();
        assert_eq!(key, "sk-fake-e-katie");
        assert_eq!(m.minted.lock().unwrap()[0], ("e/katie".to_string(), 100.0));
        m.revoke(&key).await.unwrap();
        assert_eq!(m.revoked.lock().unwrap()[0], "sk-fake-e-katie");
    }

    // --- LiteLlmMinter: real HTTP KeyMinter against a stub axum server ---

    use axum::{extract::State, http::HeaderMap, routing::post, Json, Router};
    use std::sync::Arc;

    #[derive(Default)]
    struct Captured {
        generate_body: std::sync::Mutex<Option<serde_json::Value>>,
        generate_auth: std::sync::Mutex<Option<String>>,
        delete_body: std::sync::Mutex<Option<serde_json::Value>>,
        delete_auth: std::sync::Mutex<Option<String>>,
    }

    async fn stub_generate(
        State(captured): State<Arc<Captured>>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        *captured.generate_auth.lock().unwrap() = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        *captured.generate_body.lock().unwrap() = Some(body);
        Json(serde_json::json!({ "key": "sk-virt-1" }))
    }

    async fn stub_delete(
        State(captured): State<Arc<Captured>>,
        headers: HeaderMap,
        Json(body): Json<serde_json::Value>,
    ) -> Json<serde_json::Value> {
        *captured.delete_auth.lock().unwrap() = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        *captured.delete_body.lock().unwrap() = Some(body);
        Json(serde_json::json!({ "deleted_keys": [] }))
    }

    async fn spawn_stub(captured: Arc<Captured>) -> std::net::SocketAddr {
        let app = Router::new()
            .route("/key/generate", post(stub_generate))
            .route("/key/delete", post(stub_delete))
            .with_state(captured);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    async fn spawn_failing_stub(status: axum::http::StatusCode) -> std::net::SocketAddr {
        async fn fail(status: axum::http::StatusCode) -> axum::http::StatusCode {
            status
        }
        let app = Router::new()
            .route(
                "/key/generate",
                post(move || async move { fail(status).await }),
            )
            .route(
                "/key/delete",
                post(move || async move { fail(status).await }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn mint_posts_alias_and_budget_with_master_key_and_returns_key() {
        let captured = Arc::new(Captured::default());
        let addr = spawn_stub(captured.clone()).await;

        let m = LiteLlmMinter::new(format!("http://{addr}"), "master-1".into());

        let key = m.mint("e/katie", 5.0).await.unwrap();
        assert_eq!(key, "sk-virt-1");
        assert_eq!(
            captured.generate_auth.lock().unwrap().as_deref(),
            Some("Bearer master-1")
        );
        assert_eq!(
            captured.generate_body.lock().unwrap().as_ref().unwrap(),
            &serde_json::json!({"key_alias": "e/katie", "max_budget": 5.0})
        );

        m.revoke("sk-virt-1").await.unwrap();
        assert_eq!(
            captured.delete_auth.lock().unwrap().as_deref(),
            Some("Bearer master-1")
        );
        assert_eq!(
            captured.delete_body.lock().unwrap().as_ref().unwrap(),
            &serde_json::json!({"keys": ["sk-virt-1"]})
        );
    }

    #[tokio::test]
    async fn non_2xx_from_litellm_is_an_error() {
        let addr = spawn_failing_stub(axum::http::StatusCode::INTERNAL_SERVER_ERROR).await;
        let m = LiteLlmMinter::new(format!("http://{addr}"), "master-1".into());
        assert!(m.mint("e/katie", 5.0).await.is_err());
    }

    #[tokio::test]
    async fn revoke_of_an_already_deleted_key_is_treated_as_success() {
        // LiteLLM 404s a /key/delete for a key that's already gone (e.g.
        // the reaper crashed after the first revoke applied but before it
        // recorded success). Idempotent revoke must swallow that as Ok, or
        // the reaper retries the row forever.
        let addr = spawn_failing_stub(axum::http::StatusCode::NOT_FOUND).await;
        let m = LiteLlmMinter::new(format!("http://{addr}"), "master-1".into());
        assert!(m.revoke("sk-already-gone").await.is_ok());
    }

    #[tokio::test]
    async fn revoke_of_a_genuine_server_failure_is_still_an_error() {
        let addr = spawn_failing_stub(axum::http::StatusCode::INTERNAL_SERVER_ERROR).await;
        let m = LiteLlmMinter::new(format!("http://{addr}"), "master-1".into());
        assert!(m.revoke("sk-virt-1").await.is_err());
    }

    async fn spawn_slow_stub() -> std::net::SocketAddr {
        async fn slow_generate() -> Json<serde_json::Value> {
            // Longer than any timeout under test — the client must give up
            // on its own, not because the server ever answers.
            tokio::time::sleep(std::time::Duration::from_secs(20)).await;
            Json(serde_json::json!({ "key": "sk-too-late" }))
        }
        let app = Router::new().route("/key/generate", post(slow_generate));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        addr
    }

    #[tokio::test]
    async fn mint_times_out_when_litellm_hangs() {
        let addr = spawn_slow_stub().await;
        // A short timeout so this test completes quickly, not the real
        // 15s default — the timeout value itself is what's under test,
        // not its production magnitude.
        let m = LiteLlmMinter::with_timeout(
            format!("http://{addr}"),
            "master-1".into(),
            std::time::Duration::from_millis(500),
        );
        let started = std::time::Instant::now();
        let result = m.mint("e/katie", 5.0).await;
        assert!(result.is_err(), "hung server must surface as an error");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(5),
            "mint should give up around the configured timeout, not hang"
        );
    }

    #[tokio::test]
    async fn trailing_slash_in_base_url_does_not_break_the_path_join() {
        let captured = Arc::new(Captured::default());
        let addr = spawn_stub(captured.clone()).await;

        // A base_url with a trailing slash (e.g. "http://litellm:4000/")
        // must not produce a double-slash path like "//key/generate".
        let m = LiteLlmMinter::new(format!("http://{addr}/"), "master-1".into());

        let key = m.mint("e/katie", 5.0).await.unwrap();
        assert_eq!(key, "sk-virt-1");
    }
}
