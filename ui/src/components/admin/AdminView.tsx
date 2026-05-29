/**
 * AdminView — operator's federation console (v0).
 *
 * v0 surface (this commit): live read of /api/admin/topology with a
 * hand-rolled SVG rendering of T1/T2/T3/Solo shapes, geometry lifted
 * from docs/research/federation-topology-viz.html. Identifies *this*
 * node within the shape so the operator can see at a glance which
 * device they're configuring.
 *
 * Follows in subsequent steps:
 *   - share_policy table + per-session toggle
 *   - WS live invalidation
 *   - admin_token middleware (optional)
 */

import { useMemo } from "react";
import type { Topology, TopologyShape } from "@/lib/admin-api";
import { TopologyMap } from "@/components/admin/TopologyMap";
import { FleetGrid } from "@/components/admin/FleetGrid";
import { SharePolicyTable } from "@/components/admin/SharePolicyTable";
import { admin$ } from "@/streams/admin";
import { useObservable } from "@/hooks/use-observable";

export function AdminView() {
  // Sink: subscribe to the admin topology stream. First emission is the
  // REST-seeded snapshot; subsequent emissions are WS-pushed frames.
  // The stream itself is process-wide (shareReplay 1) so remounting
  // AdminView doesn't refetch.
  const stream = useMemo(() => admin$(), []);
  const topology = useObservable<Topology | null>(stream, null);
  const loading = topology === null;
  const error: string | null = null;

  return (
    <div className="p-6 max-w-5xl mx-auto" data-testid="admin-view">
      <header className="mb-6">
        <h2 className="text-xl font-semibold text-[#c0caf5] mb-1">Admin</h2>
        <p className="text-sm text-[#565f89]">
          This device's view of the federation it's running in. v0 is read-only —
          share/store policy toggles ship next.
        </p>
      </header>

      {loading && (
        <p className="text-sm text-[#565f89]">Loading topology…</p>
      )}
      {error && (
        <div className="rounded border border-red-900 bg-red-950/30 p-3 text-sm text-red-300">
          Failed to load topology: <code className="text-red-200">{error}</code>
        </div>
      )}

      {topology && (
        <>
          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">
                This node
              </h3>
              <ShapeBadge shape={topology.shape} />
            </div>
            <NodeIdentity topology={topology} />
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Fleet</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Every host this device has evidence of — self, hosts seen in
                stored sessions, and configured peers/hubs.
              </p>
            </header>
            <FleetGrid nodes={topology.nodes} />
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Shape</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Connectivity from this device's vantage. The Fleet panel above
                shows <em>presence</em>; this shows how data flows.
              </p>
            </header>
            <TopologyMap topology={topology} />
          </section>

          <section className="rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Share policy</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Sessions originating on this device. <code>shared</code> means
                they flow into the federation aggregate; <code>private</code>{" "}
                means they never leave this device. Default is <code>shared</code>.
              </p>
            </header>
            <SharePolicyTable selfHost={topology.self.host} />
          </section>
        </>
      )}
    </div>
  );
}

function ShapeBadge({ shape }: { shape: TopologyShape }) {
  const label = ({
    solo: "Solo",
    t1: "T1 · Solo multi-device",
    t2: "T2 · Single-hub star",
    t3: "T3 · Multi-hub mesh",
  } as const)[shape];
  return (
    <span className="rounded bg-[#414868] px-2 py-0.5 text-xs font-medium text-[#c0caf5]">
      {label}
    </span>
  );
}

function NodeIdentity({ topology }: { topology: Topology }) {
  const { self } = topology;
  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
      <Row k="Host" v={<code className="text-[#7aa2f7]">{self.host}</code>} />
      <Row
        k="Role"
        v={
          <span
            className={`inline-block rounded px-2 py-0.5 text-xs ${
              self.role === "hub"
                ? "bg-[#7aa2f7]/20 text-[#7aa2f7]"
                : self.role === "leaf"
                  ? "bg-[#9ece6a]/20 text-[#9ece6a]"
                  : "bg-[#565f89]/30 text-[#c0caf5]"
            }`}
          >
            {self.role}
          </span>
        }
      />
      <Row
        k="JS domain"
        v={self.domain ? <code className="text-[#bb9af7]">{self.domain}</code> : <em className="text-[#565f89]">none (solo)</em>}
      />
      <Row
        k="Hub domain"
        v={self.hub_domain ? <code className="text-[#bb9af7]">{self.hub_domain}</code> : <em className="text-[#565f89]">—</em>}
      />
      {self.peer_hub_domains.length > 0 && (
        <Row
          k="Peer hubs"
          v={<DomainList items={self.peer_hub_domains} color="#7aa2f7" />}
        />
      )}
      {self.peer_domains.length > 0 && (
        <Row
          k="Peer devices"
          v={<DomainList items={self.peer_domains} color="#9ece6a" />}
        />
      )}
    </dl>
  );
}

function Row({ k, v }: { k: string; v: React.ReactNode }) {
  return (
    <>
      <dt className="text-[#565f89]">{k}</dt>
      <dd>{v}</dd>
    </>
  );
}

function DomainList({ items, color }: { items: readonly string[]; color: string }) {
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((d) => (
        <code
          key={d}
          className="rounded px-1.5 py-0.5 text-xs"
          style={{ background: `${color}20`, color }}
        >
          {d}
        </code>
      ))}
    </div>
  );
}
