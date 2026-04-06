/**
 * Domain fact extraction from ToolOutcome data.
 *
 * Pure functions: ToolOutcome → DomainFact.
 * Same outcome, same fact. Always.
 *
 * Layer 4 of the five-layer model — the deterministic "what changed"
 * derived from tool calls. No heuristics, no interpretation.
 */

export type FactKind =
  | "created"
  | "modified"
  | "read"
  | "command_ok"
  | "command_fail"
  | "search"
  | "agent"
  | "error";

export interface DomainFact {
  kind: FactKind;
  label: string;    // short display: "eval_apply.rs" or "cargo test"
  detail: string;   // full info: "/Users/user/.../eval_apply.rs"
}

interface ToolOutcome {
  type: string;
  path?: string;
  command?: string;
  succeeded?: boolean;
  pattern?: string;
  source?: string;
  description?: string;
  reason?: string;
}

/** Extract a displayable fact from one ToolOutcome. */
export function extractDomainFact(outcome: ToolOutcome): DomainFact {
  switch (outcome.type) {
    case "FileCreated":
      return {
        kind: "created",
        label: shortFilename(outcome.path ?? ""),
        detail: outcome.path ?? "",
      };

    case "FileModified":
      return {
        kind: "modified",
        label: shortFilename(outcome.path ?? ""),
        detail: outcome.path ?? "",
      };

    case "FileRead":
      return {
        kind: "read",
        label: shortFilename(outcome.path ?? ""),
        detail: outcome.path ?? "",
      };

    case "CommandExecuted":
      return {
        kind: outcome.succeeded ? "command_ok" : "command_fail",
        label: truncate(outcome.command ?? "", 40),
        detail: outcome.command ?? "",
      };

    case "SearchPerformed":
      return {
        kind: "search",
        label: truncate(outcome.pattern ?? "", 40),
        detail: `${outcome.source ?? "?"}: ${outcome.pattern ?? ""}`,
      };

    case "SubAgentSpawned":
      return {
        kind: "agent",
        label: truncate(outcome.description ?? "", 40),
        detail: outcome.description ?? "",
      };

    case "FileWriteFailed":
      return {
        kind: "error",
        label: `${shortFilename(outcome.path ?? "")} (write failed)`,
        detail: outcome.reason ?? "",
      };

    case "FileReadFailed":
      return {
        kind: "error",
        label: `${shortFilename(outcome.path ?? "")} (read failed)`,
        detail: outcome.reason ?? "",
      };

    default:
      return {
        kind: "command_ok",
        label: outcome.type,
        detail: JSON.stringify(outcome),
      };
  }
}

interface ApplyWithOutcome {
  tool_outcome?: ToolOutcome | null;
}

/** Extract domain facts from a list of applies, deduplicated and ordered.
 *  Order: created → modified → read → commands → searches → agents → errors.
 */
export function extractDomainFacts(applies: ApplyWithOutcome[]): DomainFact[] {
  const seen = new Set<string>();
  const facts: DomainFact[] = [];

  for (const apply of applies) {
    if (!apply.tool_outcome) continue;
    const fact = extractDomainFact(apply.tool_outcome);
    const key = `${fact.kind}:${fact.label}`;
    if (seen.has(key)) continue;
    seen.add(key);
    facts.push(fact);
  }

  // Sort by kind priority
  const priority: Record<FactKind, number> = {
    created: 0,
    modified: 1,
    read: 2,
    command_ok: 3,
    command_fail: 4,
    search: 5,
    agent: 6,
    error: 7,
  };

  facts.sort((a, b) => priority[a.kind] - priority[b.kind]);
  return facts;
}

function shortFilename(path: string): string {
  if (!path) return "";
  const parts = path.split("/");
  return parts[parts.length - 1] ?? path;
}

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  return s.slice(0, max - 3) + "...";
}
