//! Pure(ish) command implementations behind `arena`'s five subcommands.
//!
//! `cmd_up`, `cmd_down`, and `cmd_users` take `&Db` / `&dyn SandboxDriver` /
//! `&dyn KeyMinter` so they're testable against `FakeDriver`/`FakeMinter`
//! without a Docker daemon or a LiteLLM proxy. `cmd_serve` and the two
//! `*_from_env` helpers are the real-infra wiring, shared between `serve`
//! (the HTTP server) and `down` (which needs the same real
//! `DockerDriver`/`LiteLlmMinter` to actually tear sandboxes down) — see
//! `main.rs`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::auth::{generate_password, hash_password, RateLimiter};
use crate::db::Db;
use crate::docker_driver::{DockerDriver, DockerDriverConfig};
use crate::driver::SandboxDriver;
use crate::keys::{KeyMinter, LiteLlmMinter};
use crate::manifest::EventManifest;
use crate::naming::validate_username;
use crate::reaper::spawn_reaper;
use crate::routes::build_router;
use crate::state::{AppState, ArenaConfig};

/// Default per-username / per-IP rate limits, shared by `cmd_serve` and the
/// tests in `arena/tests/common/mod.rs` (kept in sync there, not imported,
/// since that module predates this one).
const LOGIN_RATE_LIMIT: (u32, Duration) = (5, Duration::from_secs(60));
const IP_RATE_LIMIT: (u32, Duration) = (20, Duration::from_secs(60));

/// Default sandbox resource caps, used when the corresponding
/// `ARENA_SANDBOX_*` env var is unset or unparseable.
const DEFAULT_SANDBOX_CPUS: f64 = 2.0;
const DEFAULT_SANDBOX_MEMORY_BYTES: i64 = 2 * 1024 * 1024 * 1024;

/// Build a [`DockerDriverConfig`] from `cfg` plus the `ARENA_EDGE_CONTAINER`,
/// `ARENA_LITELLM_CONTAINER`, `ARENA_SANDBOX_CPUS`, and
/// `ARENA_SANDBOX_MEMORY_BYTES` env vars. Shared by `cmd_serve` and by
/// `main.rs`'s `down` wiring, so both commands drive Docker identically.
pub fn docker_driver_config_from_env(cfg: &ArenaConfig) -> DockerDriverConfig {
    let cpu_limit = std::env::var("ARENA_SANDBOX_CPUS")
        .ok()
        .and_then(|s| s.parse::<f64>().ok())
        .unwrap_or(DEFAULT_SANDBOX_CPUS);
    let memory_bytes = std::env::var("ARENA_SANDBOX_MEMORY_BYTES")
        .ok()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(DEFAULT_SANDBOX_MEMORY_BYTES);

    DockerDriverConfig {
        runtime: cfg.docker_runtime.clone(),
        litellm_url: cfg.litellm_url.clone(),
        edge_container: std::env::var("ARENA_EDGE_CONTAINER").ok(),
        litellm_container: std::env::var("ARENA_LITELLM_CONTAINER").ok(),
        cpu_limit,
        memory_bytes,
        base_domain: cfg.base_domain.clone(),
        cmd_override: None,
    }
}

/// Build a [`LiteLlmMinter`] from `cfg.litellm_url` plus the required
/// `LITELLM_MASTER_KEY` env var. Shared by `cmd_serve` and by `main.rs`'s
/// `down` wiring.
pub fn litellm_minter_from_env(cfg: &ArenaConfig) -> Result<LiteLlmMinter> {
    let master_key =
        std::env::var("LITELLM_MASTER_KEY").context("LITELLM_MASTER_KEY is required")?;
    Ok(LiteLlmMinter::new(cfg.litellm_url.clone(), master_key))
}

/// `arena serve` — boot the real HTTP server: open the SQLite store, connect
/// the real Docker driver and LiteLLM minter, spawn the background reaper,
/// and serve `build_router` on `cfg.listen`.
pub async fn cmd_serve(cfg: ArenaConfig) -> Result<()> {
    let db = Db::open(&cfg.db_path).with_context(|| format!("opening db {:?}", cfg.db_path))?;

    let driver = DockerDriver::connect(docker_driver_config_from_env(&cfg))
        .context("connecting to Docker")?;
    let minter = litellm_minter_from_env(&cfg)?;

    let listen = cfg.listen.clone();
    let state = AppState {
        db: Arc::new(db),
        driver: Arc::new(driver),
        minter: Arc::new(minter),
        cfg: Arc::new(cfg),
        limiter: Arc::new(Mutex::new(RateLimiter::new(LOGIN_RATE_LIMIT.0, LOGIN_RATE_LIMIT.1))),
        ip_limiter: Arc::new(Mutex::new(RateLimiter::new(IP_RATE_LIMIT.0, IP_RATE_LIMIT.1))),
        launch_locks: Arc::new(Mutex::new(HashMap::new())),
    };

    spawn_reaper(state.clone());

    let listener = tokio::net::TcpListener::bind(&listen)
        .await
        .with_context(|| format!("binding {listen}"))?;
    axum::serve(listener, build_router(state)).await?;
    Ok(())
}

/// `arena up <manifest.toml>` — parse and register an event, then either
/// print roster credentials or the join code.
///
/// Roster mode validates the *entire* roster before creating the event or
/// any user: format (`naming::validate_username`), in-roster duplicates,
/// and collisions with a username that already exists in the database
/// (`users.username` is a global primary key across every event) are all
/// checked first, so an error anywhere in the roster leaves nothing
/// written to the database — never a half-created event with a stuck name
/// and a partial user list. Each valid, non-colliding username then gets a
/// freshly generated 12-character password (hashed with argon2 for
/// storage; the plaintext only ever appears in the returned CSV, for the
/// operator to hand out).
///
/// Returns the roster CSV (`"username,password,event\n"` per line) in
/// roster mode, or a one-line "ready" message with the join code in
/// join-code mode.
pub fn cmd_up(db: &Db, manifest_path: &Path) -> Result<String> {
    let toml_str = std::fs::read_to_string(manifest_path)
        .with_context(|| format!("reading manifest {}", manifest_path.display()))?;
    let manifest = EventManifest::from_toml(&toml_str)
        .with_context(|| format!("parsing manifest {}", manifest_path.display()))?;

    match &manifest.roster {
        Some(roster) => {
            // Validate the whole roster before any database write — an
            // error anywhere in the roster (bad format, a duplicate within
            // the roster, or a collision with an existing user) must not
            // leave the event row or any earlier username created.
            let mut seen = HashSet::new();
            for username in roster {
                validate_username(username)
                    .map_err(|e| anyhow::anyhow!("invalid username {username:?}: {e}"))?;
                if !seen.insert(username.as_str()) {
                    anyhow::bail!("duplicate username in roster: {username:?}");
                }
            }
            // `users.username` is a global primary key across every event,
            // so a name from a *previous* event's roster would otherwise
            // fail deep inside the create loop below — after insert_event
            // already committed and after earlier users in this roster
            // were already created, leaving a permanently stuck event name
            // with a partial roster. Catch it here instead.
            for username in roster {
                if let Some(existing) = db.get_user(username)? {
                    anyhow::bail!(
                        "username {username:?} already exists (from event {:?})",
                        existing.event
                    );
                }
            }

            db.insert_event(&manifest)
                .with_context(|| format!("event {:?} already exists", manifest.name))?;

            let mut csv = String::new();
            for username in roster {
                let password = generate_password();
                let hash = hash_password(&password)?;
                db.create_user(username, &manifest.name, &hash)
                    .map_err(|e| anyhow::anyhow!("creating user {username:?}: {e}"))?;
                csv.push_str(&format!("{username},{password},{}\n", manifest.name));
            }
            Ok(csv)
        }
        None => {
            db.insert_event(&manifest)
                .with_context(|| format!("event {:?} already exists", manifest.name))?;
            let code = manifest.join_code.as_deref().unwrap_or_default();
            Ok(format!(
                "event {} ready — join code: {}\n",
                manifest.name, code
            ))
        }
    }
}

/// `arena down <event>` — tear down every sandbox belonging to `event`.
///
/// Mirrors the reaper's log-and-continue semantics (`reaper::reap_once`):
/// a `driver.destroy` failure is logged but does not stop the sequence for
/// that sandbox (revoke + delete still run, so a container that failed to
/// tear down never strands a live key); a `minter.revoke` failure is logged
/// and the sandbox's row is left in place (not counted, not deleted) so a
/// future run can retry it, rather than stranding an unrevoked key with no
/// row left to find it by.
///
/// Returns the number of sandboxes that made it all the way through to
/// `delete_sandbox`.
pub async fn cmd_down(
    db: &Db,
    driver: &dyn SandboxDriver,
    minter: &dyn KeyMinter,
    event: &str,
) -> Result<u32> {
    let manifest = db
        .get_event(event)?
        .ok_or_else(|| anyhow::anyhow!("no such event: {event:?}"))?;

    let sandboxes = db.list_sandboxes_for_event(event)?;
    let mut count = 0u32;

    for row in sandboxes {
        if let Err(e) = driver.destroy(&row.username, manifest.retain_jsonl).await {
            eprintln!("arena down: destroy failed for {:?}: {e}", row.username);
            // Fall through to revoke + delete — same reasoning as the
            // reaper: a container that failed to tear down must not strand
            // a live LiteLLM key.
        }

        if let Err(e) = minter.revoke(&row.litellm_key).await {
            eprintln!("arena down: revoke failed for {:?}: {e}", row.username);
            // Leave the row in place so a future `down` can retry it,
            // rather than stranding an unrevoked key with no row left to
            // find it by.
            continue;
        }

        if let Err(e) = db.delete_sandbox(&row.username) {
            eprintln!(
                "arena down: delete_sandbox failed for {:?}: {e}",
                row.username
            );
            continue;
        }

        count += 1;
    }

    Ok(count)
}

/// `arena users <event>` — newline-joined usernames registered for `event`.
pub fn cmd_users(db: &Db, event: &str) -> Result<String> {
    let users = db.list_users(event)?;
    Ok(users
        .into_iter()
        .map(|u| u.username)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// `arena keygen` — 64 random bytes from the OS CSPRNG, hex-encoded (128
/// hex chars). Meant for `ARENA_COOKIE_KEY`.
pub fn cmd_keygen() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 64];
    rand::rngs::OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::verify_password;
    use crate::db::SandboxRow;
    use crate::driver::FakeDriver;
    use crate::keys::FakeMinter;
    use chrono::Utc;
    use std::io::Write;

    fn write_manifest(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(contents.as_bytes()).unwrap();
        f
    }

    const ROSTER_MANIFEST: &str = r#"
        name = "uva-fall"
        image = "img:1"
        roster = ["katie", "engineer-a", "bob"]
        ttl_hours = 6
        budget_usd = 5.0
    "#;

    const JOIN_CODE_MANIFEST: &str = r#"
        name = "uva-open"
        image = "img:1"
        join_code = "uva-2026"
        ttl_hours = 6
        budget_usd = 5.0
    "#;

    #[test]
    fn roster_mode_creates_one_csv_line_per_user_with_valid_hashes() {
        let db = Db::open_in_memory().unwrap();
        let f = write_manifest(ROSTER_MANIFEST);

        let csv = cmd_up(&db, f.path()).unwrap();
        let lines: Vec<&str> = csv.trim_end().lines().collect();
        assert_eq!(lines.len(), 3, "expected one CSV line per roster user: {csv:?}");

        let mut seen_usernames = Vec::new();
        for line in &lines {
            let parts: Vec<&str> = line.split(',').collect();
            assert_eq!(parts.len(), 3, "line should be username,password,event: {line:?}");
            let (username, password, event) = (parts[0], parts[1], parts[2]);
            assert_eq!(event, "uva-fall");
            assert_eq!(password.len(), 12, "password should be 12 chars: {password:?}");
            seen_usernames.push(username.to_string());

            let row = db.get_user(username).unwrap().expect("user should exist in db");
            assert_eq!(row.event, "uva-fall");
            assert!(
                verify_password(password, &row.pass_hash),
                "stored hash must verify against the plaintext password returned in the CSV"
            );
        }
        seen_usernames.sort();
        assert_eq!(seen_usernames, vec!["bob", "engineer-a", "katie"]);
    }

    #[test]
    fn invalid_roster_username_aborts_before_any_user_is_created() {
        let db = Db::open_in_memory().unwrap();
        let bad = ROSTER_MANIFEST.replace(r#""bob""#, r#""Bob Not Valid""#);
        let f = write_manifest(&bad);

        let result = cmd_up(&db, f.path());
        assert!(result.is_err(), "an invalid username in the roster must error");

        // No user from the roster — valid or not — was created.
        assert!(db.list_users("uva-fall").unwrap().is_empty());
        assert!(db.get_user("katie").unwrap().is_none());
        assert!(db.get_user("engineer-a").unwrap().is_none());
    }

    #[test]
    fn in_roster_duplicate_username_aborts_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        let dup = r#"
            name = "uva-fall"
            image = "img:1"
            roster = ["katie", "bob", "katie"]
            ttl_hours = 6
            budget_usd = 5.0
        "#;
        let f = write_manifest(dup);

        let err = cmd_up(&db, f.path()).unwrap_err().to_string();
        assert!(err.contains("katie"), "error should name the duplicate: {err:?}");

        // No event row and no users — the whole roster is rejected before
        // insert_event runs.
        assert!(db.get_event("uva-fall").unwrap().is_none());
        assert!(db.list_users("uva-fall").unwrap().is_empty());
        assert!(db.get_user("katie").unwrap().is_none());
        assert!(db.get_user("bob").unwrap().is_none());
    }

    #[test]
    fn roster_username_colliding_with_a_user_from_another_event_aborts_before_any_write() {
        let db = Db::open_in_memory().unwrap();
        // A prior event already owns "katie" — users.username is a global
        // primary key, so a second event's roster can't reuse it.
        db.insert_event(&event("spring", "")).unwrap();
        db.create_user("katie", "spring", "existing-hash").unwrap();

        let clashing = r#"
            name = "uva-fall"
            image = "img:1"
            roster = ["ann", "katie"]
            ttl_hours = 6
            budget_usd = 5.0
        "#;
        let f = write_manifest(clashing);

        let err = cmd_up(&db, f.path()).unwrap_err().to_string();
        assert!(err.contains("katie"), "error should name the collision: {err:?}");
        assert!(err.contains("spring"), "error should name the owning event: {err:?}");

        // The new event was never created, "ann" was never created, and
        // the pre-existing "katie" (from "spring") is untouched.
        assert!(db.get_event("uva-fall").unwrap().is_none());
        assert!(db.get_user("ann").unwrap().is_none());
        let katie = db.get_user("katie").unwrap().expect("pre-existing user must survive");
        assert_eq!(katie.event, "spring");
        assert_eq!(katie.pass_hash, "existing-hash");
    }

    #[test]
    fn join_code_mode_prints_the_code_line() {
        let db = Db::open_in_memory().unwrap();
        let f = write_manifest(JOIN_CODE_MANIFEST);

        let out = cmd_up(&db, f.path()).unwrap();
        assert!(out.contains("uva-open"), "{out:?}");
        assert!(out.contains("uva-2026"), "{out:?}");

        assert_eq!(
            db.event_by_join_code("uva-2026").unwrap().as_deref(),
            Some("uva-open")
        );
    }

    #[test]
    fn duplicate_up_of_the_same_event_errors_cleanly() {
        let db = Db::open_in_memory().unwrap();
        let f = write_manifest(JOIN_CODE_MANIFEST);

        cmd_up(&db, f.path()).unwrap();
        let second = cmd_up(&db, f.path());
        assert!(second.is_err(), "a second `up` of the same event must error");
    }

    fn event(name: &str, retain_jsonl_line: &str) -> EventManifest {
        EventManifest::from_toml(&format!(
            "name = \"{name}\"\nimage = \"img:1\"\njoin_code = \"{name}-code\"\nttl_hours = 6\nbudget_usd = 5.0\n{retain_jsonl_line}"
        ))
        .unwrap()
    }

    #[tokio::test]
    async fn cmd_down_destroys_every_sandbox_and_respects_retain_jsonl() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("keep", "")).unwrap(); // retain_jsonl defaults true
        db.create_user("katie", "keep", "h").unwrap();
        db.create_user("ann", "keep", "h").unwrap();

        let now = Utc::now();
        db.upsert_sandbox(&SandboxRow {
            username: "katie".into(),
            container_id: "c1".into(),
            litellm_key: "key-katie".into(),
            expires_at: now,
        })
        .unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "ann".into(),
            container_id: "c2".into(),
            litellm_key: "key-ann".into(),
            expires_at: now,
        })
        .unwrap();

        let driver = FakeDriver::new();
        let minter = FakeMinter::new();

        let count = cmd_down(&db, driver.as_ref(), minter.as_ref(), "keep").await.unwrap();
        assert_eq!(count, 2);

        let destroyed = driver.destroyed.lock().unwrap().clone();
        assert!(destroyed.contains(&("katie".to_string(), true)), "{destroyed:?}");
        assert!(destroyed.contains(&("ann".to_string(), true)), "{destroyed:?}");

        assert!(db.get_sandbox("katie").unwrap().is_none());
        assert!(db.get_sandbox("ann").unwrap().is_none());
    }

    #[tokio::test]
    async fn cmd_down_passes_keep_volume_false_when_retain_jsonl_is_false() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("drop", "retain_jsonl = false")).unwrap();
        db.create_user("bob", "drop", "h").unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "bob".into(),
            container_id: "c1".into(),
            litellm_key: "key-bob".into(),
            expires_at: Utc::now(),
        })
        .unwrap();

        let driver = FakeDriver::new();
        let minter = FakeMinter::new();
        cmd_down(&db, driver.as_ref(), minter.as_ref(), "drop").await.unwrap();

        assert_eq!(
            driver.destroyed.lock().unwrap().clone(),
            vec![("bob".to_string(), false)]
        );
    }

    #[tokio::test]
    async fn cmd_down_keeps_the_row_when_revoke_fails_and_does_not_count_it() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("keep", "")).unwrap();
        db.create_user("katie", "keep", "h").unwrap();
        db.create_user("ann", "keep", "h").unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "katie".into(),
            container_id: "c1".into(),
            litellm_key: "key-katie".into(),
            expires_at: Utc::now(),
        })
        .unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "ann".into(),
            container_id: "c2".into(),
            litellm_key: "key-ann".into(),
            expires_at: Utc::now(),
        })
        .unwrap();

        let driver = FakeDriver::new();
        let minter = FakeMinter::new();
        minter.fail_revoke.store(true, std::sync::atomic::Ordering::SeqCst);

        let count = cmd_down(&db, driver.as_ref(), minter.as_ref(), "keep").await.unwrap();
        assert_eq!(count, 0, "no sandbox fully processed when every revoke fails");
        // destroy still ran for both, since destroy-failure/success doesn't
        // gate revoke — but here it's revoke that fails.
        assert_eq!(driver.destroyed.lock().unwrap().len(), 2);
        assert!(db.get_sandbox("katie").unwrap().is_some(), "row must survive a failed revoke");
        assert!(db.get_sandbox("ann").unwrap().is_some(), "row must survive a failed revoke");
    }

    #[tokio::test]
    async fn cmd_down_counts_a_sandbox_even_when_destroy_fails() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("keep", "")).unwrap();
        db.create_user("katie", "keep", "h").unwrap();
        db.upsert_sandbox(&SandboxRow {
            username: "katie".into(),
            container_id: "c1".into(),
            litellm_key: "key-katie".into(),
            expires_at: Utc::now(),
        })
        .unwrap();

        let driver = FakeDriver::new();
        driver.fail_destroy.store(true, std::sync::atomic::Ordering::SeqCst);
        let minter = FakeMinter::new();

        let count = cmd_down(&db, driver.as_ref(), minter.as_ref(), "keep").await.unwrap();
        assert_eq!(
            count, 1,
            "a destroy failure must not block revoke + delete, and the row is still counted"
        );
        assert!(
            driver.destroyed.lock().unwrap().is_empty(),
            "the failed destroy call itself isn't recorded, but that must not stop the rest"
        );
        assert!(minter.revoked.lock().unwrap().contains(&"key-katie".to_string()));
        assert!(db.get_sandbox("katie").unwrap().is_none(), "row must still be deleted");
    }

    #[tokio::test]
    async fn cmd_down_unknown_event_errors() {
        let db = Db::open_in_memory().unwrap();
        let driver = FakeDriver::new();
        let minter = FakeMinter::new();
        assert!(cmd_down(&db, driver.as_ref(), minter.as_ref(), "nope").await.is_err());
    }

    #[test]
    fn cmd_users_lists_newline_joined_usernames() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("e", "")).unwrap();
        db.create_user("bob", "e", "h").unwrap();
        db.create_user("ann", "e", "h").unwrap();

        let out = cmd_users(&db, "e").unwrap();
        assert_eq!(out, "ann\nbob");
    }

    #[test]
    fn cmd_users_on_an_empty_event_is_an_empty_string() {
        let db = Db::open_in_memory().unwrap();
        db.insert_event(&event("e", "")).unwrap();
        assert_eq!(cmd_users(&db, "e").unwrap(), "");
    }

    #[test]
    fn keygen_outputs_128_hex_chars() {
        let key = cmd_keygen();
        assert_eq!(key.len(), 128, "{key:?}");
        assert!(key.chars().all(|c| c.is_ascii_hexdigit()), "{key:?}");

        let other = cmd_keygen();
        assert_ne!(key, other, "keygen must not repeat");
    }
}
