//! `RequirePublicSession` — axum extractor that gates per-session reads
//! by `share_policy`.
//!
//! Phase 4.2. When a session is marked `private` in `share_policy`, the
//! extractor returns 404 so the handler body never runs. Adding it as a
//! parameter to a per-session handler is the only change required to opt
//! that handler into Invariant ①.
//!
//! Used by ~18 handlers under `/api/sessions/{session_id}/...`. The gate
//! reads the session id from the URL path parameters (so it composes
//! cleanly with `Path<String>` extractors in the same handler — `Path`
//! is a `FromRequestParts` implementation and can appear multiple times
//! per handler).
//!
//! The extractor consults `share_policy` via the same code path as
//! `private_session_ids` in `api.rs`. When Phase 4.3/4.4 hardens that
//! lookup to fail closed on store errors, the extractor inherits the
//! new behavior with no change here.

use std::collections::HashMap;

use axum::extract::{FromRequestParts, Path};
use axum::http::StatusCode;
use axum::http::request::Parts;
use open_story_store::event_store::SharePolicyMode;

use crate::logging::{log_event, short_id};
use crate::state::SharedState;

/// Gate: return 404 if the session referenced by `{session_id}` in the
/// URL path is marked `private`. Add as a handler parameter to opt that
/// handler into Invariant ① enforcement.
pub struct RequirePublicSession;

impl FromRequestParts<SharedState> for RequirePublicSession {
    type Rejection = StatusCode;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &SharedState,
    ) -> Result<Self, Self::Rejection> {
        // Read the path parameters without consuming them — Path is a
        // FromRequestParts extractor and can be invoked multiple times
        // per request, so the handler can still have its own
        // `Path<String>` for `session_id`.
        let Path(params): Path<HashMap<String, String>> =
            Path::from_request_parts(parts, state)
                .await
                .map_err(|_| StatusCode::BAD_REQUEST)?;
        let session_id = params
            .get("session_id")
            .ok_or(StatusCode::BAD_REQUEST)?
            .as_str();

        let s = state.read().await;
        // Phase 4.4: fail closed. A transient store error on the privacy
        // path must NOT silently widen exposure — return 503 so the
        // caller sees degraded availability instead of getting private
        // session content as if it were shared. Per the approved plan
        // we hard-fail (no LKG cache); the alternative was rejected for
        // the sovereignty-significant case where stale policies could
        // leak hours of data on a long store outage.
        let mode = s
            .store
            .event_store
            .get_share_policy(session_id)
            .await
            .map_err(|e| {
                log_event(
                    "api",
                    &format!(
                        "GATE  /api/sessions/{}/* → 503 (share_policy read error: {e})",
                        short_id(session_id)
                    ),
                );
                StatusCode::SERVICE_UNAVAILABLE
            })?;
        if matches!(mode, SharePolicyMode::Private) {
            log_event(
                "api",
                &format!(
                    "GATE  /api/sessions/{}/* → 404 (private)",
                    short_id(session_id)
                ),
            );
            return Err(StatusCode::NOT_FOUND);
        }
        Ok(Self)
    }
}
