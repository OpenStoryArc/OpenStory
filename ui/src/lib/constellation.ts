/** Pure model for the agent-constellation graph — a session and the subagents
 *  it delegated to, laid out radially for an interactive pan-zoom canvas.
 *
 *  Uses the recovered parent→child linkage (`extractSubagents`) for the edges
 *  and the already-loaded sessions list for each child's stats, so the whole
 *  graph is built from data in hand — no per-subagent fetches. Deterministic
 *  radial layout (no randomness) keeps it unit-testable. */

import type { WireRecord } from "@/types/wire-record";
import type { StorySession } from "@/lib/story-api";
import { extractSubagents } from "@/lib/subagents";

export interface ConstellationNode {
  readonly id: string;
  readonly label: string;
  readonly kind: "root" | "subagent";
  readonly events: number;
  readonly tokens: number;
  readonly status: string;
  readonly subagentType: string | null;
  readonly isError: boolean;
  /** Whether the child session was found in the sessions list. */
  readonly linked: boolean;
  /** Normalized layout coords in [0,1]; the view scales to px. */
  readonly x: number;
  readonly y: number;
}

export interface ConstellationEdge {
  readonly from: string;
  readonly to: string;
}

export interface Constellation {
  readonly nodes: ConstellationNode[];
  readonly edges: ConstellationEdge[];
}

const CENTER = 0.5;
const RADIUS = 0.34;

function sessionTokens(s: StorySession | undefined): number {
  return (s?.total_input_tokens ?? 0) + (s?.total_output_tokens ?? 0);
}

export function buildConstellation(
  rootId: string,
  records: readonly WireRecord[],
  sessionsById: ReadonlyMap<string, StorySession>,
): Constellation {
  const rootSession = sessionsById.get(rootId);
  const nodes: ConstellationNode[] = [
    {
      id: rootId,
      label: rootSession?.label || rootId.slice(0, 8),
      kind: "root",
      events: rootSession?.event_count ?? records.length,
      tokens: sessionTokens(rootSession),
      status: rootSession?.status ?? "completed",
      subagentType: null,
      isError: false,
      linked: Boolean(rootSession),
      x: CENTER,
      y: CENTER,
    },
  ];
  const edges: ConstellationEdge[] = [];

  const subs = extractSubagents(records);
  const n = subs.length;
  subs.forEach((sub, i) => {
    const childId = sub.sessionId ?? `unlinked-${sub.callId}`;
    const child = sub.sessionId ? sessionsById.get(sub.sessionId) : undefined;
    // Even radial distribution, starting at the top and going clockwise.
    const angle = -Math.PI / 2 + (2 * Math.PI * i) / Math.max(n, 1);
    nodes.push({
      id: childId,
      label: sub.description,
      kind: "subagent",
      events: child?.event_count ?? 0,
      tokens: sessionTokens(child),
      status: child?.status ?? "unknown",
      subagentType: sub.subagentType,
      isError: sub.isError,
      linked: Boolean(child),
      x: CENTER + RADIUS * Math.cos(angle),
      y: CENTER + RADIUS * Math.sin(angle),
    });
    edges.push({ from: rootId, to: childId });
  });

  return { nodes, edges };
}
