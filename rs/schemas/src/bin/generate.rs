//! Generate all committed JSON Schema files from the Rust types.
//!
//! Run: `cargo run -p open-story-schemas --bin generate`
//!
//! Drift-check in CI: run this, then `git diff --exit-code schemas/`.

use anyhow::Result;
use open_story_core::subtype::Subtype;
use open_story_schemas::{schema_dir, write_schema};

fn main() -> Result<()> {
    let dir = schema_dir();
    std::fs::create_dir_all(&dir)?;

    write_schema::<Subtype>("subtype.schema.json")?;

    eprintln!("schemas written to {}", dir.display());
    Ok(())
}
