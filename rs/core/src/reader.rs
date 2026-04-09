//! Incremental file reader with partial-line safety.
//!
//! Reads new complete lines from a JSONL file since the last read position.
//! Partial lines (no trailing newline) are NOT consumed — the byte offset
//! stays put so the next read picks up the complete line.

use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::Path;

use anyhow::Result;
use serde_json::Value;

use crate::cloud_event::CloudEvent;
use crate::translate::{translate_line, TranscriptFormat, TranscriptState};
use crate::translate_pi::{is_pi_mono_format, translate_pi_line};

/// Detect a pre-translated CloudEvent line. The shape is unambiguous:
/// `specversion: "1.0"` plus `type: "io.arc.event"`. Raw Claude Code and
/// Pi-mono lines never have these fields.
fn is_cloud_event(obj: &Value) -> bool {
    obj.get("specversion").and_then(|v| v.as_str()) == Some("1.0")
        && obj.get("type").and_then(|v| v.as_str()) == Some("io.arc.event")
}

/// Read new complete lines from a transcript file, translate to CloudEvents.
///
/// Auto-detects the transcript format (Claude Code vs pi-mono) on the first
/// line and locks the format for the file's lifetime. Dispatches to the
/// appropriate translator.
///
/// Updates `state.byte_offset` and `state.line_count` in place.
/// Partial lines (no trailing `\n`) are left unconsumed.
pub fn read_new_lines(file_path: &Path, state: &mut TranscriptState) -> Result<Vec<CloudEvent>> {
    if !file_path.exists() {
        return Ok(vec![]);
    }

    let file = File::open(file_path)?;
    let file_len = file.metadata()?.len();

    // Nothing new to read
    if file_len <= state.byte_offset {
        return Ok(vec![]);
    }

    let mut reader = BufReader::new(file);
    reader.seek(SeekFrom::Start(state.byte_offset))?;

    let mut events = Vec::new();
    let mut line_buf = String::new();

    loop {
        line_buf.clear();
        let bytes_read = reader.read_line(&mut line_buf)?;

        if bytes_read == 0 {
            // EOF reached
            break;
        }

        // Partial line check: if the line doesn't end with \n, it's incomplete.
        // Do NOT advance byte_offset — re-read next time.
        if !line_buf.ends_with('\n') {
            break;
        }

        // Advance offset past this complete line
        state.byte_offset += bytes_read as u64;
        state.line_count += 1;

        let trimmed = line_buf.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse JSON — skip invalid lines
        let obj: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Pre-translated CloudEvent passthrough.
        // If a line is already a CloudEvent (specversion + io.arc.event type),
        // deserialize it directly instead of running it through translate_line
        // — translation would be a no-op at best and a parse failure at worst.
        // This lets test fixtures and replay tooling write CloudEvents to JSONL
        // and have the watcher load them faithfully.
        if is_cloud_event(&obj) {
            if let Ok(ce) = serde_json::from_value::<CloudEvent>(obj.clone()) {
                events.push(ce);
                continue;
            }
        }

        // Detect format once per file, then lock
        if state.format == TranscriptFormat::Unknown {
            state.format = if is_pi_mono_format(&obj) {
                TranscriptFormat::PiMono
            } else {
                TranscriptFormat::ClaudeCode
            };
        }

        let new_events = match state.format {
            TranscriptFormat::PiMono => translate_pi_line(&obj, state),
            _ => translate_line(&obj, state),
        };
        events.extend(new_events);
    }

    Ok(events)
}
