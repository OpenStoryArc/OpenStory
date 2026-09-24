//! presence consumer (P-02): a node's beats land in the `presence` table.
//!
//! Presence is an observed family of its own. It is never a session, never
//! an event row, never an FTS document, never a JSONL line. The persist
//! consumer routes a presence batch here before any of that machinery, and
//! the `presence.>` subscriber at boot feeds this directly.

use open_story_core::cloud_event::CloudEvent;
use open_story_store::event_store::{EventStore, PresenceRow};
use open_story_store::persistence::PresenceLog;
use serde_json::{json, Value};

/// Is this event a node's presence beat?
pub fn is_presence(ce: &CloudEvent) -> bool {
    ce.subtype.as_deref() == Some(crate::presence::SUBTYPE)
}

/// The row a beat becomes. Identity comes from the envelope first (the
/// stamped extension attributes) and from the body second.
pub fn row_from(ce: &CloudEvent) -> Option<PresenceRow> {
    if !is_presence(ce) {
        return None;
    }
    let raw = &ce.data.raw;
    let from_body = |key: &str| raw.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let host = ce.host.clone().or_else(|| from_body("host"))?;
    let principal_id = ce
        .principal_id
        .clone()
        .or_else(|| from_body("principal_id"))
        .unwrap_or_else(|| "local".to_string());
    let person_id = ce.person_id.clone().or_else(|| from_body("person_id"));
    Some(PresenceRow {
        host,
        principal_id,
        person_id,
        time: ce.time.clone(),
        body: raw.clone(),
    })
}

/// The compact history line for a beat (D-02): who, when, which build,
/// and the verdict. Pure.
pub fn log_line(row: &PresenceRow) -> Value {
    let b = &row.body;
    let findings: Vec<Value> = b["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| f.get("id").cloned())
        .collect();
    json!({
        "time": row.time,
        "host": row.host,
        "principal_id": row.principal_id,
        "git_sha": b.get("git_sha").cloned().unwrap_or(Value::Null),
        "built_at": b.get("built_at").cloned().unwrap_or(Value::Null),
        "level": b["verdict"].get("level").cloned().unwrap_or(Value::Null),
        "findings": findings,
    })
}

/// Store every presence beat in the batch, and append its history line
/// when a log is given. Returns how many landed; a failed upsert or append
/// is logged and counted.
pub async fn store_presence(
    store: &dyn EventStore,
    log: Option<&PresenceLog>,
    events: &[CloudEvent],
) -> usize {
    let mut stored = 0;
    for ce in events {
        let Some(row) = row_from(ce) else {
            continue;
        };
        match store.upsert_presence(&row).await {
            Ok(()) => stored += 1,
            Err(e) => crate::logging::failed("presence_upsert", &format!("{e:#}")),
        }
        if let Some(log) = log {
            if let Err(e) = log.append(&log_line(&row)) {
                crate::logging::failed("presence_log_append", &format!("{e:#}"));
            }
        }
    }
    stored
}

#[cfg(test)]
mod tests {
    use super::*;

    mod when_the_event_is_not_presence {
        use super::*;

        #[test]
        fn it_yields_no_row() {
            let ce = crate::ui_events::ui_cloud_event(
                "interaction",
                "click",
                "s",
                serde_json::json!({}),
            );
            assert!(!is_presence(&ce));
            assert!(row_from(&ce).is_none());
        }
    }

    mod when_the_envelope_is_stamped {
        use super::*;

        #[test]
        fn it_prefers_the_envelope_identity() {
            let ce =
                crate::presence::presence_event("h", Some("p"), "dev", serde_json::json!({"x": 1}));
            let row = row_from(&ce).unwrap();
            assert_eq!(row.host, "h");
            assert_eq!(row.principal_id, "dev");
            assert_eq!(row.person_id.as_deref(), Some("p"));
            assert_eq!(row.time, ce.time);
            assert_eq!(row.body["x"], 1);
        }
    }
}
