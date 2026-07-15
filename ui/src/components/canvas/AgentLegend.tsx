/** AgentLegend — a compact color key for views that encode agent by color
 *  (Scatter, Gantt). Shows only the agents actually present, in a stable order
 *  (most frequent first), so the color encoding is readable. */

import { agentColor } from "@/lib/agent-color";

/** Distinct agents present, ordered by frequency desc then name — pure, so the
 *  legend order is deterministic and testable. */
export function agentsPresent(agents: readonly string[]): string[] {
  const counts = new Map<string, number>();
  for (const a of agents) {
    const key = a || "unknown";
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return [...counts.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).map((e) => e[0]);
}

export function AgentLegend({ agents, className = "" }: { agents: readonly string[]; className?: string }) {
  const present = agentsPresent(agents);
  if (present.length === 0) return null;
  return (
    <div className={`flex flex-wrap items-center gap-x-3 gap-y-1 ${className}`} data-testid="agent-legend">
      {present.map((a) => (
        <span key={a} className="flex items-center gap-1 text-[10px] text-[color:var(--text-bright)]">
          <span className="h-2 w-2 rounded-full" style={{ background: agentColor(a) }} />
          {a}
        </span>
      ))}
    </div>
  );
}
