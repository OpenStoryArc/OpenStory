/** Reconnect a session to the subagents it spawned.
 *
 *  Subagents are stored as separate `agent-<agentId>` sessions with no parent
 *  link in the data model — so a session's report never showed them. But the
 *  link is recoverable: the parent's `Agent` tool_call describes the delegation,
 *  and its tool_result echoes `agentId: <hex>`, which is exactly the child
 *  session id (`agent-<hex>`). This pure extractor rebuilds that edge.
 */

import type { WireRecord } from "@/types/wire-record";
import type { AgentInput, ToolCall, ToolResult } from "@/types/view-record";

export interface Subagent {
  readonly callId: string;
  readonly description: string;
  readonly subagentType: string | null;
  /** Recovered from the tool_result (`agentId: <hex>`), else null. */
  readonly agentId: string | null;
  /** The child session id (`agent-<agentId>`), else null if unlinked. */
  readonly sessionId: string | null;
  readonly isError: boolean;
}

/** True for the separate sessions that ARE subagents (vs. main sessions). */
export function isSubagentSession(sessionId: string): boolean {
  return sessionId.startsWith("agent-");
}

function isAgentCall(r: WireRecord): boolean {
  if (r.record_type !== "tool_call") return false;
  const p = r.payload as ToolCall;
  return p?.name === "Agent" || (p?.typed_input as { tool?: string })?.tool === "agent";
}

function firstLine(s: string): string {
  return s.split("\n").map((l) => l.trim()).find((l) => l.length > 0) ?? "";
}

/** Extract the child agentId from an Agent tool_result's output text. */
function agentIdFromResult(result: WireRecord | undefined): string | null {
  if (!result) return null;
  const output = (result.payload as ToolResult)?.output;
  const text = typeof output === "string" ? output : JSON.stringify(output ?? "");
  const m = /agentId:\s*([0-9a-fA-F]{6,})/.exec(text);
  return m ? m[1]! : null;
}

export function extractSubagents(records: readonly WireRecord[]): Subagent[] {
  const resultByCall = new Map<string, WireRecord>();
  for (const r of records) {
    if (r.record_type !== "tool_result") continue;
    const cid = (r.payload as ToolResult)?.call_id;
    if (cid && !resultByCall.has(cid)) resultByCall.set(cid, r);
  }

  const subs: Subagent[] = [];
  for (const r of records) {
    if (!isAgentCall(r)) continue;
    const call = r.payload as ToolCall;
    const input = call.typed_input as AgentInput | undefined;
    const callId = call.call_id;
    if (!callId) continue;

    const result = resultByCall.get(callId);
    const agentId = agentIdFromResult(result);
    const description = (input?.description || firstLine(input?.prompt ?? "") || "subagent").trim();

    subs.push({
      callId,
      description,
      subagentType: input?.subagent_type ?? null,
      agentId,
      sessionId: agentId ? `agent-${agentId}` : null,
      isError: Boolean(result && (result.payload as ToolResult)?.is_error),
    });
  }
  return subs;
}
