//! Capstone dogfood: every line in every committed session JSONL
//! backup must validate against cloud_event.schema.json.
//!
//! This is the sovereignty contract made executable. CLAUDE.md says:
//!
//!   > JSONL backup is always on disk. Whichever durable EventStore is
//!   > selected, the SessionStore JSONL appender keeps writing per-session
//!   > *.jsonl files in data_dir. That's the sovereignty escape hatch
//!   > the project promises: your data is always grep-able from outside
//!   > the database, regardless of which backend you choose.
//!
//! If a line on disk doesn't validate, the escape hatch has drifted from
//! the declared schema — and "grep-able from outside" becomes "grep-able,
//! but good luck trusting it." That's the failure this test catches.
//!
//! Run:
//!   cargo test -p open-story-schemas --test test_jsonl_escape_hatch -- --ignored --nocapture

use std::path::PathBuf;

use open_story_schemas::load_schema;
use serde_json::Value;

/// Where the local committed JSONL backups live. Repo-relative path.
fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("data")
}

/// How many files to sample. The full 361-file set works but is slow.
const FILE_SAMPLE: usize = 40;

/// Status (2026-04-15, first run): FAILING on real data. ~273 bad lines
/// across 3 of 40 sampled files (all written Apr 7). Root cause: torn
/// or concatenated JSONL lines — two CloudEvents on one line with no
/// newline separator. That's a concurrent-write / interrupted-write
/// issue in the SessionStore JSONL appender, NOT a schema mismatch.
///
/// Not silencing — this is the capstone finding its job. The
/// sovereignty contract ("your data is always grep-able from outside
/// the database") is violated when lines aren't line-shaped. See
/// BACKLOG.md entry "JSONL Escape-Hatch Append Integrity".
#[test]
#[ignore = "scans local data/ directory — documents the append-integrity bug"]
fn every_line_in_every_session_jsonl_validates_as_cloud_event() {
    let schema = load_schema("cloud_event.schema.json").expect("schema");
    let validator = jsonschema::validator_for(&schema).expect("compile");

    let dir = data_dir();
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap_or_else(|_| panic!("read {}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "jsonl").unwrap_or(false))
        .collect();
    files.sort();
    let files: Vec<_> = files.into_iter().take(FILE_SAMPLE).collect();
    assert!(!files.is_empty(), "no JSONL files at {}", dir.display());

    let mut total_lines = 0usize;
    let mut invalid_lines = 0usize;
    let mut first_failures: Vec<(PathBuf, usize, String)> = Vec::new();
    let mut per_file: Vec<(PathBuf, usize, usize)> = Vec::new(); // (path, ok, bad)

    for path in &files {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(_) => continue,
        };
        let mut ok = 0usize;
        let mut bad = 0usize;
        for (i, line) in text.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            total_lines += 1;
            let value: Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(e) => {
                    bad += 1;
                    invalid_lines += 1;
                    if first_failures.len() < 5 {
                        first_failures.push((path.clone(), i + 1, format!("JSON parse: {e}")));
                    }
                    continue;
                }
            };
            let errors: Vec<_> = validator.iter_errors(&value).collect();
            if errors.is_empty() {
                ok += 1;
            } else {
                bad += 1;
                invalid_lines += 1;
                if first_failures.len() < 5 {
                    let msg = errors
                        .iter()
                        .take(3)
                        .map(|e| format!("  {}: {}", e.instance_path, e))
                        .collect::<Vec<_>>()
                        .join("\n");
                    first_failures.push((path.clone(), i + 1, msg));
                }
            }
        }
        per_file.push((path.clone(), ok, bad));
    }

    eprintln!("\n── JSONL escape-hatch validation ({} files) ──", files.len());
    let files_ok = per_file.iter().filter(|(_, _, b)| *b == 0).count();
    let files_bad = per_file.len() - files_ok;
    eprintln!("  files fully valid : {files_ok}");
    eprintln!("  files with errors : {files_bad}");
    eprintln!("  total lines       : {total_lines}");
    eprintln!("  invalid lines     : {invalid_lines}");

    if !first_failures.is_empty() {
        eprintln!("\n❌ first {} failure(s):", first_failures.len());
        for (path, lineno, msg) in &first_failures {
            eprintln!(
                "  {}:{}",
                path.file_name().unwrap().to_string_lossy(),
                lineno
            );
            eprintln!("{msg}");
        }
    }

    assert_eq!(
        invalid_lines, 0,
        "{invalid_lines} line(s) in the escape-hatch JSONL do not validate — sovereignty contract broken"
    );
}
