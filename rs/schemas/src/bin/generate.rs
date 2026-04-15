//! Generate all committed JSON Schema files from the Rust types.
//!
//! Run: `cargo run -p open-story-schemas --bin generate`
//!
//! Drift-check in CI: run this, then `git diff --exit-code schemas/`.

use anyhow::Result;
use open_story_bus::IngestBatch;
use open_story_core::cloud_event::CloudEvent;
use open_story_core::subtype::Subtype;
use open_story_patterns::{PatternEvent, StructuralTurn};
use open_story_schemas::{schema_dir, write_schema};
use open_story_server::broadcast::BroadcastMessage;
use open_story_store::event_store::SessionRow;
use open_story_store::queries::FtsSearchResult;
use open_story_views::view_record::ViewRecord;
use open_story_views::wire_record::WireRecord;

fn main() -> Result<()> {
    let dir = schema_dir();
    std::fs::create_dir_all(&dir)?;

    write_schema::<Subtype>("subtype.schema.json")?;
    write_schema::<CloudEvent>("cloud_event.schema.json")?;
    write_schema::<ViewRecord>("view_record.schema.json")?;
    write_schema::<WireRecord>("wire_record.schema.json")?;
    write_schema::<PatternEvent>("pattern_event.schema.json")?;
    write_schema::<StructuralTurn>("structural_turn.schema.json")?;
    write_schema::<IngestBatch>("ingest_batch.schema.json")?;
    write_schema::<SessionRow>("session_row.schema.json")?;
    write_schema::<FtsSearchResult>("fts_search_result.schema.json")?;
    write_schema::<BroadcastMessage>("broadcast_message.schema.json")?;

    eprintln!("schemas written to {}", dir.display());
    Ok(())
}
