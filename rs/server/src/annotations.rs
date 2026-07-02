//! Durable, user/agent-authored annotations — the OVERLAY namespace.
//!
//! Sovereignty: annotations are notes people (or agents acting for them) pin to
//! sessions. They are deliberately kept OUT of the observed event stream — this
//! is authored overlay data, not something we watched an agent do. It lives in
//! its own `{data_dir}/annotations.jsonl` (grep-able, portable — the same JSONL
//! escape hatch the rest of the store promises), never mixed into the events.

use std::fs::OpenOptions;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Annotation {
    pub id: String,
    /// The session this note is pinned to.
    pub session_id: String,
    pub body: String,
    /// Who authored it (person or agent), for the overlay's provenance.
    pub issuer: String,
    /// RFC3339 creation timestamp.
    pub created_at: String,
}

fn annotations_path(dir: &Path) -> PathBuf {
    dir.join("annotations.jsonl")
}

/// Append one annotation as a JSONL line, creating the file if needed.
pub fn append_annotation(dir: &Path, a: &Annotation) -> std::io::Result<()> {
    let mut f = OpenOptions::new()
        .create(true)
        .append(true)
        .open(annotations_path(dir))?;
    let line = serde_json::to_string(a).unwrap_or_default();
    writeln!(f, "{line}")
}

/// Read all annotations, skipping malformed lines. Missing file → empty.
pub fn read_annotations(dir: &Path) -> Vec<Annotation> {
    let Ok(f) = std::fs::File::open(annotations_path(dir)) else {
        return Vec::new();
    };
    BufReader::new(f)
        .lines()
        .map_while(Result::ok)
        .filter_map(|l| serde_json::from_str::<Annotation>(&l).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_then_read_roundtrips() {
        let dir = tempfile::tempdir().unwrap();
        let a = Annotation {
            id: "1".into(),
            session_id: "sess-a".into(),
            body: "look here".into(),
            issuer: "agent:claude".into(),
            created_at: "2026-07-01T00:00:00Z".into(),
        };
        let b = Annotation {
            id: "2".into(),
            session_id: "sess-b".into(),
            body: "and here".into(),
            issuer: "max".into(),
            created_at: "2026-07-01T00:01:00Z".into(),
        };
        append_annotation(dir.path(), &a).unwrap();
        append_annotation(dir.path(), &b).unwrap();

        let read = read_annotations(dir.path());
        assert_eq!(read.len(), 2);
        assert_eq!(read[0].id, "1");
        assert_eq!(read[0].body, "look here");
        assert_eq!(read[1].session_id, "sess-b");
    }

    #[test]
    fn read_missing_file_is_empty() {
        let dir = tempfile::tempdir().unwrap();
        assert!(read_annotations(dir.path()).is_empty());
    }

    #[test]
    fn read_skips_malformed_lines() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            annotations_path(dir.path()),
            "not json\n{\"id\":\"9\",\"session_id\":\"s\",\"body\":\"b\",\"issuer\":\"i\",\"created_at\":\"t\"}\n",
        )
        .unwrap();
        let read = read_annotations(dir.path());
        assert_eq!(read.len(), 1);
        assert_eq!(read[0].id, "9");
    }
}
