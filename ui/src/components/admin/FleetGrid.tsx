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
    return <p className="text-sm text-[#565f89]">No nodes visible.</p>;
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
  } as const)[node.source];

  const evidenceColor = node.is_self
    ? "text-[#1a1b26]"
    : node.source === "hub-config"
      ? "text-[#bb9af7]"
      : node.source === "peer-config"
        ? "text-[#7aa2f7]"
        : "text-[#565f89]";

  const cardClass = node.is_self
    ? "bg-[#7aa2f7] text-[#1a1b26] border-[#7aa2f7]"
    : "bg-[#1a1b26] text-[#c0caf5] border-[#24283b] hover:border-[#414868]";

  return (
    <div
      className={`rounded-lg border p-3 transition-colors ${cardClass}`}
      data-testid={`node-card-${node.host}`}
    >
      <div className="flex items-baseline justify-between gap-2">
        <code
          className={`text-sm font-semibold truncate ${node.is_self ? "text-[#1a1b26]" : "text-[#c0caf5]"}`}
          title={node.host}
        >
          {node.host}
        </code>
        {node.is_self && (
          <span className="rounded bg-[#1a1b26]/20 px-1.5 py-0.5 text-[10px] uppercase tracking-wider text-[#1a1b26]">
            self
          </span>
        )}
      </div>
      <div className="mt-2 flex items-baseline justify-between gap-2 text-xs">
        <span className={evidenceColor}>{evidenceLabel}</span>
        <span className={node.is_self ? "text-[#1a1b26]" : "text-[#9ece6a]"}>
          {node.session_count > 0
            ? `${node.session_count} session${node.session_count === 1 ? "" : "s"}`
            : "no sessions yet"}
        </span>
      </div>
    </div>
  );
}
