//! Live analysis over shape rows — the Rust-side fold the actor maintains per
//! session and broadcasts to the UI (which is a dumb sink).
//!
//! Phase 1 is intentionally thin: cumulative running counts. The sliding-window
//! trajectory layer (shape-space classification + drift) builds on this later.
//! Pure — rows in, counts mutated in place, no I/O.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::ShapeRow;

/// Running per-session tallies over the shapes seen so far. `BTreeMap`s keep the
/// tallies deterministically ordered for stable wire output and tests.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct ShapeCounts {
    /// Count of bash-shape / path-shape / change-shape rows.
    pub bash: u64,
    pub path: u64,
    pub change: u64,
    /// Summed change deltas across change-shape rows.
    pub lines_added: u64,
    pub lines_removed: u64,
    /// Tally of shell programs (bash-shape `data.program`).
    pub programs: BTreeMap<String, u64>,
    /// Tally of top path segments (path-shape `data.top_segment`).
    pub top_segments: BTreeMap<String, u64>,
}

impl ShapeCounts {
    /// Fold a batch of rows into the running counts.
    pub fn ingest(&mut self, rows: &[ShapeRow]) {
        for row in rows {
            match row.shape_type.as_str() {
                "bash-shape" => {
                    self.bash += 1;
                    if let Some(p) = str_field(row, "program") {
                        *self.programs.entry(p).or_default() += 1;
                    }
                }
                "path-shape" => {
                    self.path += 1;
                    if let Some(s) = str_field(row, "top_segment") {
                        *self.top_segments.entry(s).or_default() += 1;
                    }
                }
                "change-shape" => {
                    self.change += 1;
                    self.lines_added += u64_field(row, "lines_added");
                    self.lines_removed += u64_field(row, "lines_removed");
                }
                _ => {}
            }
        }
    }

    /// Total shape rows folded so far.
    pub fn total(&self) -> u64 {
        self.bash + self.path + self.change
    }
}

fn str_field(row: &ShapeRow, key: &str) -> Option<String> {
    row.data
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

fn u64_field(row: &ShapeRow, key: &str) -> u64 {
    row.data.get(key).and_then(|v| v.as_u64()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(shape_type: &str, data: serde_json::Value) -> ShapeRow {
        ShapeRow {
            id: format!("e:{shape_type}:0"),
            session_id: "s".into(),
            shape_type: shape_type.into(),
            seq: 0,
            timestamp: "2026-01-01T00:00:00Z".into(),
            event_id: "e".into(),
            data,
        }
    }

    #[test]
    fn ingest_accumulates_per_type_and_tallies() {
        let mut c = ShapeCounts::default();
        c.ingest(&[
            row("bash-shape", json!({"program": "git"})),
            row("bash-shape", json!({"program": "git"})),
            row("path-shape", json!({"top_segment": "rs"})),
            row("change-shape", json!({"lines_added": 10, "lines_removed": 3})),
        ]);
        assert_eq!(c.bash, 2);
        assert_eq!(c.path, 1);
        assert_eq!(c.change, 1);
        assert_eq!(c.lines_added, 10);
        assert_eq!(c.lines_removed, 3);
        assert_eq!(c.programs.get("git"), Some(&2));
        assert_eq!(c.top_segments.get("rs"), Some(&1));
        assert_eq!(c.total(), 4);
    }

    #[test]
    fn ingest_is_cumulative_across_batches() {
        let mut c = ShapeCounts::default();
        c.ingest(&[row("bash-shape", json!({"program": "cargo"}))]);
        c.ingest(&[row("bash-shape", json!({"program": "cargo"}))]);
        assert_eq!(c.bash, 2);
        assert_eq!(c.programs.get("cargo"), Some(&2));
    }

    #[test]
    fn ingest_skips_empty_and_unknown_fields() {
        let mut c = ShapeCounts::default();
        c.ingest(&[
            row("bash-shape", json!({"program": ""})), // empty program not tallied
            row("change-shape", json!({})),            // missing deltas → 0
        ]);
        assert_eq!(c.bash, 1);
        assert!(c.programs.is_empty());
        assert_eq!(c.change, 1);
        assert_eq!(c.lines_added, 0);
    }
}
