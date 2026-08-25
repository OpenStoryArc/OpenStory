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
}
