/**
 * TopologyMap — hand-rolled SVG of the federation shape (T1/T2/T3/Solo).
 *
 * Geometry lifted from docs/research/federation-topology-viz.html so the
 * static doc and the live view share a visual language. v0 renders:
 *
 *   - Solo: a single node card.
 *   - T1: ring of self + peers, bidirectional source arrows.
 *   - T2: hub center, self + (placeholder) leaves around it; source-up
 *         arrows and mirror-down dashed lines.
 *   - T3: 2 hubs sourcing each other + self attached to its hub.
 *
 * Self is always highlighted (filled blue). v0 doesn't enumerate the
 * other devices on the fleet — that comes when we wire cross-domain
 * stream-info introspection in a follow-up. For now T1 shows the
 * configured `peer_domains` and T2/T3 show the hub + this node.
 */

import type { NodeSummary, Topology } from "@/lib/admin-api";

interface Props {
  topology: Topology;
}

export function TopologyMap({ topology }: Props) {
  switch (topology.shape) {
    case "solo":
    default: {
      // A `solo` node with evidence of other hosts (a detected leafnode hub,
      // or hosts seen in stored sessions) isn't really alone — draw the fleet
      // from that evidence instead of a lone self card. Falls back to SoloMap
      // only when this is genuinely the only node we know of.
      const others = topology.nodes.filter((n) => !n.is_self);
      return others.length > 0 ? (
        <FleetShape self={topology.self.host} nodes={topology.nodes} />
      ) : (
        <SoloMap host={topology.self.host} />
      );
    }
    case "t1":
      return (
        <T1Map host={topology.self.host} peers={topology.self.peer_domains} />
      );
    case "t2":
      return (
        <T2Map
          host={topology.self.host}
          hubDomain={topology.self.hub_domain ?? "hub"}
          isHub={topology.self.role === "hub"}
        />
      );
    case "t3":
      return (
        <T3Map
          host={topology.self.host}
          hubDomain={topology.self.hub_domain ?? "hub-a"}
          peerHubs={topology.self.peer_hub_domains}
          isHub={topology.self.role === "hub"}
        />
      );
  }
}

// ── Solo ─────────────────────────────────────────────────────────────

function SoloMap({ host }: { host: string }) {
  return (
    <svg viewBox="0 0 600 200" role="img" className="block mx-auto max-w-full h-auto">
      <NodeRect x={220} y={70} w={160} h={60} label={host} subject="events.>" highlight />
    </svg>
  );
}

// ── Fleet (evidence-inferred) ────────────────────────────────────────
//
// Drawn when this node reports `solo` but we have evidence of other hosts:
// a detected leafnode hub (live NATS leaf) and/or hosts seen in stored
// sessions. Self → hub is solid (the live leaf we can see); other hosts →
// hub are dashed ("seen via the hub", inferred from their sessions).

function FleetShape({ self, nodes }: { self: string; nodes: readonly NodeSummary[] }) {
  const isHub = (n: NodeSummary) =>
    n.source === "nats-leafnode-hub" || n.source === "hub-config";
  const hub = nodes.find(isHub);
  const devices = nodes.filter((n) => !isHub(n));

  const W = 760;
  const H = 360;
  const deviceY = 230;
  const dw = 150;
  const hubCy = 70;
  const hubRy = 44;
  const n = devices.length;
  const xs = devices.map((_, i) => {
    if (n <= 1) return W / 2;
    const span = W - 220;
    return 110 + (span / (n - 1)) * i;
  });

  return (
    <svg viewBox={`0 0 ${W} ${H}`} role="img" className="block mx-auto max-w-full h-auto">
      <defs>
        <marker id="fleetarrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill="#9ece6a" />
        </marker>
      </defs>
      {hub && (
        <HubEllipse cx={W / 2} cy={hubCy} rx={110} ry={hubRy} label={hub.host} />
      )}
      {devices.map((d, i) => {
        const x = xs[i] ?? W / 2;
        const mine = d.is_self || d.host === self;
        return (
          <g key={d.host}>
            {hub && (
              <line
                x1={x}
                y1={deviceY}
                x2={W / 2}
                y2={hubCy + hubRy}
                stroke={mine ? "#9ece6a" : "#414868"}
                strokeWidth={1.5}
                strokeDasharray={mine ? undefined : "4,3"}
                markerEnd="url(#fleetarrow)"
              />
            )}
            <NodeRect
              x={x - dw / 2}
              y={deviceY}
              w={dw}
              h={56}
              label={d.host}
              subject={`${d.session_count} sessions`}
              highlight={mine}
            />
          </g>
        );
      })}
      <text x={W / 2} y={H - 14} textAnchor="middle" fontSize={11} fill="#565f89">
        {hub
          ? "Fleet inferred from evidence — this node reports solo (NATS leaf not yet self-detected); hub + hosts seen in sessions."
          : "Hosts seen in stored sessions (no hub connection detected)."}
      </text>
    </svg>
  );
}

// ── T1 ───────────────────────────────────────────────────────────────

function T1Map({ host, peers }: { host: string; peers: readonly string[] }) {
  // Place this node at the top, peers spread on the bottom.
  // For up to 4 peers we lay them out in a row; more falls back to a clipped row.
  const W = 700;
  const H = 320;
  const selfX = W / 2;
  const selfY = 50;
  const peerY = 240;
  const peerWidth = 130;
  const peerCount = peers.length;
  const peerXs = peerCount === 0
    ? []
    : peers.map((_, i) => {
        // Evenly spread peer centers within [80, W-80]
        if (peerCount === 1) return W / 2;
        const span = W - 160;
        return 80 + (span / (peerCount - 1)) * i;
      });

  return (
    <svg viewBox={`0 0 ${W} ${H}`} role="img" className="block mx-auto max-w-full h-auto">
      <defs>
        <marker id="t1arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill="#9ece6a" />
        </marker>
      </defs>
      <NodeRect x={selfX - 80} y={selfY} w={160} h={56} label={host} subject={`events.${host}.>`} highlight />
      {peerXs.map((px, i) => {
        const peer = peers[i] ?? "";
        return (
          <g key={peer || i}>
            <NodeRect x={px - peerWidth / 2} y={peerY} w={peerWidth} h={56} label={peer} subject={`events.${peer}.>`} />
            <line
              x1={selfX}
              y1={selfY + 56}
              x2={px}
              y2={peerY}
              stroke="#9ece6a"
              strokeWidth={1.5}
              markerStart="url(#t1arrow)"
              markerEnd="url(#t1arrow)"
            />
          </g>
        );
      })}
      {peers.length === 0 && (
        <text x={W / 2} y={peerY} textAnchor="middle" fill="#565f89" fontSize={12}>
          No peer devices configured (set OPEN_STORY_PEER_DOMAINS).
        </text>
      )}
    </svg>
  );
}

// ── T2 ───────────────────────────────────────────────────────────────

function T2Map({ host, hubDomain, isHub }: { host: string; hubDomain: string; isHub: boolean }) {
  // Hub above (or this if isHub), this node below.
  return (
    <svg viewBox="0 0 700 320" role="img" className="block mx-auto max-w-full h-auto">
      <defs>
        <marker id="t2up" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0,0 L10,5 L0,10 z" fill="#1f2937" />
        </marker>
        <marker id="t2down" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto">
          <path d="M0,0 L10,5 L0,10 z" fill="#0ea5e9" />
        </marker>
      </defs>
      <HubEllipse cx={350} cy={70} rx={100} ry={45} label={hubDomain} highlight={isHub} />
      <NodeRect
        x={270}
        y={220}
        w={160}
        h={56}
        label={host}
        subject={`events.${host}.>`}
        highlight={!isHub}
        muted={isHub}
      />
      {/* source up */}
      <line x1={350} y1={220} x2={350} y2={120} stroke="#c0caf5" strokeWidth={1.5} markerEnd="url(#t2up)" />
      {/* mirror down */}
      <line x1={330} y1={120} x2={330} y2={220} stroke="#0ea5e9" strokeWidth={1.5} strokeDasharray="4,3" markerEnd="url(#t2down)" />
      <text x={295} y={170} textAnchor="end" fontSize={11} fill="#c0caf5">source up</text>
      <text x={385} y={170} fontSize={11} fill="#0ea5e9">mirror down</text>
    </svg>
  );
}

// ── T3 ───────────────────────────────────────────────────────────────

function T3Map({
  host,
  hubDomain,
  peerHubs,
  isHub,
}: {
  host: string;
  hubDomain: string;
  peerHubs: readonly string[];
  isHub: boolean;
}) {
  // Two hubs side-by-side, this node hangs off its hub.
  const otherHub = peerHubs[0] ?? "hub-?";
  // Self's hub on the left, peer hub on the right by convention.
  return (
    <svg viewBox="0 0 700 360" role="img" className="block mx-auto max-w-full h-auto">
      <defs>
        <marker id="t3arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse">
          <path d="M0,0 L10,5 L0,10 z" fill="#c0caf5" />
        </marker>
      </defs>
      <HubEllipse cx={190} cy={150} rx={90} ry={42} label={hubDomain} highlight={isHub} />
      <HubEllipse cx={510} cy={150} rx={90} ry={42} label={otherHub} highlight={false} muted />
      {/* hub-to-hub bidirectional source */}
      <line x1={280} y1={150} x2={420} y2={150} stroke="#c0caf5" strokeWidth={2} markerStart="url(#t3arrow)" markerEnd="url(#t3arrow)" />
      <text x={350} y={140} textAnchor="middle" fontSize={11} fill="#565f89">hub ↔ hub</text>
      {/* self leaf under its hub */}
      <NodeRect
        x={110}
        y={270}
        w={160}
        h={56}
        label={host}
        subject={`events.${host}.>`}
        highlight={!isHub}
        muted={isHub}
      />
      <line x1={190} y1={270} x2={190} y2={192} stroke="#c0caf5" strokeWidth={1.5} markerEnd="url(#t3arrow)" />
      {/* placeholder for the other hub's leaves */}
      <text x={510} y={290} textAnchor="middle" fontSize={11} fill="#565f89">
        peer hub's leaves
      </text>
    </svg>
  );
}

// ── primitives ───────────────────────────────────────────────────────

function NodeRect({
  x,
  y,
  w,
  h,
  label,
  subject,
  highlight,
  muted,
}: {
  x: number;
  y: number;
  w: number;
  h: number;
  label: string;
  subject?: string;
  highlight?: boolean;
  muted?: boolean;
}) {
  const fill = highlight ? "#7aa2f7" : muted ? "#16161e" : "#24283b";
  const stroke = highlight ? "#7aa2f7" : "#414868";
  const textColor = highlight ? "#1a1b26" : muted ? "#565f89" : "#c0caf5";
  const sub = highlight ? "#1a1b26" : "#565f89";
  return (
    <g>
      <rect x={x} y={y} width={w} height={h} rx={8} fill={fill} stroke={stroke} strokeWidth={1.5} />
      <text x={x + w / 2} y={y + 22} textAnchor="middle" fontWeight={600} fontSize={13} fill={textColor}>
        {label}
      </text>
      {subject && (
        <text x={x + w / 2} y={y + 40} textAnchor="middle" fontSize={10} fontFamily="monospace" fill={sub}>
          {subject}
        </text>
      )}
    </g>
  );
}

function HubEllipse({
  cx,
  cy,
  rx,
  ry,
  label,
  highlight,
  muted,
}: {
  cx: number;
  cy: number;
  rx: number;
  ry: number;
  label: string;
  highlight?: boolean;
  muted?: boolean;
}) {
  const fill = highlight ? "#bb9af7" : muted ? "#16161e" : "#1a1b26";
  const stroke = highlight ? "#bb9af7" : "#bb9af7";
  const textColor = highlight ? "#1a1b26" : muted ? "#565f89" : "#bb9af7";
  return (
    <g>
      <ellipse cx={cx} cy={cy} rx={rx} ry={ry} fill={fill} stroke={stroke} strokeWidth={2} />
      <text x={cx} y={cy + 2} textAnchor="middle" fontWeight={600} fontSize={13} fill={textColor}>
        {label}
      </text>
      <text x={cx} y={cy + 18} textAnchor="middle" fontSize={10} fontFamily="monospace" fill={highlight ? "#1a1b26" : "#565f89"}>
        events.&gt;
      </text>
    </g>
  );
}
