use async_trait::async_trait;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
pub struct SandboxSpec {
    pub username: String,
    pub event: String,
    pub image: String,
    pub api_key: String,
    pub expires_at: DateTime<Utc>,
}

#[async_trait]
pub trait SandboxDriver: Send + Sync {
    async fn create(&self, spec: &SandboxSpec) -> anyhow::Result<String>;
    async fn destroy(&self, username: &str, keep_volume: bool) -> anyhow::Result<()>;
    async fn is_running(&self, container_id: &str) -> anyhow::Result<bool>;
}

pub struct FakeDriver {
    pub created: std::sync::Mutex<Vec<SandboxSpec>>,
    pub destroyed: std::sync::Mutex<Vec<(String, bool)>>,
}

impl FakeDriver {
    pub fn new() -> std::sync::Arc<Self> {
        std::sync::Arc::new(FakeDriver {
            created: std::sync::Mutex::new(Vec::new()),
            destroyed: std::sync::Mutex::new(Vec::new()),
        })
    }
}

impl Default for FakeDriver {
    fn default() -> Self {
        FakeDriver {
            created: std::sync::Mutex::new(Vec::new()),
            destroyed: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl SandboxDriver for FakeDriver {
    async fn create(&self, spec: &SandboxSpec) -> anyhow::Result<String> {
        let mut created = self.created.lock().unwrap();
        created.push(spec.clone());
        Ok(format!("fake-{}", spec.username))
    }

    async fn destroy(&self, username: &str, keep_volume: bool) -> anyhow::Result<()> {
        let mut destroyed = self.destroyed.lock().unwrap();
        destroyed.push((username.to_string(), keep_volume));
        Ok(())
    }

    async fn is_running(&self, container_id: &str) -> anyhow::Result<bool> {
        let created = self.created.lock().unwrap();
        let destroyed = self.destroyed.lock().unwrap();

        // Extract username from container_id (e.g., "fake-katie" -> "katie")
        if !container_id.starts_with("fake-") {
            return Ok(false);
        }

        let username = &container_id[5..];

        // Check if this username was created
        let was_created = created.iter().any(|spec| spec.username == username);

        // Check if this username was destroyed
        let was_destroyed = destroyed.iter().any(|(u, _)| u == username);

        Ok(was_created && !was_destroyed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(u: &str) -> SandboxSpec {
        SandboxSpec {
            username: u.into(),
            event: "e".into(),
            image: "img:1".into(),
            api_key: "sk-x".into(),
            expires_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn fake_driver_tracks_create_destroy_and_running_state() {
        let d = FakeDriver::new();
        let id = d.create(&spec("katie")).await.unwrap();
        assert_eq!(id, "fake-katie");
        assert!(d.is_running(&id).await.unwrap());
        d.destroy("katie", true).await.unwrap();
        assert!(!d.is_running(&id).await.unwrap());
        assert_eq!(d.destroyed.lock().unwrap()[0], ("katie".to_string(), true));
        d.destroy("katie", true).await.unwrap(); // idempotent
    }
}
