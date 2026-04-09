/** Display labels and tooltips for UI elements.
 *
 * Separates user-facing text from internal filter keys.
 * Filter keys ("narrative", "deep") are shared with the Rust server
 * for filter_deltas — we don't rename those. We only rename what users see. */

// ═══════════════════════════════════════════════════════════════════
// Filter labels — map internal key → user-facing display text
// ═══════════════════════════════════════════════════════════════════

export const FILTER_LABELS: Record<string, string> = {
  all: "All",
  conversation: "Conversation",
  code: "Code",
  commands: "Commands",
  tests: "Tests",
  git: "Git",
  errors: "Errors",
  thinking: "Thinking",
  plans: "Plans",
  agents: "Agents",
};

// ═══════════════════════════════════════════════════════════════════
// Filter tooltips — describe what each filter matches
// ═══════════════════════════════════════════════════════════════════

export const FILTER_TOOLTIPS: Record<string, string> = {
  all: "Show all events",
  conversation: "User prompts and assistant responses",
  code: "File reads, edits, writes, and searches",
  commands: "Shell commands (bash)",
  tests: "Test runs and their pass/fail results",
  git: "Git operations (commit, push, diff, etc.)",
  errors: "Errors, compile failures, and failed tool results",
  thinking: "Extended reasoning (chain-of-thought)",
  plans: "Plan mode events (enter/exit plan mode)",
  agents: "Delegated work from subagents",
};

// ═══════════════════════════════════════════════════════════════════
// Pattern labels — map pattern type → user-facing display text
// ═══════════════════════════════════════════════════════════════════
//
// Pre-cleanup, this file held labels and tooltips for the legacy
// `test.cycle` / `git.workflow` / `error.recovery` / `agent.delegation` /
// `turn.phase` pattern types. All five were retired in
// chore/cut-legacy-detectors. The maps remain as the dispatch surface
// for any current/future named pattern type that wants a friendlier
// display label — the consumers (Timeline.tsx, etc.) fall back to the
// raw pattern_type string when no entry exists, so this empty default
// is safe.

export const PATTERN_LABELS: Record<string, string> = {};

// ═══════════════════════════════════════════════════════════════════
// Pattern tooltips — describe what each pattern detected
// ═══════════════════════════════════════════════════════════════════

export const PATTERN_TOOLTIPS: Record<string, string> = {};
