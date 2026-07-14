/**
 * FleetGrid — every node visible from this device's vantage.
 *
 * Renders one card per host discovered through any of three channels:
 *   - this device itself (`self-node`)
 *   - sessions whose `host` column equals that host (`sessions`)
 *   - configured `OPEN_STORY_PEER_DOMAINS` (`peer-config`, T1 mesh)
 *   - configured `OPEN_STORY_HUB_DOMAIN` / `_PEER_HUB_DOMAINS` (`hub-config`)
 *
 * The cards together are the "whole topology" view — the shape SVG below
 * shows *connectivity*, this grid shows *presence*. Self is highlighted.
 */

import type { NodeSummary } from "@/lib/admin-api";

interface Props {
  nodes: readonly NodeSummary[];
}

export function FleetGrid({ nodes }: Props) {
  if (nodes.length === 0) {
    return <p className="text-sm text-[color:var(--text-muted)]">No nodes visible.</p>;
  }
  return (
    <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-3" data-testid="fleet-grid">
      {nodes.map((n) => (
        <NodeCard key={n.host} node={n} />
      ))}
    </div>
  );
}

function NodeCard({ node }: { node: NodeSummary }) {
  const evidenceLabel = ({
    "self-node": "this device",
    sessions: "seen in sessions",
    "peer-config": "configured peer",
    "hub-config": "configured hub",
    "nats-leafnode-hub": "NATS upstream hub",
  } as const)[node.source];

  const evidenceColor = node.is_self
    ? "text-[color:var(--bg)]"
    : node.source === "hub-config" || node.source === "nats-leafnode-hub"
      ? "text-[color:var(--purple)]"
      : node.source === "peer-config"
        ? "text-[color:var(--accent)]"
        : "text-[color:var(--text-muted)]";

  const cardClass = node.is_self
    ? "bg-[color:var(--accent)] text-[color:var(--bg)] border-[color:var(--accent)]"
    : "bg-[color:var(--bg)] text-[color:var(--text)] border-[color:var(--bg-surface)] hover:border-[#414868]";

  return (
    <div
      className={`rounded-lg border p-3 transition-colors ${cardClass}`}
      data-testid={`node-card-${node.host}`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <code
          className={`text-sm font-semibold truncate ${node.is_self ? "text-[color:var(--bg)]" : "text-[color:var(--text)]"}`}
          title={node.host}
        >
          {node.host}
        </code>
        {node.is_self && (
          <span className="rounded bg-[color:var(--bg)]/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-[color:var(--bg)]">
            self
          </span>
        )}
      </div>
      <div className="mt-2 flex items-baseline justify-between gap-2 text-xs">
        <span className={evidenceColor}>{evidenceLabel}</span>
        <span className={node.is_self ? "text-[color:var(--bg)]" : "text-[color:var(--green)]"}>
          {node.session_count > 0
            ? `${node.session_count} session${node.session_count === 1 ? "" : "s"}`
            : "no sessions yet"}
        </span>
      </div>
    </div>
  );
}
