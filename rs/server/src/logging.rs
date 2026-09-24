//! Logging helpers (metadata only — never log message content).

use chrono::Local;
use open_story_core::cloud_event::CloudEvent;

/// Short session ID for display (first 8 chars). UTF-8-safe.
///
/// `id[..id.len().min(8)]` panics when byte 8 falls in the middle of a
/// multi-byte char. Session IDs from agents are typically UUIDs (ASCII),
/// but custom session ids and project ids can be unicode. Bug found
/// 2026-04-15 during audit walk #9. See
/// docs/research/architecture-audit/API_WALK.md F-1.
pub fn short_id(id: &str) -> &str {
    open_story_core::strings::truncate_at_char_boundary(id, 8)
}

/// Re-export for callers that import via this module.
pub use open_story_core::strings::truncate_at_char_boundary;

/// Format a log line with timestamp, category label, and message.
pub fn log_event(category: &str, message: &str) {
    let now = Local::now().format("%H:%M:%S");
    eprintln!("\x1b[2m{now}\x1b[0m \x1b[36m{category:>5}\x1b[0m {message}");
}

/// Summarize a batch of CloudEvents as a compact subtype list.
/// e.g. "message.user.prompt, progress.bash"
pub fn event_type_summary(events: &[CloudEvent]) -> String {
    let types: Vec<&str> = events
        .iter()
        .map(|e| e.subtype.as_deref().unwrap_or(&e.event_type))
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── short_id / truncate_at_char_boundary (audit walk #9) ──────────

    #[test]
    fn short_id_truncates_ascii_to_8_chars() {
        assert_eq!(short_id("0123456789abcdef"), "01234567");
    }

    #[test]
    fn short_id_returns_full_string_when_shorter_than_8() {
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id(""), "");
    }

    #[test]
    fn short_id_does_not_panic_on_multibyte_at_byte_8() {
        // BUG that this fix closes: previously `&id[..id.len().min(8)]`
        // panicked when byte 8 fell inside a multi-byte character.
        //
        // Fixture: "abc日日" — "abc" = 3 bytes, each 日 = 3 bytes.
        //   Total: 9 bytes.
        //   Byte 8 lands inside the SECOND 日 (which spans bytes 6..9).
        //   Old impl: panic with "byte index 8 is not a char boundary".
        //   New impl: backs up to char boundary (byte 6) and returns "abc日".
        //
        // I confirmed the panic with a standalone rustc binary before
        // committing the fix.
        let id = "abc日日";
        assert_eq!(id.len(), 9);
        let truncated = short_id(id);
        // Returns "abc日" — 6 bytes, the largest valid prefix ≤ 8 bytes.
        assert_eq!(truncated, "abc日");
    }

    #[test]
    fn truncate_handles_japanese_at_byte_50() {
        // 日 = 3 bytes. 17 of them = 51 bytes. Asking for 50 lands
        // mid-char; helper must back up to a boundary.
        let s = "日".repeat(17);
        assert_eq!(s.len(), 51);
        let truncated = truncate_at_char_boundary(&s, 50);
        assert!(truncated.len() <= 50);
        assert_eq!(
            truncated.len() % 3,
            0,
            "must be a multiple of 3 bytes (whole 日 chars)"
        );
        // Should be 16 日 chars = 48 bytes
        assert_eq!(truncated.chars().count(), 16);
    }

    #[test]
    fn truncate_zero_max_bytes_returns_empty() {
        assert_eq!(truncate_at_char_boundary("hello", 0), "");
    }

    #[test]
    fn truncate_max_bytes_exactly_at_boundary() {
        // 4-byte emoji, ask for exactly 4 bytes — should return the emoji
        let s = "🦀tail";
        let truncated = truncate_at_char_boundary(s, 4);
        assert_eq!(truncated, "🦀");
    }

    // ── event_type_summary basic coverage (was untested) ──────────────

    #[test]
    fn event_type_summary_empty_input_returns_empty_string() {
        assert_eq!(event_type_summary(&[]), "");
    }
}

// ── Structured logging (REQUIREMENTS L-01) ──────────────────────────────
//
// `tracing` is the API; this module owns the two formatters. `Text` keeps
// the look a person expects at a terminal. `Json` writes one object per
// line with stable top-level names so an agent, a log ring, or `jq` can
// read a node's logs without guessing: `ts`, `level`, `target`, `event`,
// `message`, then every field on the record. Never log message content.

use tracing::Subscriber;
use tracing_subscriber::fmt::format::Writer;
use tracing_subscriber::fmt::{FmtContext, FormatEvent, FormatFields, MakeWriter};
use tracing_subscriber::layer::{Context, Layer, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::EnvFilter;

/// How log lines are written. Parsed from `log_format` in config or
/// `OPEN_STORY_LOG_FORMAT`; empty means `Text`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogFormat {
    #[default]
    Text,
    Json,
}

impl std::str::FromStr for LogFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "" | "text" => Ok(LogFormat::Text),
            "json" => Ok(LogFormat::Json),
            other => Err(format!(
                "unknown log_format {other:?}; expected \"text\" or \"json\""
            )),
        }
    }
}

/// Collects a record's fields into a JSON map. `message` is the format
/// string's rendered text; every other field keeps its name.
#[derive(Default)]
struct JsonFields(serde_json::Map<String, serde_json::Value>);

impl tracing::field::Visit for JsonFields {
    fn record_f64(&mut self, f: &tracing::field::Field, v: f64) {
        self.0.insert(f.name().to_string(), serde_json::json!(v));
    }
    fn record_i64(&mut self, f: &tracing::field::Field, v: i64) {
        self.0.insert(f.name().to_string(), serde_json::json!(v));
    }
    fn record_u64(&mut self, f: &tracing::field::Field, v: u64) {
        self.0.insert(f.name().to_string(), serde_json::json!(v));
    }
    fn record_bool(&mut self, f: &tracing::field::Field, v: bool) {
        self.0.insert(f.name().to_string(), serde_json::json!(v));
    }
    fn record_str(&mut self, f: &tracing::field::Field, v: &str) {
        self.0.insert(f.name().to_string(), serde_json::json!(v));
    }
    fn record_debug(&mut self, f: &tracing::field::Field, v: &dyn std::fmt::Debug) {
        self.0
            .insert(f.name().to_string(), serde_json::json!(format!("{v:?}")));
    }
}

/// Keeps each span's fields as a JSON map in the span's extensions so the
/// line formatter can merge them onto every event emitted inside the span.
/// A consumer task runs inside `info_span!("consumer", actor = "persist")`;
/// that is how `actor` reaches every line (L-02) without threading it
/// through every call.
struct SpanFields;

impl<S> Layer<S> for SpanFields
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(
        &self,
        attrs: &tracing::span::Attributes<'_>,
        id: &tracing::span::Id,
        ctx: Context<'_, S>,
    ) {
        let mut fields = JsonFields::default();
        attrs.record(&mut fields);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(fields);
        }
    }
}

/// One JSON object per line: `{"ts","level","target","event",...fields}`.
/// Span fields come first (root to leaf), then the record's own fields, so a
/// leaf span or the record itself wins on a name clash. A record with no
/// `event` is stamped `event=unnamed`: findable, never silent.
struct JsonLine;

impl<S, N> FormatEvent<S, N> for JsonLine
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        _ctx: &FmtContext<'_, S, N>,
        mut writer: Writer<'_>,
        event: &tracing::Event<'_>,
    ) -> std::fmt::Result {
        let meta = event.metadata();
        let mut fields = JsonFields::default();
        event.record(&mut fields);
        let mut obj = serde_json::Map::new();
        obj.insert(
            "ts".into(),
            serde_json::json!(
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
            ),
        );
        obj.insert("level".into(), serde_json::json!(meta.level().as_str()));
        obj.insert("target".into(), serde_json::json!(meta.target()));
        if let Some(scope) = _ctx.event_scope() {
            for span in scope.from_root() {
                if let Some(sf) = span.extensions().get::<JsonFields>() {
                    for (k, v) in &sf.0 {
                        obj.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        for (k, v) in fields.0 {
            obj.insert(k, v);
        }
        obj.entry("event")
            .or_insert_with(|| serde_json::json!("unnamed"));
        let line = serde_json::to_string(&obj).map_err(|_| std::fmt::Error)?;
        writer.write_str(&line)?;
        writeln!(writer)
    }
}

/// Build a subscriber for `format`, filtered by `filter` (an `EnvFilter`
/// directive string such as `info` or `open_story=debug`), writing to
/// `writer`. Pure with respect to process state: nothing global is set.
pub fn build_subscriber<W>(
    format: LogFormat,
    filter: &str,
    writer: W,
) -> Box<dyn Subscriber + Send + Sync>
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let filter = EnvFilter::try_new(filter).unwrap_or_else(|_| EnvFilter::new("info"));
    match format {
        LogFormat::Json => Box::new(
            tracing_subscriber::registry()
                .with(filter)
                .with(SpanFields)
                .with(
                    tracing_subscriber::fmt::layer()
                        .event_format(JsonLine)
                        .with_writer(writer),
                ),
        ),
        LogFormat::Text => Box::new(
            tracing_subscriber::registry().with(filter).with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .with_writer(writer),
            ),
        ),
    }
}

/// Install the process-wide subscriber: stderr, `RUST_LOG` if set (default
/// `info`). Safe to call once; a second call is a no-op that returns false.
pub fn init(format: LogFormat) -> bool {
    let filter = std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string());
    tracing::subscriber::set_global_default(build_subscriber(format, &filter, std::io::stderr))
        .is_ok()
}
