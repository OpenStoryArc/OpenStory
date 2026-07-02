/** Pure model for the Delegation Graph (Lab candidate `delegation-graph`, the
 *  flagship). Resolves parent→subagent spawn links from records: when a parent
 *  session launches a subagent, its `tool_result` echoes `agentId: <hex>` and
 *  the child session is exactly `agent-<hex>`. We extract those hexes and
 *  cross-validate against real sessions (so code/docs merely mentioning
 *  "agentId" don't create phantom edges). Side-effect-free → unit-tested. */

export interface DelRecord {
  readonly record_type?: string;
  readonly payload?: unknown;
}
export interface DelSession {
  readonly session_id: string;
  readonly event_count?: number | null;
  readonly status?: string | null;
  readonly label?: string | null;
}
export interface DelNode {
  readonly id: string;
  readonly label: string;
  readonly events: number;
  readonly status: string;
  readonly isSub: boolean;
}
export interface DelLink {
  readonly source: string;
  readonly target: string; // parent → child
}
export interface DelGraph {
  readonly nodes: DelNode[];
  readonly links: DelLink[];
  readonly resolvedSubs: number;
  readonly totalSubs: number;
}

const AGENT_ID_RE = /agentId:\s*([0-9a-f]{8,})/gi;

/** Child agentIds echoed in a session's tool_result records (the spawn signal). */
export function extractChildAgentIds(records: readonly DelRecord[]): string[] {
  const ids = new Set<string>();
  for (const r of records) {
    if (r.record_type !== "tool_result") continue;
    const s = JSON.stringify(r.payload ?? "");
    AGENT_ID_RE.lastIndex = 0;
    let m: RegExpExecArray | null;
    while ((m = AGENT_ID_RE.exec(s)) !== null) ids.add(m[1]!.toLowerCase());
  }
  return [...ids];
}

/** Assemble the delegation graph. `recordsBySession` holds records for the
 *  (sampled) potential parent sessions. Links only form when the echoed
 *  `agent-<hex>` is a real session. */
export function buildDelegationGraph(
  sessions: readonly DelSession[],
  recordsBySession: Record<string, readonly DelRecord[]>,
): DelGraph {
  const byId = new Map(sessions.map((s) => [s.session_id, s]));
  const totalSubs = sessions.filter((s) => s.session_id.startsWith("agent-")).length;

  const childToParent = new Map<string, string>();
  for (const [parentId, records] of Object.entries(recordsBySession)) {
    for (const hex of extractChildAgentIds(records)) {
      const child = `agent-${hex}`;
      if (byId.has(child) && !childToParent.has(child)) childToParent.set(child, parentId);
    }
  }

  const links: DelLink[] = [...childToParent.entries()].map(([child, parent]) => ({ source: parent, target: child }));
  const involved = new Set<string>();
  for (const l of links) { involved.add(l.source); involved.add(l.target); }

  const node = (id: string): DelNode => {
    const s = byId.get(id);
    return {
      id,
      label: (s?.label || id).slice(0, 40),
      events: s?.event_count ?? 0,
      status: s?.status ?? "unknown",
      isSub: id.startsWith("agent-"),
    };
  };
  const nodes = [...involved].map(node);
  return { nodes, links, resolvedSubs: childToParent.size, totalSubs };
}
