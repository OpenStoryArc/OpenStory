//! Test fixture for spinning up a temp-dir SqliteStore.
//!
//! Tests use this to construct a `Server { subscriber, store }` for
//! integration tests that exercise the query tools. The returned
//! `TempDir` must be held alive for the duration of the test —
//! dropping it removes the data dir and closes the underlying SQLite
//! file.

use open_story_store::event_store::EventStore;
use open_story_store::plan_store::PlanStore;
use open_story_store::sqlite_store::SqliteStore;
use std::sync::Arc;
use tempfile::TempDir;

/// Open a fresh SqliteStore + PlanStore in a temp directory. The TempDir
/// is returned so the caller can keep it alive for the duration of
/// the test (drop = cleanup).
pub fn make_test_store() -> (Arc<dyn EventStore>, Arc<PlanStore>, TempDir) {
    let dir = tempfile::tempdir().expect("create temp dir");
    let store: Arc<dyn EventStore> =
        Arc::new(SqliteStore::new(dir.path()).expect("open SqliteStore"));
    let plans_dir = dir.path().join("plans");
    std::fs::create_dir_all(&plans_dir).expect("create plans dir");
    let plan_store = Arc::new(PlanStore::new(&plans_dir).expect("open PlanStore"));
    (store, plan_store, dir)
}
