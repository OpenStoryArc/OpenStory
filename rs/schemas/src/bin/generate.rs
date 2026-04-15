//! Generate all committed JSON Schema files from the Rust types.
//!
//! Run: `cargo run -p open-story-schemas --bin generate`
//!
//! Drift-check in CI: run this, then `git diff --exit-code schemas/`.

use anyhow::Result;
use open_story_schemas::schema_dir;

fn main() -> Result<()> {
    let dir = schema_dir();
    std::fs::create_dir_all(&dir)?;

    // Schemas land here as each TDD cycle completes. Empty for now —
    // the scaffolding commit only establishes the crate + binary.
    //
    // Next cycle (Subtype refactor + CloudEvent) adds:
    //   write_schema::<CloudEvent>(&dir, "cloud_event.schema.json")?;

    eprintln!(
        "schema generator scaffolded at {} — no schemas written yet",
        dir.display()
    );
    Ok(())
}
