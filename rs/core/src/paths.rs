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

/// Compose a hierarchical NATS subject from a JSONL file path.
///
/// Main agent:  `{watch_dir}/{project}/{session}.jsonl`
///              → `events.{project}.{session}.main`
///
/// Subagent:    `{watch_dir}/{project}/{session}/subagents/agent-{id}.jsonl`
///              → `events.{project}.{session}.agent.{id}`
///
/// The subject hierarchy encodes the parent-child relationship so NATS
/// wildcard subscriptions can target a session + all its subagents.
pub fn nats_subject_from_path(path: &Path, watch_dir: &Path) -> String {
    let project = project_id_from_path(path, watch_dir)
        .unwrap_or_else(|| "unknown".to_string());
    let file_stem = path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Check if this is a subagent file: .../subagents/agent-{id}.jsonl
    let is_subagent = path.components().any(|c| c.as_os_str() == "subagents")
        && file_stem.starts_with("agent-");

    if is_subagent {
        // Extract agent_id by stripping "agent-" prefix
        let agent_id = &file_stem["agent-".len()..];
        // Parent session_id is the directory name containing "subagents/"
        // Path: {project}/{session}/subagents/agent-{id}.jsonl
        let parent_session = path.parent()  // subagents/
            .and_then(|p| p.parent())       // {session}/
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");
        format!("events.{project}.{parent_session}.agent.{agent_id}")
    } else {
        format!("events.{project}.{file_stem}.main")
    }
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

    // -- nats_subject_from_path --

    #[test]
    fn subject_for_main_agent_session() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/my-project/06907d46-uuid.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch_dir),
            "events.my-project.06907d46-uuid.main"
        );
    }

    #[test]
    fn subject_for_subagent_session() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/my-project/06907d46-uuid/subagents/agent-a6dcf911.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch_dir),
            "events.my-project.06907d46-uuid.agent.a6dcf911"
        );
    }

    #[test]
    fn subject_strips_agent_prefix_from_filename() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/proj/sess-123/subagents/agent-abc123def.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch_dir),
            "events.proj.sess-123.agent.abc123def"
        );
    }

    #[test]
    fn subject_fallback_when_no_project() {
        let watch_dir = PathBuf::from("/home/user/.claude/projects");
        let path = PathBuf::from("/home/user/.claude/projects/session.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch_dir),
            "events.unknown.session.main"
        );
    }

    // ── T3: characterization — current behavior under unusual paths ────
    // Documents today's raw-interpolation behavior so regressions (or a
    // future sanitizer) are explicit. See
    // docs/research/architecture-audit/T3_NATS_SUBJECT_ALIGNMENT.md

    #[test]
    fn subject_dotted_project_name_produces_extra_tokens() {
        // A project dir named "my.project" yields 5 tokens instead of 4.
        // Coarse `events.>` subscription still matches, but any
        // hierarchical filter `events.{project}.>` would not.
        let watch = PathBuf::from("/watch");
        let path = PathBuf::from("/watch/my.project/sess.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch),
            "events.my.project.sess.main"
        );
    }

    #[test]
    fn subject_space_in_project_name_produces_invalid_nats_subject() {
        // NATS rejects subjects containing spaces at publish time.
        // This test records today's behavior: no sanitization, space
        // flows through verbatim. The event would fail to publish.
        let watch = PathBuf::from("/watch");
        let path = PathBuf::from("/watch/My Project/sess.jsonl");
        let subject = nats_subject_from_path(&path, &watch);
        assert_eq!(subject, "events.My Project.sess.main");
        assert!(subject.contains(' '), "space survives into subject — NATS publish will fail");
    }

    #[test]
    fn subject_nats_wildcard_chars_in_path_pass_through() {
        // `*` and `>` are NATS wildcards. If they appear in a filename
        // they'd shadow the subscription matching. Rare but diagnostic.
        let watch = PathBuf::from("/watch");
        let path = PathBuf::from("/watch/proj/sess-*.jsonl");
        assert_eq!(
            nats_subject_from_path(&path, &watch),
            "events.proj.sess-*.main"
        );
    }
}
