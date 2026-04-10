// eval-apply-detector.ts — The eval-apply detector.
//
// A pure fold: (state, record) → (state, events[])
//
// No mutation. No side effects. No API calls.
// The CLI wrapper handles I/O. This is the algebra.

import {
  type ApiRecord,
  type DetectorState,
  type StructuralEvent,
  type StructuralPhase,
  type StructuralTurn,
  type SessionSummary,
  type EvalDecision,
  initialState,
} from "./types.js";

// ─────────────────────────────────────────────
// The fold: (state, record) → (state, events[])
// ─────────────────────────────────────────────

export function feed(
  state: DetectorState,
  record: ApiRecord
): [DetectorState, StructuralEvent[]] {
  const s: DetectorState = {
    ...state,
    pendingTools: [...state.pendingTools],
    currentTurnIds: [...state.currentTurnIds],
    scopeStack: [...state.scopeStack],
  };
  const events: StructuralEvent[] = [];
  const ts = record.timestamp;
  const depth = record.depth;

  // Note: the `depth` field in OpenStory records is a tree position
  // counter (monotonically increasing), NOT scope nesting depth.
  // We track scope via Agent tool calls instead.

  s.currentTurnIds.push(record.id);
  if (!s.currentTurnStart) {
    s.currentTurnStart = ts;
  }

  switch (record.record_type) {
    case "user_message": {
      const content = extractUserText(record);
      if (content && !content.startsWith("System:")) {
        s.envSize++;
        events.push(emit("human", s, ts, [record.id], {
          content: content.slice(0, 1000),
          summary: `Human: "${content.slice(0, 80)}"`,
        }));
      }
      break;
    }

    case "reasoning": {
      const content = record.payload?.summary || record.payload?.content || "";
      const tokens = estimateTokens(content);
      events.push(emit("thinking", s, ts, [record.id], {
        content: typeof content === "string" ? content.slice(0, 200) : "",
        tokens,
        summary: `Thinking (${tokens} tokens)`,
      }));
      break;
    }

    case "assistant_message": {
      s.turnNumber++;
      s.envSize++;
      const content = extractAssistantText(record);
      const hasToolUse = assistantHasToolUse(record);
      const hasText = content.length > 0;
      const decision: EvalDecision = hasToolUse && hasText
        ? "text_and_tool_use"
        : hasToolUse ? "tool_use" : "text_only";
      const tokens = estimateTokens(content);
      events.push(emit("eval", s, ts, [record.id], {
        content: content.slice(0, 1000),
        tokens,
        summary: `Turn ${s.turnNumber}: eval [${decision}] (${tokens} tokens)`,
      }));
      s.phase = "saw_eval";
      break;
    }

    case "tool_call": {
      const name = record.payload?.name ?? "unknown";
      const input = summarizeInput(record.payload?.input);
      s.pendingTools.push(name);
      // Agent tool = compound procedure = scope open
      if (name === "Agent") {
        s.scopeDepth++;
        s.scopeStack.push(s.scopeDepth);
        events.push(emit("scope_open", s, ts, [record.id], {
          toolName: name,
          toolInput: input,
          summary: `Compound procedure: nested eval-apply at depth ${s.scopeDepth}`,
        }));
      }
      events.push(emit("apply", s, ts, [record.id], {
        toolName: name,
        toolInput: input,
        summary: `Turn ${s.turnNumber}: apply (${name})`,
      }));
      s.phase = "saw_apply";
      break;
    }

    case "tool_result": {
      const output = record.payload?.output ?? "";
      const isError = record.payload?.is_error ?? false;
      const poppedTool = s.pendingTools.pop();
      s.envSize++;
      // Agent tool returning = scope close
      if (poppedTool === "Agent" && s.scopeStack.length > 0) {
        s.scopeStack.pop();
        s.scopeDepth = Math.max(0, s.scopeDepth - 1);
        events.push(emit("scope_close", s, ts, [record.id], {
          summary: `Scope closed, returning to depth ${s.scopeDepth}`,
        }));
      }
      // Attach output to the most recent apply event
      if (events.length > 0) {
        const lastApply = [...events].reverse().find(e => e.phase === "apply");
        if (lastApply) {
          lastApply.toolOutput = typeof output === "string"
            ? output.slice(0, 500) : "";
          if (isError) lastApply.summary += " [ERROR]";
        }
      }
      s.phase = "results_ready";
      break;
    }

    case "turn_end": {
      const reason = record.payload?.reason ?? "end_turn";
      const duration = record.payload?.duration_ms ?? null;
      events.push(emitTurnEnd(s, ts, reason, duration));
      s.currentTurnIds = [];
      s.currentTurnStart = "";
      s.phase = "idle";
      break;
    }

    case "token_usage": {
      // Enrich the most recent eval with actual token counts
      const input = record.payload?.input_tokens ?? 0;
      const output = record.payload?.output_tokens ?? 0;
      const total = input + output;
      if (total > 0 && events.length > 0) {
        const lastEval = [...events].reverse().find(e => e.phase === "eval");
        if (lastEval) {
          lastEval.tokens = total;
        }
      }
      break;
    }

    case "system_event": {
      const subtype = record.payload?.subtype;
      if (subtype === "system.compact") {
        events.push(emit("compact", s, ts, [record.id], {
          summary: `GC: context compaction (env was ${s.envSize} messages)`,
        }));
      }
      break;
    }
  }

  return [s, events];
}

// ─────────────────────────────────────────────
// Emission helpers
// ─────────────────────────────────────────────

function emit(
  phase: StructuralPhase,
  state: DetectorState,
  timestamp: string,
  eventIds: string[],
  extra: Partial<StructuralEvent> = {},
): StructuralEvent {
  return {
    phase,
    timestamp,
    turnNumber: state.turnNumber,
    scopeDepth: state.scopeDepth,
    envSize: state.envSize,
    eventIds,
    summary: `${phase} at turn ${state.turnNumber}`,
    ...extra,
  };
}

function emitTurnEnd(
  state: DetectorState,
  timestamp: string,
  reason: string,
  durationMs: number | null,
): StructuralEvent {
  const isTerminal = reason === "end_turn" || reason === "stop_sequence";
  return emit("turn_end", state, timestamp, [...state.currentTurnIds], {
    stopReason: reason,
    tokens: durationMs ?? undefined,
    summary: `Turn ${state.turnNumber}: ${reason} → ${isTerminal ? "TERMINATE" : "CONTINUE"} | env: ${state.envSize} messages`,
  });
}

// ─────────────────────────────────────────────
// Content extraction
// ─────────────────────────────────────────────

function extractUserText(record: ApiRecord): string {
  const content = record.payload?.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((b: any) => b?.type === "text")
      .map((b: any) => b.text ?? "")
      .join(" ");
  }
  return "";
}

function extractAssistantText(record: ApiRecord): string {
  const msg = record.payload?.message ?? record.payload;
  const content = msg?.content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((b: any) => b?.type === "text" && typeof b.text === "string")
      .map((b: any) => b.text)
      .join("\n");
  }
  return "";
}

function assistantHasToolUse(record: ApiRecord): boolean {
  const msg = record.payload?.message ?? record.payload;
  const content = msg?.content;
  if (Array.isArray(content)) {
    return content.some((b: any) => b?.type === "tool_use");
  }
  return false;
}

function summarizeInput(input: any): string {
  if (!input) return "";
  if (typeof input === "string") return input.slice(0, 100);
  if (input.command) return input.command.toString().slice(0, 100);
  if (input.file_path) return input.file_path;
  if (input.pattern) return `pattern: ${input.pattern}`;
  if (input.prompt) return input.prompt.toString().slice(0, 100);
  if (input.description) return input.description;
  if (input.query) return `query: ${input.query}`;
  if (input.url) return input.url;
  return JSON.stringify(input).slice(0, 100);
}

function estimateTokens(content: string): number {
  if (!content) return 0;
  return Math.ceil(content.length / 4);
}

// ─────────────────────────────────────────────
// Session builder: fold events into turns
// ─────────────────────────────────────────────
//
// A "turn" is anchored on turn_end records — the real coalgebra
// step boundary. Between two turn_ends, there may be multiple
// eval-apply cycles (the model responds, calls tools, sees results,
// responds again). All of that is ONE turn.
//
// Sub-agent events (depth > 0) produce their own assistant_messages
// and tool_calls but are collected as nested activity within the
// parent turn, not as separate turns.

export function buildSession(
  sessionId: string,
  label: string,
  model: string,
  records: ApiRecord[],
): SessionSummary {
  // First pass: run the detector to get structural events
  let state = initialState();
  const allEvents: StructuralEvent[] = [];

  for (const record of records) {
    const [newState, events] = feed(state, record);
    state = newState;
    allEvents.push(...events);
  }

  // Second pass: group by turn_end boundaries
  // Collect everything between turn_ends into one turn.
  const turns: StructuralTurn[] = [];
  let turnNumber = 0;
  let prevEnv = 0;
  const toolCounts: Record<string, number> = {};
  let totalThinkingTokens = 0;

  // Accumulate events for the current turn
  let humans: Array<{ content: string; timestamp: string }> = [];
  let thinkings: Array<{ summary: string; tokens: number }> = [];
  let evals: Array<{ content: string; timestamp: string; tokens: number; decision: EvalDecision }> = [];
  let applies: Array<{ toolName: string; inputSummary: string; outputSummary: string; isAgent: boolean; isError: boolean }> = [];
  let turnTimestamp = "";
  let turnScopeDepth = 0;

  for (const event of allEvents) {
    switch (event.phase) {
      case "human": {
        if (!turnTimestamp) turnTimestamp = event.timestamp;
        humans.push({
          content: event.content ?? "",
          timestamp: event.timestamp,
        });
        break;
      }

      case "thinking": {
        const tokens = event.tokens ?? 0;
        totalThinkingTokens += tokens;
        thinkings.push({
          summary: event.content ?? "",
          tokens,
        });
        break;
      }

      case "eval": {
        if (!turnTimestamp) turnTimestamp = event.timestamp;
        turnScopeDepth = event.scopeDepth;
        const hasToolUse = event.summary?.includes("tool_use") ?? false;
        const hasText = (event.content?.length ?? 0) > 0;
        const decision: EvalDecision = hasToolUse && hasText
          ? "text_and_tool_use"
          : hasToolUse ? "tool_use" : "text_only";
        evals.push({
          content: event.content ?? "",
          timestamp: event.timestamp,
          tokens: event.tokens ?? 0,
          decision,
        });
        break;
      }

      case "apply": {
        const name = event.toolName ?? "unknown";
        applies.push({
          toolName: name,
          inputSummary: event.toolInput ?? "",
          outputSummary: event.toolOutput ?? "",
          isAgent: name === "Agent",
          isError: event.summary?.includes("[ERROR]") ?? false,
        });
        toolCounts[name] = (toolCounts[name] ?? 0) + 1;
        break;
      }

      case "turn_end": {
        turnNumber++;
        const envNow = event.envSize;
        const isTerminal =
          event.stopReason === "end_turn" ||
          event.stopReason === "stop_sequence";

        // Merge multiple humans into one (use the first)
        const human = humans.length > 0 ? humans[0] : null;

        // Merge thinking blocks (sum tokens, use last summary)
        const thinking = thinkings.length > 0
          ? {
              summary: thinkings[thinkings.length - 1].summary,
              tokens: thinkings.reduce((acc, t) => acc + t.tokens, 0),
            }
          : null;

        // Use the last eval as the primary (the one that ended the turn)
        // but note how many eval cycles happened
        const lastEval = evals.length > 0 ? evals[evals.length - 1] : null;
        const evalCycles = evals.length;
        const eval_ = lastEval
          ? {
              content: evalCycles > 1
                ? `[${evalCycles} eval cycles] ${lastEval.content}`
                : lastEval.content,
              timestamp: lastEval.timestamp,
              decision: lastEval.decision,
              tokens: evals.reduce((acc, e) => acc + e.tokens, 0),
            }
          : null;

        turns.push({
          turnNumber,
          scopeDepth: turnScopeDepth,
          human,
          thinking,
          eval: eval_,
          applies,
          envSize: envNow,
          envDelta: envNow - prevEnv,
          stopReason: event.stopReason ?? "end_turn",
          isTerminal,
          timestamp: turnTimestamp || event.timestamp,
          durationMs: null,
        });

        prevEnv = envNow;
        // Reset accumulators for next turn
        humans = [];
        thinkings = [];
        evals = [];
        applies = [];
        turnTimestamp = "";
        turnScopeDepth = 0;
        break;
      }
    }
  }

  return {
    sessionId,
    label,
    model,
    turns,
    totalEvals: allEvents.filter(e => e.phase === "eval").length,
    totalApplies: allEvents.filter(e => e.phase === "apply").length,
    totalThinkingTokens,
    maxScopeDepth: Math.max(0, ...turns.map(t => t.scopeDepth)),
    compactionCount: allEvents.filter(e => e.phase === "compact").length,
    toolCounts,
    envGrowth: [turns[0]?.envSize ?? 0, state.envSize],
  };
}

// ─────────────────────────────────────────────
// CLI
// ─────────────────────────────────────────────

async function main() {
  const sessionId = process.argv[2];
  if (!sessionId) {
    const api = process.env.OPEN_STORY_API_URL ?? "http://localhost:3002";
    const resp = await fetch(`${api}/api/sessions`);
    const { sessions } = await resp.json() as { sessions: any[]; total: number };
    console.log("\nAvailable sessions:\n");
    for (const s of sessions.slice(0, 15)) {
      const label = (s.label ?? s.first_prompt ?? "").slice(0, 60);
      console.log(`  ${s.session_id}  [${s.project_name}] ${label}`);
    }
    console.log("\nUsage: npx tsx eval-apply-detector.ts <session-id> [--html]");
    process.exit(0);
  }

  const api = process.env.OPEN_STORY_API_URL ?? "http://localhost:3002";

  const sessionsResp = await fetch(`${api}/api/sessions`);
  const { sessions } = await sessionsResp.json() as { sessions: any[]; total: number };
  const sessionInfo = sessions.find((s: any) => s.session_id === sessionId);

  const resp = await fetch(`${api}/api/sessions/${sessionId}/records`);
  const records = await resp.json() as ApiRecord[];

  const session = buildSession(
    sessionId,
    sessionInfo?.label ?? sessionInfo?.first_prompt ?? "",
    sessionInfo?.model ?? "unknown",
    records,
  );

  const useHtml = process.argv.includes("--html");
  if (useHtml) {
    const { renderHtml } = await import("./render-html.js");
    const fs = await import("fs");
    const outPath = `eval-apply-${sessionId.slice(0, 8)}.html`;
    fs.writeFileSync(outPath, renderHtml(session));
    console.log(`Written to ${outPath}`);
    const { execSync } = await import("child_process");
    execSync(`open ${outPath}`);
  } else {
    const { render } = await import("./render.js");
    console.log(render(session));
  }
}

const isMain = process.argv[1]?.includes("eval-apply-detector");
if (isMain) {
  main().catch(e => {
    console.error(e);
    process.exit(1);
  });
}
