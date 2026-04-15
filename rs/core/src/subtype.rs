//! The Subtype enum — closed set of event classifier strings.
//!
//! Today these live as ~330 string literals scattered across 30 files.
//! This module makes the taxonomy explicit and compile-checked.
//!
//! See `docs/research/architecture-audit/SCHEMA_MAP.md` §Layer H for the
//! full list and rationale.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Event subtype — hierarchical classifier on every CloudEvent.
///
/// The dot-separated string form is what flows over the wire; the enum
/// variants give us exhaustive matching at compile time and one single
/// source of truth for the JSON Schema.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Subtype {
    // ── message.* — conversation ──
    #[serde(rename = "message.user.prompt")]
    UserPrompt,
    #[serde(rename = "message.user.tool_result")]
    UserToolResult,
    #[serde(rename = "message.assistant.text")]
    AssistantText,
    #[serde(rename = "message.assistant.thinking")]
    AssistantThinking,
    #[serde(rename = "message.assistant.tool_use")]
    AssistantToolUse,

    // ── system.* — runtime lifecycle ──
    #[serde(rename = "system.turn.complete")]
    TurnComplete,
    #[serde(rename = "system.error")]
    SystemError,
    #[serde(rename = "system.compact")]
    SystemCompact,
    #[serde(rename = "system.hook")]
    SystemHook,
    #[serde(rename = "system.session_start")]
    SessionStart,
    #[serde(rename = "system.model_change")]
    ModelChange,
    #[serde(rename = "system.local_command")]
    LocalCommand,
    #[serde(rename = "system.away_summary")]
    AwaySummary,

    // ── progress.* — ephemeral streaming ──
    #[serde(rename = "progress.bash")]
    ProgressBash,
    #[serde(rename = "progress.agent")]
    ProgressAgent,
    #[serde(rename = "progress.hook")]
    ProgressHook,

    // ── file.* — filesystem / git snapshots ──
    #[serde(rename = "file.snapshot")]
    FileSnapshot,

    // ── queue.* — internal queue lifecycle ──
    #[serde(rename = "queue.enqueue")]
    QueueEnqueue,
    #[serde(rename = "queue.dequeue")]
    QueueDequeue,
    #[serde(rename = "queue.remove")]
    QueueRemove,
    /// Claude Code emits this with camelCase on the wire.
    #[serde(rename = "queue.popAll")]
    QueuePopAll,
}

/// Error returned when parsing a string that isn't a known Subtype.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSubtype(pub String);

impl fmt::Display for UnknownSubtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown subtype: {}", self.0)
    }
}

impl std::error::Error for UnknownSubtype {}

impl Subtype {
    /// The dot-separated wire form. Same string serde writes.
    pub fn as_str(&self) -> &'static str {
        match self {
            Subtype::UserPrompt => "message.user.prompt",
            Subtype::UserToolResult => "message.user.tool_result",
            Subtype::AssistantText => "message.assistant.text",
            Subtype::AssistantThinking => "message.assistant.thinking",
            Subtype::AssistantToolUse => "message.assistant.tool_use",
            Subtype::TurnComplete => "system.turn.complete",
            Subtype::SystemError => "system.error",
            Subtype::SystemCompact => "system.compact",
            Subtype::SystemHook => "system.hook",
            Subtype::SessionStart => "system.session_start",
            Subtype::ModelChange => "system.model_change",
            Subtype::LocalCommand => "system.local_command",
            Subtype::AwaySummary => "system.away_summary",
            Subtype::ProgressBash => "progress.bash",
            Subtype::ProgressAgent => "progress.agent",
            Subtype::ProgressHook => "progress.hook",
            Subtype::FileSnapshot => "file.snapshot",
            Subtype::QueueEnqueue => "queue.enqueue",
            Subtype::QueueDequeue => "queue.dequeue",
            Subtype::QueueRemove => "queue.remove",
            Subtype::QueuePopAll => "queue.popAll",
        }
    }

    // ── Family predicates ──────────────────────────────────────────────

    pub fn is_message(&self) -> bool {
        matches!(
            self,
            Subtype::UserPrompt
                | Subtype::UserToolResult
                | Subtype::AssistantText
                | Subtype::AssistantThinking
                | Subtype::AssistantToolUse
        )
    }

    pub fn is_system(&self) -> bool {
        matches!(
            self,
            Subtype::TurnComplete
                | Subtype::SystemError
                | Subtype::SystemCompact
                | Subtype::SystemHook
                | Subtype::SessionStart
                | Subtype::ModelChange
                | Subtype::LocalCommand
                | Subtype::AwaySummary
        )
    }

    pub fn is_progress(&self) -> bool {
        matches!(
            self,
            Subtype::ProgressBash | Subtype::ProgressAgent | Subtype::ProgressHook
        )
    }

    pub fn is_file(&self) -> bool {
        matches!(self, Subtype::FileSnapshot)
    }

    pub fn is_queue(&self) -> bool {
        matches!(
            self,
            Subtype::QueueEnqueue
                | Subtype::QueueDequeue
                | Subtype::QueueRemove
                | Subtype::QueuePopAll
        )
    }

    // ── Sub-family predicates ──────────────────────────────────────────

    pub fn is_assistant(&self) -> bool {
        matches!(
            self,
            Subtype::AssistantText | Subtype::AssistantThinking | Subtype::AssistantToolUse
        )
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Subtype::UserPrompt | Subtype::UserToolResult)
    }

    /// True for event kinds that should not be stored durably.
    /// Today: progress.* only. See persist consumer docs.
    pub fn is_ephemeral(&self) -> bool {
        self.is_progress()
    }
}

impl fmt::Display for Subtype {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for Subtype {
    type Err = UnknownSubtype;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "message.user.prompt" => Ok(Subtype::UserPrompt),
            "message.user.tool_result" => Ok(Subtype::UserToolResult),
            "message.assistant.text" => Ok(Subtype::AssistantText),
            "message.assistant.thinking" => Ok(Subtype::AssistantThinking),
            "message.assistant.tool_use" => Ok(Subtype::AssistantToolUse),
            "system.turn.complete" => Ok(Subtype::TurnComplete),
            "system.error" => Ok(Subtype::SystemError),
            "system.compact" => Ok(Subtype::SystemCompact),
            "system.hook" => Ok(Subtype::SystemHook),
            "system.session_start" => Ok(Subtype::SessionStart),
            "system.model_change" => Ok(Subtype::ModelChange),
            "system.local_command" => Ok(Subtype::LocalCommand),
            "system.away_summary" => Ok(Subtype::AwaySummary),
            "progress.bash" => Ok(Subtype::ProgressBash),
            "progress.agent" => Ok(Subtype::ProgressAgent),
            "progress.hook" => Ok(Subtype::ProgressHook),
            "file.snapshot" => Ok(Subtype::FileSnapshot),
            "queue.enqueue" => Ok(Subtype::QueueEnqueue),
            "queue.dequeue" => Ok(Subtype::QueueDequeue),
            "queue.remove" => Ok(Subtype::QueueRemove),
            "queue.popAll" => Ok(Subtype::QueuePopAll),
            other => Err(UnknownSubtype(other.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Shape: each variant has a known wire string ────────────────────
    //
    // Source of truth for the strings is this table. Today it's scattered;
    // after this refactor it's here.

    fn all_variants_with_strings() -> Vec<(Subtype, &'static str)> {
        vec![
            (Subtype::UserPrompt, "message.user.prompt"),
            (Subtype::UserToolResult, "message.user.tool_result"),
            (Subtype::AssistantText, "message.assistant.text"),
            (Subtype::AssistantThinking, "message.assistant.thinking"),
            (Subtype::AssistantToolUse, "message.assistant.tool_use"),
            (Subtype::TurnComplete, "system.turn.complete"),
            (Subtype::SystemError, "system.error"),
            (Subtype::SystemCompact, "system.compact"),
            (Subtype::SystemHook, "system.hook"),
            (Subtype::SessionStart, "system.session_start"),
            (Subtype::ModelChange, "system.model_change"),
            (Subtype::LocalCommand, "system.local_command"),
            (Subtype::AwaySummary, "system.away_summary"),
            (Subtype::ProgressBash, "progress.bash"),
            (Subtype::ProgressAgent, "progress.agent"),
            (Subtype::ProgressHook, "progress.hook"),
            (Subtype::FileSnapshot, "file.snapshot"),
            (Subtype::QueueEnqueue, "queue.enqueue"),
            (Subtype::QueueDequeue, "queue.dequeue"),
            (Subtype::QueueRemove, "queue.remove"),
            (Subtype::QueuePopAll, "queue.popAll"),
        ]
    }

    #[test]
    fn serializes_to_dotted_string_for_every_variant() {
        for (variant, expected) in all_variants_with_strings() {
            let got = serde_json::to_string(&variant).unwrap();
            assert_eq!(got, format!("\"{}\"", expected));
        }
    }

    #[test]
    fn deserializes_from_dotted_string_for_every_variant() {
        for (expected, s) in all_variants_with_strings() {
            let quoted = format!("\"{}\"", s);
            let got: Subtype = serde_json::from_str(&quoted).unwrap();
            assert_eq!(got, expected);
        }
    }

    // ── Shape: as_str() gives the wire form ────────────────────────────
    // Needed for log lines, SQL params, hash keys. Avoids serde roundtrip.

    #[test]
    fn as_str_returns_dotted_string() {
        for (variant, expected) in all_variants_with_strings() {
            assert_eq!(variant.as_str(), expected);
        }
    }

    // ── Shape: FromStr for parsing ─────────────────────────────────────
    // Needed to migrate a subtype: Option<String> field without a big-bang
    // type swap — Subtype::from_str("message.assistant.tool_use") lets
    // old string-holding code upgrade incrementally.

    #[test]
    fn from_str_parses_known_wire_strings() {
        use std::str::FromStr;
        for (expected, s) in all_variants_with_strings() {
            let got = Subtype::from_str(s).unwrap();
            assert_eq!(got, expected);
        }
    }

    #[test]
    fn from_str_rejects_typos() {
        use std::str::FromStr;
        // The whole point of the enum: a typo fails, loudly.
        let bad = Subtype::from_str("message.assitant.text");
        assert!(bad.is_err(), "typo must not parse");
    }

    // ── Shape: hierarchy predicates ────────────────────────────────────
    //
    // Today code does `subtype.starts_with("message.assistant.")` in ~40
    // places. These predicates replace that with a typed API, preserving
    // the prefix semantics without string matching.

    #[test]
    fn is_message_covers_all_message_variants() {
        assert!(Subtype::UserPrompt.is_message());
        assert!(Subtype::UserToolResult.is_message());
        assert!(Subtype::AssistantText.is_message());
        assert!(Subtype::AssistantThinking.is_message());
        assert!(Subtype::AssistantToolUse.is_message());
        assert!(!Subtype::TurnComplete.is_message());
        assert!(!Subtype::ProgressBash.is_message());
        assert!(!Subtype::FileSnapshot.is_message());
    }

    #[test]
    fn is_system_covers_runtime_lifecycle() {
        assert!(Subtype::TurnComplete.is_system());
        assert!(Subtype::SystemError.is_system());
        assert!(Subtype::SystemCompact.is_system());
        assert!(Subtype::SystemHook.is_system());
        assert!(Subtype::SessionStart.is_system());
        assert!(Subtype::ModelChange.is_system());
        assert!(!Subtype::UserPrompt.is_system());
    }

    #[test]
    fn is_progress_is_the_ephemeral_family() {
        assert!(Subtype::ProgressBash.is_progress());
        assert!(Subtype::ProgressAgent.is_progress());
        assert!(Subtype::ProgressHook.is_progress());
        assert!(!Subtype::AssistantText.is_progress());
    }

    #[test]
    fn is_assistant_targets_assistant_authored_messages() {
        // Narrower than is_message — only what the LLM produced.
        assert!(Subtype::AssistantText.is_assistant());
        assert!(Subtype::AssistantThinking.is_assistant());
        assert!(Subtype::AssistantToolUse.is_assistant());
        assert!(!Subtype::UserPrompt.is_assistant());
        assert!(!Subtype::UserToolResult.is_assistant());
    }

    #[test]
    fn is_user_targets_user_authored_messages() {
        // tool_result is "user" on the wire (it's what goes back to the
        // LLM as the user-role message). Documenting the surprise here.
        assert!(Subtype::UserPrompt.is_user());
        assert!(Subtype::UserToolResult.is_user());
        assert!(!Subtype::AssistantText.is_user());
    }

    // ── Shape: is_ephemeral ────────────────────────────────────────────
    //
    // Used by the persist consumer to decide whether to store durably.
    // Currently: `projection::is_ephemeral(subtype)` does string matching.
    // Once Subtype is adopted, that helper can take &Subtype.

    #[test]
    fn is_ephemeral_is_true_exactly_for_progress_variants() {
        for (variant, _) in all_variants_with_strings() {
            assert_eq!(
                variant.is_ephemeral(),
                variant.is_progress(),
                "ephemeral ⇔ progress.* today — {:?}",
                variant
            );
        }
    }

    // ── Shape: exhaustive family membership ────────────────────────────
    //
    // Every variant belongs to exactly one top-level family. This is how
    // we'll know we covered everything: no variant should slip through.

    #[test]
    fn every_variant_belongs_to_exactly_one_family() {
        for (variant, s) in all_variants_with_strings() {
            let in_families = [
                variant.is_message(),
                variant.is_system(),
                variant.is_progress(),
                variant.is_file(),
                variant.is_queue(),
            ]
            .iter()
            .filter(|&&b| b)
            .count();
            assert_eq!(
                in_families, 1,
                "{} ({:?}) must be in exactly one family, got {}",
                s, variant, in_families
            );
        }
    }

    // ── Shape: Display matches as_str ──────────────────────────────────

    #[test]
    fn display_matches_wire_form() {
        for (variant, expected) in all_variants_with_strings() {
            assert_eq!(format!("{}", variant), expected);
        }
    }
}
