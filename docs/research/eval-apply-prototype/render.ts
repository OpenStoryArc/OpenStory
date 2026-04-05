// render.ts — Pure function: SessionSummary → string
//
// Renders the structural view of a session.
// No side effects. Same input → same output.

import type { SessionSummary, StructuralTurn } from "./types.js";

export function render(session: SessionSummary): string {
  const lines: string[] = [];

  // Header
  lines.push(`\n${"═".repeat(60)}`);
  lines.push(`  Session: ${session.sessionId.slice(0, 12)}...`);
  lines.push(`  "${session.label.slice(0, 70)}"`);
  lines.push(`  Model: ${session.model} | ${session.turns.length} turns | ${session.totalApplies} tool calls`);
  lines.push(`${"═".repeat(60)}\n`);

  // Turns
  for (const turn of session.turns) {
    lines.push(renderTurn(turn));
  }

  // Summary
  lines.push(`${"═".repeat(60)}`);
  lines.push("  Summary");
  lines.push(`${"═".repeat(60)}`);

  const continued = session.turns.filter(t => !t.isTerminal).length;
  const terminated = session.turns.filter(t => t.isTerminal).length;
  lines.push(`  Turns: ${session.turns.length} (${continued} continued, ${terminated} terminated)`);
  lines.push(`  Total evals: ${session.totalEvals}`);
  lines.push(`  Total applies: ${session.totalApplies}`);

  // Tool breakdown
  const sorted = Object.entries(session.toolCounts)
    .sort(([, a], [, b]) => b - a);
  if (sorted.length > 0) {
    const toolStr = sorted.map(([name, count]) => `${name}(${count})`).join(", ");
    lines.push(`  Tools: ${toolStr}`);
  }

  lines.push(`  Max scope depth: ${session.maxScopeDepth}`);
  lines.push(`  Compactions: ${session.compactionCount}`);
  lines.push(`  Env growth: ${session.envGrowth[0]} → ${session.envGrowth[1]} messages`);
  lines.push("");

  return lines.join("\n");
}

function renderTurn(turn: StructuralTurn): string {
  const indent = "  ".repeat(turn.scopeDepth);
  const lines: string[] = [];
  const step = `coalgebra step ${turn.turnNumber}`;
  const arrow = turn.isTerminal ? "→ TERMINATE" : "→ CONTINUE";
  const reason = turn.stopReason ? ` (${turn.stopReason})` : "";

  lines.push(`${indent}┌ Turn ${turn.turnNumber} ${"─".repeat(Math.max(1, 40 - turn.turnNumber.toString().length))} ${step}`);

  // Eval
  if (turn.eval) {
    const content = turn.eval.content.slice(0, 80);
    const display = content.includes("\n") ? content.split("\n")[0] : content;
    lines.push(`${indent}│ EVAL: "${display}${display.length >= 80 ? "..." : ""}"`);
  }

  // Applies
  for (const apply of turn.applies) {
    const input = apply.inputSummary ? ` (${apply.inputSummary.slice(0, 50)})` : "";
    const prefix = apply.isAgent ? "APPLY [compound]: " : "APPLY: ";
    lines.push(`${indent}│ ${prefix}${apply.toolName}${input}`);

    if (apply.isAgent) {
      lines.push(`${indent}│   ┌ nested scope ─── compound procedure`);
      lines.push(`${indent}│   └ (sub-agent events follow at depth +1)`);
    }
  }

  // Footer
  lines.push(`${indent}│ Environment: ${turn.envSize} messages`);
  lines.push(`${indent}│ ${arrow}${reason}`);
  lines.push(`${indent}└${"─".repeat(58)}\n`);

  return lines.join("\n");
}
