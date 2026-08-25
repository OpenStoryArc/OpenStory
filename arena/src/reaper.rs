use chrono::{DateTime, Utc};
use std::time::Duration;

use crate::db::Db;
use crate::driver::SandboxDriver;
use crate::keys::KeyMinter;
use crate::state::AppState;

/// Destroys every expired sandbox: for each row `list_expired(now)` returns,
/// resolves the owning user's event manifest (user -> event), calls
/// `driver.destroy(username, keep_volume = manifest.retain_jsonl)`, then
/// `minter.revoke(&row.litellm_key)`, then `db.delete_sandbox(username)`.
///
/// If the user row or event manifest can't be resolved, the sandbox is
/// skipped entirely (logged via `eprintln`) rather than destroyed blind —
/// without the manifest we don't know whether to keep the JSONL volume.
///
/// A `driver.destroy` error is logged and does NOT stop the sweep for that
/// sandbox: we still attempt revoke + delete, because a container that
/// failed to tear down (or already vanished) must not strand a live LiteLLM
/// key or a stale DB row. Likewise a `minter.revoke` error is logged and we
/// still proceed to `delete_sandbox`.
///
/// Returns the number of sandboxes that made it all the way through to
/// `delete_sandbox` (i.e. were not skipped for missing user/manifest data).
pub async fn reap_once(
    db: &Db,
    driver: &dyn SandboxDriver,
    minter: &dyn KeyMinter,
    now: DateTime<Utc>,
) -> anyhow::Result<u32> {
    let expired = db.list_expired(now)?;
    let mut reaped = 0u32;

    for row in expired {
        let user = match db.get_user(&row.username) {
            Ok(Some(u)) => u,
            Ok(None) => {
                eprintln!(
                    "reaper: skipping sandbox for {:?}: no matching user row",
                    row.username
                );
                continue;
            }
            Err(e) => {
                eprintln!("reaper: skipping sandbox for {:?}: get_user failed: {e}", row.username);
                continue;
            }
        };

        let manifest = match db.get_event(&user.event) {
            Ok(Some(m)) => m,
            Ok(None) => {
                eprintln!(
                    "reaper: skipping sandbox for {:?}: no manifest for event {:?}",
                    row.username, user.event
                );
                continue;
            }
            Err(e) => {
                eprintln!(
                    "reaper: skipping sandbox for {:?}: get_event({:?}) failed: {e}",
                    row.username, user.event
                );
                continue;
            }
        };

        if let Err(e) = driver.destroy(&row.username, manifest.retain_jsonl).await {
            eprintln!("reaper: destroy failed for {:?}: {e}", row.username);
            // Intentionally fall through to revoke + delete: a container
            // that failed to tear down (or already vanished) must not
            // strand a live LiteLLM key or a stale DB row.
        }

        if let Err(e) = minter.revoke(&row.litellm_key).await {
            eprintln!("reaper: revoke failed for {:?}: {e}", row.username);
        }

        if let Err(e) = db.delete_sandbox(&row.username) {
            eprintln!("reaper: delete_sandbox failed for {:?}: {e}", row.username);
            continue;
        }

        reaped += 1;
    }

    Ok(reaped)
}

/// Spawns a tokio task that sweeps for expired sandboxes every 60 seconds,
/// forever. Errors from `reap_once` are logged, never fatal to the loop.
pub fn spawn_reaper(state: AppState) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            if let Err(e) = reap_once(&state.db, state.driver.as_ref(), state.minter.as_ref(), Utc::now()).await {
                eprintln!("reaper: sweep failed: {e}");
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::{Db, SandboxRow};
    use crate::driver::FakeDriver;
    use crate::keys::FakeMinter;
    use crate::manifest::EventManifest;
    use chrono::Duration as ChronoDuration;

    fn event(name: &str, retain_jsonl_line: &str) -> EventManifest {
        EventManifest::from_toml(&format!(
            "name = \"{name}\"\nimage = \"img:1\"\njoin_code = \"{name}-code\"\nttl_hours = 6\nbudget_usd = 5.0\n{retain_jsonl_line}"
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn reap_once_destroys_only_expired_revokes_key_and_respects_retain_jsonl() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("keep", "")).unwrap(); // retain_jsonl defaults true
        db.insert_event(&event("drop", "retain_jsonl = false")).unwrap();

        db.create_user("katie", "keep", "h").unwrap();
        db.create_user("bob", "drop", "h").unwrap();
        db.create_user("ann", "keep", "h").unwrap();

        let now = Utc::now();
        db.upsert_sandbox(&SandboxRow {
            username: "katie".into(),
            container_id: "c1".into(),
            litellm_key: "key-katie".into(),
            expires_at: now - ChronoDuration::hours(1),
        })
        .unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "bob".into(),
            container_id: "c2".into(),
            litellm_key: "key-bob".into(),
            expires_at: now - ChronoDuration::minutes(1),
        })
        .unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "ann".into(),
            container_id: "c3".into(),
            litellm_key: "key-ann".into(),
            expires_at: now + ChronoDuration::hours(1),
        })
        .unwrap();

        let driver = FakeDriver::new();
        let minter = FakeMinter::new();

        let reaped = reap_once(&db, driver.as_ref(), minter.as_ref(), now).await.unwrap();

        assert_eq!(reaped, 2);

        let destroyed = driver.destroyed.lock().unwrap().clone();
        assert!(destroyed.contains(&("katie".to_string(), true)));
        assert!(destroyed.contains(&("bob".to_string(), false)));
        assert!(!destroyed.iter().any(|(u, _)| u == "ann"));

        let revoked = minter.revoked.lock().unwrap().clone();
        assert!(revoked.contains(&"key-katie".to_string()));
        assert!(revoked.contains(&"key-bob".to_string()));

        assert!(db.get_sandbox("katie").unwrap().is_none());
        assert!(db.get_sandbox("bob").unwrap().is_none());
        assert!(db.get_sandbox("ann").unwrap().is_some());
    }

    #[tokio::test]
    async fn reap_once_with_nothing_expired_is_zero() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("keep", "")).unwrap();
        db.create_user("ann", "keep", "h").unwrap();

        let now = Utc::now();
        db.upsert_sandbox(&SandboxRow {
            username: "ann".into(),
            container_id: "c3".into(),
            litellm_key: "key-ann".into(),
            expires_at: now + ChronoDuration::hours(1),
        })
        .unwrap();

        let driver = FakeDriver::new();
        let minter = FakeMinter::new();

        let reaped = reap_once(&db, driver.as_ref(), minter.as_ref(), now).await.unwrap();

        assert_eq!(reaped, 0);
        assert!(driver.destroyed.lock().unwrap().is_empty());
        assert!(minter.revoked.lock().unwrap().is_empty());
    }
}
