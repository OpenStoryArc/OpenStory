//! Bridge helpers between in-memory store state and EventStore types.

use open_story_store::event_store::SessionRow;
use open_story_store::projection::SessionProjection;
use open_story_store::state::StoreState;

/// Build a SessionRow from a SessionProjection + StoreState metadata.
pub fn session_row_from_projection(
    session_id: &str,
    proj: &SessionProjection,
    store: &StoreState,
) -> SessionRow {
    let rows = proj.timeline_rows();
    let first_event = rows.first().map(|r| r.timestamp.clone());
    let last_event = rows.last().map(|r| r.timestamp.clone());

    SessionRow {
        id: session_id.to_string(),
        project_id: store.session_projects.get(session_id).map(|r| r.value().clone()),
        project_name: store.session_project_names.get(session_id).map(|r| r.value().clone()),
        label: proj.label().map(|s| s.to_string()),
        custom_label: None, // never set from projection — only via user PUT
        branch: proj.branch().map(|s| s.to_string()),
        event_count: proj.event_count() as u64,
        first_event,
        last_event,
        host: None,
    }
}
