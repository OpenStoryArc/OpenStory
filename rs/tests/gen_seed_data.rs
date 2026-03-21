//! One-shot test to generate E2E seed data from fixture files.
//! Run with: cargo test --test gen_seed_data -- --ignored
//!
//! Generates CloudEvent JSONL files for E2E tests from:
//!   - synthetic.jsonl (edge-case coverage)
//!   - synth_origin.jsonl (implementation-style session)
//!   - synth_hooks.jsonl (debugging-style session)

use std::io::Write;
use std::path::Path;

use open_story::reader::read_new_lines;
use open_story::translate::TranscriptState;

fn translate_fixture(fixture_name: &str, session_id: &str, out_dir: &Path) {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(fixture_name);

    let mut ts = TranscriptState::new(session_id.to_string());
    let events = read_new_lines(&fixture, &mut ts).unwrap();

    let out_path = out_dir.join(format!("{}.jsonl", session_id));
    let mut f = std::fs::File::create(&out_path).unwrap();

    for event in &events {
        let json = serde_json::to_string(event).unwrap();
        writeln!(f, "{}", json).unwrap();
    }

    eprintln!(
        "Generated {} CloudEvents from {} → {}",
        events.len(),
        fixture_name,
        out_path.display()
    );
}

#[test]
#[ignore]
fn generate_seed_data() {
    let out_dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("e2e")
        .join("fixtures")
        .join("seed-data");

    std::fs::create_dir_all(&out_dir).unwrap();

    translate_fixture("synthetic.jsonl", "synth-session", &out_dir);
    translate_fixture("synth_origin.jsonl", "synth-origin", &out_dir);
    translate_fixture("synth_hooks.jsonl", "synth-hooks", &out_dir);
}
