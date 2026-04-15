//! JSON Schema artifacts for open-story's serialization boundaries.
//!
//! This crate is the schema *registry* — a file-backed, generated, grep-able
//! set of JSON Schemas derived from the Rust types that already encode our
//! contracts. No runtime service, no external registry server; schemas live
//! as committed files under `/schemas/` at the repo root.
//!
//! See `docs/research/architecture-audit/SCHEMA_MAP.md` for the inventory
//! and `docs/research/architecture-audit/SCHEMA_TESTS.md` for the test plan.
//!
//! Regenerate via:
//!   cargo run -p open-story-schemas --bin generate

use anyhow::{Context, Result};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Where schema files live, relative to the repo root.
pub const SCHEMA_DIR_RELATIVE: &str = "schemas";

/// Resolve `/schemas/` absolute path from the crate manifest.
pub fn schema_dir() -> PathBuf {
    // CARGO_MANIFEST_DIR = rs/schemas/
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("rs/schemas has a parent")
        .parent()
        .expect("rs has a parent")
        .join(SCHEMA_DIR_RELATIVE)
}

/// Load a committed schema file by basename (e.g. `"cloud_event.schema.json"`).
pub fn load_schema(basename: &str) -> Result<Value> {
    let path = schema_dir().join(basename);
    load_json(&path).with_context(|| format!("loading schema {basename}"))
}

/// Load any JSON file.
pub fn load_json(path: &Path) -> Result<Value> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("reading {}", path.display()))?;
    let value = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing {} as JSON", path.display()))?;
    Ok(value)
}

/// Canonicalize a JSON value so formatting changes in committed schemas don't
/// spuriously trip the drift test. Recursively sorts object keys.
pub fn canonicalize(v: &Value) -> Value {
    match v {
        Value::Object(map) => {
            let mut sorted: Vec<(String, Value)> = map
                .iter()
                .map(|(k, v)| (k.clone(), canonicalize(v)))
                .collect();
            sorted.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(arr) => Value::Array(arr.iter().map(canonicalize).collect()),
        other => other.clone(),
    }
}
