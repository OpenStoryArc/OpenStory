//! Test fixture for spinning up a temp-dir SqliteStore.
//!
//! Tests use this to construct a `Server { subscriber, store }` for
//! integration tests that exercise the query tools. The returned
//! `TempDir` must be held alive for the duration of the test —
//! dropping it removes the data dir and closes the underlying SQLite
//! file.

use open_story_store::event_store::EventStore;
use open_story_store::sqlite_store::SqliteStore;
use std::sync::Arc;
use tempfile::TempDir;

/// Open a fresh SqliteStore in a temp directory. The TempDir is
/// returned so the caller can keep it alive for the duration of
/// the test (drop = cleanup).
pub fn make_test_store() -> (Arc<dyn EventStore>, TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let store: Arc<dyn EventStore> =
        Arc::new(SqliteStore::new(dir.path()).expect("open SqliteStore in temp dir"));
    (store, dir)
}
