/** Pure model for the Agent×Project adjacency matrix (Lab candidate
 *  `agent-project-matrix`). Rows = agents, columns = projects, cell = how much
 *  work that agent did in that project. Projects are long-tailed (~310), so the
 *  top-N by event volume are kept and the rest folded into an "other" column.
 *  Side-effect-free → unit-tested; no records needed (sessions list only). */

interface APSession {
  readonly origin_agent?: string | null;
  readonly project_name?: string | null;
  readonly project_id?: string | null;
  readonly event_count?: number | null;
}

export interface APCell {
  readonly agent: string;
  readonly project: string;
  readonly sessions: number;
  readonly events: number;
}

export interface APMatrix {
  readonly agents: string[];   // rows, by session volume desc
  readonly projects: string[]; // cols (top-N by events) + "other" if folded
  readonly cells: APCell[];
  readonly maxEvents: number;
  readonly maxSessions: number;
}

const OTHER = "other";

export function buildAgentProjectMatrix(
  sessions: readonly APSession[],
  { topProjects = 16 }: { topProjects?: number } = {},
): APMatrix {
  const agentOf = (s: APSession) => s.origin_agent || "unknown";
  const projOf = (s: APSession) => s.project_name || s.project_id || "unknown";

  // pass 1: rank agents (by session count) and projects (by events)
  const agentVol = new Map<string, number>();
  const projVol = new Map<string, number>();
  for (const s of sessions) {
    agentVol.set(agentOf(s), (agentVol.get(agentOf(s)) ?? 0) + 1);
    projVol.set(projOf(s), (projVol.get(projOf(s)) ?? 0) + (s.event_count ?? 0));
  }
  const agents = [...agentVol.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map((e) => e[0]);
  const topProj = [...projVol.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, topProjects).map((e) => e[0]);
  const keep = new Set(topProj);
  const folded = projVol.size > topProj.length;
  const projects = folded ? [...topProj, OTHER] : [...topProj];

  // pass 2: aggregate into (agent, column) cells, folding non-top → "other"
  const cellMap = new Map<string, Map<string, { sessions: number; events: number }>>();
  for (const s of sessions) {
    const a = agentOf(s);
    const col = keep.has(projOf(s)) ? projOf(s) : OTHER;
    const row = cellMap.get(a) ?? new Map();
    const cur = row.get(col) ?? { sessions: 0, events: 0 };
    cur.sessions += 1;
    cur.events += s.event_count ?? 0;
    row.set(col, cur);
    cellMap.set(a, row);
  }

  const cells: APCell[] = [];
  let maxEvents = 0, maxSessions = 0;
  for (const [agent, row] of cellMap) {
    for (const [project, v] of row) {
      cells.push({ agent, project, sessions: v.sessions, events: v.events });
      if (v.events > maxEvents) maxEvents = v.events;
      if (v.sessions > maxSessions) maxSessions = v.sessions;
    }
  }
  return { agents, projects, cells, maxEvents, maxSessions };
}
