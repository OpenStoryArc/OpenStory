//! presence — the node's heartbeat on the bus (P-01).
//!
//! Every `interval` the node publishes its health body (the same one
//! `/api/health` serves) as a CloudEvent on `presence.{host}.{principal}`.
//! Presence is an observed family of its own: never `events.*`, so it can
//! never be mistaken for agent history, and never `ui.*`, because nobody
//! authored it. The subject names the node, so the hub, the fleet tab, and
//! another node can read who is alive without asking anyone.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use anyhow::Context as _;
use open_story_bus::IngestBatch;
use open_story_core::cloud_event::CloudEvent;
use open_story_core::event_data::EventData;
use open_story_store::event_store::PresenceRow;
use serde_json::Value;
use tokio::task::JoinHandle;

use crate::config::Person;
use crate::principal_resolver::{resolve, SourceContext};
use crate::state::SharedState;

/// The agent value on every presence event: the node itself.
pub const AGENT: &str = "openstory";
/// The subtype the persist consumer routes on (P-02).
pub const SUBTYPE: &str = "node.presence";
/// The CloudEvent source.
pub const SOURCE: &str = "openstory-node";
/// A publish that has not answered by then is cut off and counted as a
/// failure (P-06), so a stuck bus never wedges the beat.
pub const PUBLISH_TIMEOUT: Duration = Duration::from_secs(10);

static BEATS: AtomicU64 = AtomicU64::new(0);
static FAILURES: AtomicU64 = AtomicU64::new(0);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

/// What the beat has done since boot (P-06): on the health body, so a
/// node whose presence is not leaving the machine says so itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct PresenceStats {
    pub beats: u64,
    pub failures: u64,
    pub last_error: Option<String>,
}

pub fn stats() -> PresenceStats {
    PresenceStats {
        beats: BEATS.load(Ordering::Relaxed),
        failures: FAILURES.load(Ordering::Relaxed),
        last_error: LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()).clone(),
    }
}

/// The health body's `presence` block.
pub fn stats_json(interval_secs: u64) -> Value {
    let s = stats();
    serde_json::json!({
        "interval_secs": interval_secs,
        "beats": s.beats,
        "failures": s.failures,
        "last_error": s.last_error,
    })
}

fn record_failure(subject: &str, session_id: &str, err: &anyhow::Error) {
    FAILURES.fetch_add(1, Ordering::Relaxed);
    *LAST_ERROR.lock().unwrap_or_else(|e| e.into_inner()) = Some(format!("{err:#}"));
    crate::logging::publish_failed("presence", subject, session_id, 1, err);
}

/// One NATS subject token. Dots would split it, wildcards would widen it,
/// whitespace is not allowed; an empty token reads `unknown`.
fn token(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c == '.' || c == '*' || c == '>' || c.is_whitespace() {
                '-'
            } else {
                c
            }
        })
        .collect();
    if cleaned.is_empty() {
        "unknown".to_string()
    } else {
        cleaned
    }
}

/// `presence.{host}.{principal}`, with both tokens cleaned.
pub fn subject(host: &str, principal: &str) -> String {
    format!("presence.{}.{}", token(host), token(principal))
}

/// The session id presence rides under: one per host, never an agent session.
fn session_id(host: &str) -> String {
    format!("presence:{}", token(host))
}

/// (person_id, principal_id) for this node: the first configured principal
/// whose matchers name this host and user. The node is a device, not an
/// agent platform, so no agent is offered to the matchers. With no person
/// configured yet, the node is `local`.
pub fn identity(person: Option<&Person>, host: &str) -> (Option<String>, String) {
    match person {
        Some(p) => {
            let resolved = resolve(
                p,
                &SourceContext {
                    agent: None,
                    host: Some(host.to_string()),
                    user: Some(open_story_core::user::user().to_string()),
                    source_path: None,
                },
            );
            (Some(resolved.person_id), resolved.principal_id)
        }
        None => (None, "local".to_string()),
    }
}

/// The presence CloudEvent: the health body with the node's identity written
/// into it, stamped the same way agent events are.
pub fn presence_event(
    host: &str,
    person_id: Option<&str>,
    principal_id: &str,
    mut body: Value,
) -> CloudEvent {
    if let Some(obj) = body.as_object_mut() {
        obj.insert("host".to_string(), Value::String(host.to_string()));
        obj.insert(
            "principal_id".to_string(),
            Value::String(principal_id.to_string()),
        );
        if let Some(p) = person_id {
            obj.insert("person_id".to_string(), Value::String(p.to_string()));
        }
    }
    let data = EventData::new(body, 0, session_id(host));
    let ce = CloudEvent::new(
        SOURCE.to_string(),
        "io.arc.event".to_string(),
        data,
        Some(SUBTYPE.to_string()),
        None,
        None,
        None,
        None,
        Some(AGENT.to_string()),
    )
    .with_host(host)
    .with_user(open_story_core::user::user())
    .with_principal_id(principal_id);
    match person_id {
        Some(p) => ce.with_person_id(p),
        None => ce,
    }
}

/// A node is stale once it has missed this many beats.
pub const STALE_AFTER_BEATS: i64 = 3;

/// The fleet as the latest beats tell it (P-03). Pure: rows in, one JSON
/// object per node out, with `age_secs` against `now` and `stale` once the
/// beat is older than three intervals. A beat whose time cannot be read is
/// stale with a null age. The sha, level, and status are lifted to the top
/// so a fleet tab needs no second path; the whole beat rides under `body`.
pub fn fleet_view(
    rows: &[PresenceRow],
    now: chrono::DateTime<chrono::Utc>,
    interval_secs: u64,
) -> Vec<Value> {
    let stale_after = (interval_secs as i64).max(1) * STALE_AFTER_BEATS;
    rows.iter()
        .map(|r| {
            let age_secs = chrono::DateTime::parse_from_rfc3339(&r.time)
                .ok()
                .map(|t| (now - t.with_timezone(&chrono::Utc)).num_seconds());
            let stale = age_secs.is_none_or(|a| a > stale_after);
            serde_json::json!({
                "host": r.host,
                "principal_id": r.principal_id,
                "person_id": r.person_id,
                "time": r.time,
                "age_secs": age_secs,
                "stale": stale,
                "status": r.body.get("status").cloned().unwrap_or(Value::Null),
                "git_sha": r.body.get("git_sha").cloned().unwrap_or(Value::Null),
                "version": r.body.get("version").cloned().unwrap_or(Value::Null),
                "body": r.body,
            })
        })
        .collect()
}

/// One presence event in the bus envelope.
pub fn batch(host: &str, ce: CloudEvent) -> IngestBatch {
    IngestBatch {
        session_id: session_id(host),
        project_id: SOURCE.to_string(),
        events: vec![ce],
    }
}

/// One beat: build the health body, stamp identity, publish. Returns the
/// subject it went to. A failure is logged in the E-05 shape, counted, and
/// returned; a publish still pending after `PUBLISH_TIMEOUT` is a failure.
pub async fn publish_once(state: &SharedState) -> anyhow::Result<String> {
    publish_once_with_timeout(state, PUBLISH_TIMEOUT).await
}

pub async fn publish_once_with_timeout(
    state: &SharedState,
    timeout: Duration,
) -> anyhow::Result<String> {
    let (_status, body) = crate::api::health_body(state).await;
    let host = open_story_core::host::host();
    let (bus, person) = {
        let s = state.read().await;
        (s.bus.clone(), s.config.person.clone())
    };
    let (person_id, principal_id) = identity(person.as_ref(), host);
    let subject = subject(host, &principal_id);
    let b = batch(
        host,
        presence_event(host, person_id.as_deref(), &principal_id, body),
    );
    let published = match tokio::time::timeout(timeout, bus.publish(&subject, &b)).await {
        Ok(r) => r,
        Err(_) => Err(anyhow::anyhow!(
            "publish timed out after {} ms",
            timeout.as_millis()
        ))
        .with_context(|| format!("failed to publish to {subject}")),
    };
    if let Err(e) = published {
        record_failure(&subject, &b.session_id, &e);
        return Err(e);
    }
    BEATS.fetch_add(1, Ordering::Relaxed);
    Ok(subject)
}

/// The beat: a first presence right away, then one every `interval`.
pub fn spawn(state: SharedState, interval: Duration) -> JoinHandle<()> {
    spawn_with_timeout(state, interval, PUBLISH_TIMEOUT)
}

/// The beat with an explicit publish timeout (tests use a short one).
pub fn spawn_with_timeout(
    state: SharedState,
    interval: Duration,
    timeout: Duration,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(interval);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            // audit-ok: publish_once logs and counts its own failure; the next tick retries
            let _ = publish_once_with_timeout(&state, timeout).await;
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    mod when_no_person_is_configured {
        use super::*;

        #[test]
        fn it_is_local() {
            assert_eq!(identity(None, "h"), (None, "local".to_string()));
        }
    }

    mod when_the_event_is_built {
        use super::*;

        #[test]
        fn it_writes_identity_into_the_body_and_the_envelope() {
            let ce = presence_event(
                "h.local",
                Some("p"),
                "dev",
                serde_json::json!({"status": "ok"}),
            );
            assert_eq!(ce.data.raw["host"], "h.local");
            assert_eq!(ce.data.raw["person_id"], "p");
            assert_eq!(ce.data.raw["principal_id"], "dev");
            assert_eq!(ce.data.raw["status"], "ok");
            assert_eq!(ce.agent.as_deref(), Some(AGENT));
            assert_eq!(ce.subtype.as_deref(), Some(SUBTYPE));
            assert_eq!(ce.person_id.as_deref(), Some("p"));
            assert_eq!(ce.principal_id.as_deref(), Some("dev"));
            assert_eq!(batch("h.local", ce).session_id, "presence:h-local");
        }
    }
}
