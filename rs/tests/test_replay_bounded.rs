//! B-09 (b): the read model is bounded during replay
//! (docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md).
//!
//! Replay walks every stored session through the projection cache. Today
//! each walk counts as an access, so every session lands inside the
//! working-set window and nothing is evictable until replay ends — the
//! whole read model resident, whatever the budget. Replay must not count
//! as access: "in the working set" is decided by the session's own last
//! event against `working_set_days`. Sessions inside the window stay
//! pinned as before; the rest are evictable the moment the budget says so;
//! every session still rebuilds on first access.
//!
//! Run with: cargo test -p open-story --test test_replay_bounded

mod helpers;

use std::sync::Arc;

use axum::body::Body;
use axum::http::Request;
use helpers::{body_json, send_request};
use open_story::server::{create_state, replay_boot_sessions, Config, ReplayContext, SharedState};
use open_story_bus::noop_bus::NoopBus;
use open_story_store::event_store::{EventStore, SessionRow};
use open_story_store::rebuild::rebuild_session;
use open_story_store::sqlite_store::SqliteStore;
use serde_json::{json, Value};

const EVENTS_PER_SESSION: u64 = 12;
const RECENT: usize = 10;
const TOTAL: usize = 100;
const BUDGET_SESSIONS: u64 = 20;

fn recent_id(i: usize) -> String {
    format!("recent-{i:03}")
}
fn old_id(i: usize) -> String {
    format!("old-{i:03}")
}

fn event(sid: &str, seq: u64, time: &str) -> Value {
    // Every event carries a ~2 KB tool output so a projection has real size.
    let text =
        format!("{seq:04} ") + &"line of tool output that fills the record body\n".repeat(40);
    json!({
        "specversion": "1.0",
        "id": format!("{sid}-evt-{seq}"),
        "source": format!("arc://transcript/{sid}"),
        "type": "io.arc.event",
        "subtype": "message.user.tool_result",
        "time": time,
        "datacontenttype": "application/json",
        "agent": "claude-code",
        "data": {
            "seq": seq,
            "session_id": sid,
            "raw": {"type": "user", "cwd": "/work/p", "sessionId": sid,
                    "uuid": format!("{sid}-u-{seq}"),
                    "message": {"role": "user", "content": [{"type": "tool_result",
                        "tool_use_id": format!("toolu_{seq}"), "content": text, "is_error": false}]},
                    "toolUseResult": {"stdout": text, "stderr": ""}},
            "agent_payload": {"_variant": "claude-code", "meta": {"agent": "claude-code"},
                "cwd": "/work/p", "text": text, "uuid": format!("{sid}-u-{seq}"),
                "timestamp": time, "user_type": "external"}
        }
    })
}

fn row(id: &str, last_event: &str) -> SessionRow {
    SessionRow {
        id: id.to_string(),
        project_id: None,
        project_name: None,
        label: None,
        custom_label: None,
        branch: None,
        event_count: EVENTS_PER_SESSION,
        first_event: Some(last_event.to_string()),
        last_event: Some(last_event.to_string()),
        host: None,
        user: None,
        origin_agent: None,
        person_id: None,
        principal_id: None,
    }
}

/// 100 sessions in SQLite: 10 with events (and a row) dated now, 90 dated
/// thirty days ago. Returns the heap bytes of one such projection.
async fn seed(data_dir: &std::path::Path) -> u64 {
    let db = SqliteStore::new(data_dir).unwrap();
    let now = chrono::Utc::now();
    let old = now - chrono::Duration::days(30);
    let stamp = |t: chrono::DateTime<chrono::Utc>, seq: u64| {
        (t + chrono::Duration::seconds(seq as i64))
            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
    };
    for i in 0..TOTAL {
        let (sid, base) = if i < RECENT {
            (recent_id(i), now)
        } else {
            (old_id(i - RECENT), old)
        };
        let events: Vec<Value> = (1..=EVENTS_PER_SESSION)
            .map(|seq| event(&sid, seq, &stamp(base, seq)))
            .collect();
        db.insert_batch(&sid, &events).await.unwrap();
        db.upsert_session(&row(&sid, &stamp(base, EVENTS_PER_SESSION)))
            .await
            .unwrap();
    }
    let one = rebuild_session(&db, &old_id(0))
        .await
        .expect("a seeded session rebuilds");
    assert_eq!(one.event_count(), EVENTS_PER_SESSION as usize);
    one.heap_bytes()
}

async fn boot(tmp: &tempfile::TempDir) -> (SharedState, u64) {
    let data_dir = tmp.path().join("data");
    let watch_dir = tmp.path().join("watch");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(&watch_dir).unwrap();
    let per_session = seed(&data_dir).await;
    let config = Config {
        projection_cache_bytes: per_session * BUDGET_SESSIONS,
        working_set_days: 7,
        ..Config::default()
    };
    let state = create_state(&data_dir, &watch_dir, Arc::new(NoopBus), config)
        .await
        .unwrap();
    (state, per_session)
}

async fn replay(state: &SharedState) {
    let ctx = {
        let s = state.read().await;
        ReplayContext {
            event_store: s.store.event_store.clone(),
            projections: s.store.projections.clone(),
            subagent_parents: s.store.subagent_parents.clone(),
            session_children: s.store.session_children.clone(),
            full_payloads: s.store.full_payloads.clone(),
            session_projects: s.store.session_projects.clone(),
            session_project_names: s.store.session_project_names.clone(),
        }
    };
    replay_boot_sessions(&ctx).await;
}

mod when_replay_walks_a_store_larger_than_the_budget {
    use super::*;

    #[tokio::test]
    async fn it_keeps_the_working_set_and_at_most_the_budget_of_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, per_session) = boot(&tmp).await;
        replay(&state).await;

        let s = state.read().await;
        let cache = &s.store.projections;
        for i in 0..RECENT {
            assert!(
                cache.contains(&recent_id(i)),
                "{} is inside the working set and stays resident",
                recent_id(i)
            );
        }
        let resident = cache.resident_sessions();
        // The 10 pinned plus at most the budget's worth of others (one
        // extra allowed: the session being appended when the budget trips).
        let ceiling = RECENT + BUDGET_SESSIONS as usize + 1;
        assert!(
            resident <= ceiling,
            "after replay {resident} projections are resident; at most {ceiling} \
             (10 in the window + a {BUDGET_SESSIONS}-session budget) may be; \
             evictions {} resident_bytes {} max_bytes {} per_session {per_session}; \
             old-000 last_event_at {:?} working_set_days {}",
            cache.evictions(),
            cache.resident_bytes(),
            cache.max_bytes(),
            cache.get(&old_id(0)).and_then(|p| p.last_event_at()),
            s.config.working_set_days
        );
        assert!(resident >= RECENT, "{resident} resident");
        assert!(
            cache.resident_bytes() <= per_session * (BUDGET_SESSIONS + RECENT as u64 + 1),
            "resident bytes {} exceed the budget plus the pinned working set",
            cache.resident_bytes()
        );
        assert!(
            cache.evictions() > 0,
            "replay evicted out-of-window sessions as it went"
        );
    }

    #[tokio::test]
    async fn it_says_so_on_health_and_still_rebuilds_any_session_on_access() {
        let tmp = tempfile::tempdir().unwrap();
        let (state, _) = boot(&tmp).await;
        replay(&state).await;

        let resident_before = state.read().await.store.projections.resident_sessions();
        let req = Request::get("/api/health").body(Body::empty()).unwrap();
        let body = body_json(send_request(state.clone(), req).await).await;
        assert_eq!(
            body["projections"]["count"], resident_before as u64,
            "{body}"
        );
        assert_eq!(body["projections"]["sessions"], TOTAL as u64);
        assert!(
            resident_before < TOTAL,
            "the read model is bounded: {resident_before} of {TOTAL} resident"
        );

        // An evicted old session rebuilds on first access with its full history.
        let s = state.read().await;
        let evicted = (0..TOTAL - RECENT)
            .map(old_id)
            .find(|id| !s.store.projections.contains(id))
            .expect("some old session was evicted during replay");
        let p = s
            .store
            .get_or_rebuild(&evicted)
            .await
            .expect("rebuilds from SQLite");
        assert_eq!(
            p.event_count(),
            EVENTS_PER_SESSION as usize,
            "{evicted} rebuilt whole"
        );
        drop(p);
        assert!(s.store.projections.contains(&evicted), "now resident");
    }
}

/// B-09 (c): a budget larger than the box is no bound. With a cgroup
/// limit known, the projection budget the node runs with is the smaller
/// of the configured value and a fixed share of the limit, so the read
/// model can never fill the container by itself.
mod when_the_configured_budget_exceeds_the_box {
    use open_story::server::effective_projection_budget;

    #[test]
    fn it_is_clamped_to_a_share_of_the_cgroup_limit() {
        let gb = 1_000_000_000u64;
        // 4 GB configured inside a 2 GiB cgroup: 40 % of the limit.
        assert_eq!(
            effective_projection_budget(4 * gb, Some(2_147_483_648)),
            2_147_483_648 * 2 / 5
        );
        // A configured budget below the share is kept as is.
        assert_eq!(
            effective_projection_budget(500_000_000, Some(2_147_483_648)),
            500_000_000
        );
        // No limit known: the configured value stands.
        assert_eq!(effective_projection_budget(4 * gb, None), 4 * gb);
    }
}
