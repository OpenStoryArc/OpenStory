// domain.ts — Domain events: the facts layer.
//
// Domain events are DETERMINISTIC. No heuristics. No interpretation.
//
//   Write + "created successfully" → FileCreated. Always.
//   Edit → FileModified. Always.
//   Read → FileRead. Always.
//   Bash → CommandExecuted. Always.
//
// The mapping from tool calls to domain events is a pure function.
// Same input → same output. This is the honest contract between
// the structural layer (eval-apply) and the language layer (sentences).
//
// Interpretation (was this Read preparatory? was this Bash a test?)
// belongs in the sentence layer above, NOT here. This layer is facts.

import type { StructuralTurn } from "./types.js";

// ─────────────────────────────────────────────
// Domain event types — the fact vocabulary
// ─────────────────────────────────────────────

export type DomainEvent =
  | { type: "FileCreated"; path: string }
  | { type: "FileModified"; path: string }
  | { type: "FileRead"; path: string }
  | { type: "FileWriteFailed"; path: string; reason: string }
  | { type: "FileReadFailed"; path: string; reason: string }
  | { type: "SearchPerformed"; pattern: string; source: string }
  | { type: "CommandExecuted"; command: string; succeeded: boolean }
  | { type: "SubAgentSpawned"; description: string }
  | { type: "ResponseDelivered"; tokens: number };

// ─────────────────────────────────────────────
// Domain turn — actors, commands, events, aggregate
// ─────────────────────────────────────────────

export interface DomainTurn {
  // Who started this turn?
  initiator: "human" | "claude";

  // What was requested? (human's words, or "continuing")
  command: string;

  // What happened? (deterministic facts)
  events: DomainEvent[];

  // How did the aggregate change?
  aggregate: AggregateChange;
}

export interface AggregateChange {
  filesCreated: string[];
  filesModified: string[];
  filesRead: string[];
  commandsRun: string[];
  commandsSucceeded: number;
  commandsFailed: number;
  searchesPerformed: number;
  subAgentsSpawned: number;
}

// ─────────────────────────────────────────────
// Tool call → domain event (deterministic)
// ─────────────────────────────────────────────

export function toDomainEvent(
  toolName: string,
  input: string,
  output: string,
  isError: boolean = false,
): DomainEvent {
  switch (toolName) {
    case "Write": {
      if (isError) {
        return { type: "FileWriteFailed", path: input, reason: output };
      }
      if (output.includes("created successfully")) {
        return { type: "FileCreated", path: input };
      }
      return { type: "FileModified", path: input };
    }

    case "Edit": {
      if (isError) {
        return { type: "FileWriteFailed", path: input, reason: output };
      }
      return { type: "FileModified", path: input };
    }

    case "Read": {
      if (isError) {
        return { type: "FileReadFailed", path: input, reason: output };
      }
      return { type: "FileRead", path: input };
    }

    case "Grep":
    case "Glob": {
      return { type: "SearchPerformed", pattern: input, source: "filesystem" };
    }

    case "WebSearch":
    case "WebFetch": {
      return { type: "SearchPerformed", pattern: input, source: "web" };
    }

    case "Bash": {
      return {
        type: "CommandExecuted",
        command: input,
        succeeded: !isError,
      };
    }

    case "Agent": {
      return { type: "SubAgentSpawned", description: input };
    }

    default: {
      return { type: "CommandExecuted", command: `${toolName}: ${input}`, succeeded: !isError };
    }
  }
}

// ─────────────────────────────────────────────
// Build domain turn from structural turn
// ─────────────────────────────────────────────

export function buildDomainTurn(turn: StructuralTurn): DomainTurn {
  // Initiator
  const initiator = turn.human ? "human" as const : "claude" as const;
  const command = turn.human?.content ?? "continuing";

  // Map each apply to a domain event
  const events: DomainEvent[] = turn.applies.map(apply =>
    toDomainEvent(apply.toolName, apply.inputSummary, apply.outputSummary, apply.isError)
  );

  // If there's an eval with no tools, it's a response delivery
  if (events.length === 0 && turn.eval) {
    events.push({ type: "ResponseDelivered", tokens: turn.eval.tokens });
  }

  // Build aggregate change
  const aggregate = buildAggregate(events);

  return { initiator, command, events, aggregate };
}

// ─────────────────────────────────────────────
// Aggregate change — derived from events (deterministic)
// ─────────────────────────────────────────────

function buildAggregate(events: DomainEvent[]): AggregateChange {
  const agg: AggregateChange = {
    filesCreated: [],
    filesModified: [],
    filesRead: [],
    commandsRun: [],
    commandsSucceeded: 0,
    commandsFailed: 0,
    searchesPerformed: 0,
    subAgentsSpawned: 0,
  };

  for (const event of events) {
    switch (event.type) {
      case "FileCreated":
        agg.filesCreated.push(extractFilename(event.path));
        break;
      case "FileModified":
        agg.filesModified.push(extractFilename(event.path));
        break;
      case "FileRead":
        agg.filesRead.push(extractFilename(event.path));
        break;
      case "CommandExecuted":
        agg.commandsRun.push(event.command);
        if (event.succeeded) agg.commandsSucceeded++;
        else agg.commandsFailed++;
        break;
      case "SearchPerformed":
        agg.searchesPerformed++;
        break;
      case "SubAgentSpawned":
        agg.subAgentsSpawned++;
        break;
    }
  }

  return agg;
}

// ─────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────

function extractFilename(path: string): string {
  if (!path) return "";
  const parts = path.split("/");
  return parts[parts.length - 1] || path;
}
