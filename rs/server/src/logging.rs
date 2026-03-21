//! Logging helpers (metadata only — never log message content).

use open_story_core::cloud_event::CloudEvent;
use chrono::Local;

/// Short session ID for display (first 8 chars).
pub fn short_id(id: &str) -> &str {
    &id[..id.len().min(8)]
}

/// Format a log line with timestamp, category label, and message.
pub fn log_event(category: &str, message: &str) {
    let now = Local::now().format("%H:%M:%S");
    eprintln!(
        "\x1b[2m{now}\x1b[0m \x1b[36m{category:>5}\x1b[0m {message}"
    );
}

/// Summarize a batch of CloudEvents as a compact subtype list.
/// e.g. "message.user.prompt, progress.bash"
pub fn event_type_summary(events: &[CloudEvent]) -> String {
    let types: Vec<&str> = events
        .iter()
        .map(|e| {
            e.subtype
                .as_deref()
                .unwrap_or(&e.event_type)
        })
        .collect();
    if types.is_empty() {
        return String::new();
    }
    // Deduplicate while preserving order, show counts for repeated types
    let mut seen: Vec<(&str, usize)> = Vec::new();
    for t in &types {
        if let Some(entry) = seen.iter_mut().find(|(name, _)| name == t) {
            entry.1 += 1;
        } else {
            seen.push((t, 1));
        }
    }
    seen.iter()
        .map(|(name, count)| {
            if *count > 1 {
                format!("{name} x{count}")
            } else {
                name.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}
