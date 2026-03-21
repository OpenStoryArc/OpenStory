//! Path utilities for deriving session and project IDs from transcript file paths.

use std::path::Path;

/// Derive a session ID from a JSONL file path.
/// Uses the file stem (filename without extension).
pub fn session_id_from_path(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

/// Derive a project ID from a JSONL file path relative to the watch directory.
///
/// Returns the immediate child directory of `watch_dir` that contains the file.
/// For example, given `watch_dir/project-a/sess.jsonl`, returns `Some("project-a")`.
/// Returns `None` if the file is directly in `watch_dir` (no project subdirectory).
pub fn project_id_from_path(path: &Path, watch_dir: &Path) -> Option<String> {
    let relative = path.strip_prefix(watch_dir).ok()?;
    let mut components = relative.components();
    let first = components.next()?;
    // If there's no second component, the file is directly in watch_dir
    components.next()?;
    first.as_os_str().to_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // -- session_id_from_path --

    #[test]
    fn session_id_extracts_file_stem() {
        let path = PathBuf::from("/home/user/.claude/projects/my-project/abc123.jsonl");
        assert_eq!(session_id_from_path(&path), "abc123");
    }

    #[test]
    fn session_id_handles_uuid_format() {
        let path = PathBuf::from("/data/002baf80-3d86-4596-8184-ba60725e02d8.jsonl");
        assert_eq!(
            session_id_from_path(&path),
            "002baf80-3d86-4596-8184-ba60725e02d8"
        );
    }

    #[test]
    fn session_id_returns_unknown_for_no_stem() {
        let path = PathBuf::from("/");
        assert_eq!(session_id_from_path(&path), "unknown");
    }

    #[test]
    fn session_id_strips_only_last_extension() {
        let path = PathBuf::from("/data/session.backup.jsonl");
        assert_eq!(session_id_from_path(&path), "session.backup");
    }

    // -- project_id_from_path --

    #[test]
    fn project_id_extracts_parent_dir() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/my-project/session123.jsonl");
        assert_eq!(
            project_id_from_path(&path, &watch_dir),
            Some("my-project".to_string())
        );
    }

    #[test]
    fn project_id_none_when_direct_in_watch_dir() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/session123.jsonl");
        assert_eq!(project_id_from_path(&path, &watch_dir), None);
    }

    #[test]
    fn project_id_returns_first_component_for_nested() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/project-a/sub/session.jsonl");
        assert_eq!(
            project_id_from_path(&path, &watch_dir),
            Some("project-a".to_string())
        );
    }

    #[test]
    fn project_id_handles_encoded_path_format() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from(
            "/home/user/.claude/projects/-home-user-projects-open-story/abc123-uuid.jsonl",
        );
        assert_eq!(
            project_id_from_path(&path, &watch_dir),
            Some("-home-user-projects-open-story".to_string())
        );
    }

    #[test]
    fn project_id_none_when_path_unrelated() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/tmp/other/session.jsonl");
        assert_eq!(project_id_from_path(&path, &watch_dir), None);
    }
}
