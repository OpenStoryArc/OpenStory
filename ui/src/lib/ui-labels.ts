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

export const PATTERN_LABELS: Record<string, string> = {
  "test.cycle": "test",
  "git.workflow": "git",
  "error.recovery": "recovery",
  "agent.delegation": "agent",
  "turn.phase": "phase",
};

// ═══════════════════════════════════════════════════════════════════
// Pattern tooltips — describe what each pattern detected
// ═══════════════════════════════════════════════════════════════════

export const PATTERN_TOOLTIPS: Record<string, string> = {
  "test.cycle": "Test cycle: a sequence of test runs with pass/fail results",
  "git.workflow": "Git workflow: a sequence of git commands (add, commit, push, etc.)",
  "error.recovery": "Error recovery: the agent hit an error and corrected its approach",
  "agent.delegation": "Agent delegation: work was delegated to a subagent",
  "turn.phase": "Turn phase: the kind of work happening (conversation, implementation, testing, etc.)",
};
