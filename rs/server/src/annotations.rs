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

/// Remove the annotation with `id` by rewriting the file without it. Returns
/// `true` if one was removed. The overlay is authored data the user owns, so
/// deletion is a first-class operation (unlike the observed event stream, which
/// is append-only). Missing file → `false`.
pub fn remove_annotation(dir: &Path, id: &str) -> std::io::Result<bool> {
    let path = annotations_path(dir);
    let existing = read_annotations(dir);
    let before = existing.len();
    let kept: Vec<Annotation> = existing.into_iter().filter(|a| a.id != id).collect();
    if kept.len() == before {
        return Ok(false); // nothing matched
    }
    // Rewrite atomically-ish: truncate + write the survivors.
    let mut f = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)?;
    for a in &kept {
        writeln!(f, "{}", serde_json::to_string(a).unwrap_or_default())?;
    }
    Ok(true)
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
    fn remove_deletes_only_the_matching_id() {
        let dir = tempfile::tempdir().unwrap();
        let mk = |id: &str, s: &str| Annotation {
            id: id.into(),
            session_id: s.into(),
            body: "b".into(),
            issuer: "i".into(),
            created_at: "t".into(),
        };
        append_annotation(dir.path(), &mk("1", "sess-a")).unwrap();
        append_annotation(dir.path(), &mk("2", "sess-b")).unwrap();
        append_annotation(dir.path(), &mk("3", "sess-c")).unwrap();

        let removed = remove_annotation(dir.path(), "2").unwrap();
        assert!(removed, "should report a removal");

        let read = read_annotations(dir.path());
        assert_eq!(read.len(), 2);
        assert_eq!(read.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(), vec!["1", "3"]);
    }

    #[test]
    fn remove_unknown_id_is_false_and_keeps_all() {
        let dir = tempfile::tempdir().unwrap();
        let a = Annotation {
            id: "1".into(),
            session_id: "s".into(),
            body: "b".into(),
            issuer: "i".into(),
            created_at: "t".into(),
        };
        append_annotation(dir.path(), &a).unwrap();
        assert!(!remove_annotation(dir.path(), "nope").unwrap());
        assert_eq!(read_annotations(dir.path()).len(), 1);
    }

    #[test]
    fn remove_from_missing_file_is_false() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!remove_annotation(dir.path(), "x").unwrap());
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
