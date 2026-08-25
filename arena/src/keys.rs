use async_trait::async_trait;

#[async_trait]
#[allow(dead_code)]
pub trait KeyMinter: Send + Sync {
    async fn mint(&self, alias: &str, budget_usd: f64) -> anyhow::Result<String>;
    async fn revoke(&self, key: &str) -> anyhow::Result<()>;
}

#[allow(dead_code)]
pub struct FakeMinter {
    pub minted: std::sync::Mutex<Vec<(String, f64)>>,
    pub revoked: std::sync::Mutex<Vec<String>>,
}

impl FakeMinter {
    #[allow(dead_code)]
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(FakeMinter {
            minted: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl Default for FakeMinter {
    fn default() -> Self {
        FakeMinter {
            minted: std::sync::Mutex::new(Vec::new()),
            revoked: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl KeyMinter for FakeMinter {
    async fn mint(&self, alias: &str, budget_usd: f64) -> anyhow::Result<String> {
        let sanitized_alias = alias.replace('/', "-");
        let key = format!("sk-fake-{}", sanitized_alias);
        let mut minted = self.minted.lock().unwrap();
        minted.push((alias.to_string(), budget_usd));
        Ok(key)
    }

    async fn revoke(&self, key: &str) -> anyhow::Result<()> {
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
