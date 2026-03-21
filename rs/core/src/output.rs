//! JSONL output: file append and/or stdout.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use anyhow::Result;

use crate::cloud_event::CloudEvent;

/// Write a CloudEvent as a single JSONL line to stdout.
pub fn write_stdout(event: &CloudEvent) -> Result<()> {
    let json = serde_json::to_string(event)?;
    let stdout = std::io::stdout();
    let mut handle = stdout.lock();
    writeln!(handle, "{}", json)?;
    Ok(())
}

/// Append a CloudEvent as a single JSONL line to a file.
pub fn append_file(path: &Path, event: &CloudEvent) -> Result<()> {
    let json = serde_json::to_string(event)?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", json)?;
    Ok(())
}

/// Write events to configured outputs.
pub fn emit_events(events: &[CloudEvent], output_file: Option<&Path>, stdout: bool) -> Result<()> {
    for event in events {
        if stdout {
            write_stdout(event)?;
        }
        if let Some(path) = output_file {
            append_file(path, event)?;
        }
    }
    Ok(())
}
