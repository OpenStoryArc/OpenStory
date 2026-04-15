//! Generate all committed JSON Schema files from the Rust types.
//!
//! Run: `cargo run -p open-story-schemas --bin generate`
//!
//! Drift-check in CI: run this, then `git diff --exit-code schemas/`.

use anyhow::Result;
use open_story_core::cloud_event::CloudEvent;
use open_story_core::subtype::Subtype;
use open_story_schemas::{schema_dir, write_schema};
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::WireRecord;

fn main() -> Result<()> {
    let dir = schema_dir();
    std::fs::create_dir_all(&dir)?;

    write_schema::<Subtype>("subtype.schema.json")?;
    write_schema::<CloudEvent>("cloud_event.schema.json")?;
    write_schema::<ViewRecord>("view_record.schema.json")?;
    write_schema::<WireRecord>("wire_record.schema.json")?;

    eprintln!("schemas written to {}", dir.display());
    Ok(())
}
