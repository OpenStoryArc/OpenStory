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
import { PersonClustersView } from "@/components/admin/PersonClustersView";
import { ParticipantsPanel } from "@/components/admin/ParticipantsPanel";
import { FleetGrid } from "@/components/admin/FleetGrid";
import { LiveSourcesPanel } from "@/components/admin/LiveSourcesPanel";
import { SharePolicyTable } from "@/components/admin/SharePolicyTable";
import { BetaBadge } from "@/components/admin/BetaBadge";
import { DataSourceNote } from "@/components/admin/DataSourceNote";
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
        <h2 className="text-xl font-semibold text-[#c0caf5] mb-1 flex items-center">
          Admin
          <BetaBadge note="Beta — the whole admin surface (federation, sharing, roles) is new and not guaranteed to work yet. Keep testing before relying on it." />
        </h2>
        <p className="text-sm text-[#565f89]">
          This device's view of the federation it's running in — topology,
          sharing, and roles. Everything here is computed by{" "}
          <em>this node</em> from three REST endpoints
          (<code className="text-[#7dcfff]">/api/admin/topology</code>,{" "}
          <code className="text-[#7dcfff]">/api/admin/participants</code>,{" "}
          <code className="text-[#7dcfff]">/api/admin/share-policy</code>). Each
          subsection notes its endpoint and how its data is derived, with a dot
          showing whether it's a deterministic read of local state or a live
          network probe:
        </p>
        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-[#565f89]">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[#9ece6a]" />
            deterministic — local store / config / roles DB
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[#7aa2f7]" />
            mostly local · one live probe
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[#e0af68]" />
            live network — best-effort, varies
          </span>
        </div>
        <div className="mt-2 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
          <strong>Beta:</strong> these controls are new and still being
          hardened. They work in our tests but aren't guaranteed end-to-end —
          verify behavior before depending on it, and expect rough edges.
        </div>
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
            <DataSourceNote
              endpoint="GET /api/admin/topology › self"
              derivation="host, role & JS/hub domain read straight from config.toml (role is derived from config, not probed)"
              kind="local"
            />
            <NodeIdentity topology={topology} />
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Fleet</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Every host this device has evidence of — self, hosts seen in
                stored sessions, and configured peers/hubs.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › nodes"
                derivation="self + hosts tallied from your local session store (deterministic); the NATS upstream hub is a live /leafz probe of this node's own NATS"
                kind="mixed"
              />
            </header>
            <FleetGrid nodes={topology.nodes} />
          </section>

          <section className="mb-6 rounded-lg border border-[#bb9af7]/30 bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#bb9af7]">
                Live sources <span className="text-xs text-[#565f89]">— from JetStream events-agg</span>
              </h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Authoritative fleet roster — every leaf currently registered with the hub aggregate, with delivery state.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › live_sources"
                derivation="queried live from the hub's JetStream events-agg stream; null when this node isn't federated (as below)"
                kind="live"
              />
            </header>
            {topology.live_sources ? (
              <LiveSourcesPanel sources={topology.live_sources} />
            ) : (
              <div className="rounded border border-dashed border-[#414868] p-3 text-sm text-[#565f89]">
                <p>
                  <strong className="text-[#c0caf5]">Not federated.</strong>{" "}
                  JetStream's <code>events-agg</code> stream isn't reachable
                  from this node — either there's no NATS context (NoopBus) or
                  no events-agg has been created yet. The wire is in place;
                  the panel will populate the moment a hub aggregate appears.
                </p>
                <p className="mt-2 text-xs">
                  To light it up locally:{" "}
                  <code className="bg-[#16161e] px-1 py-0.5 rounded">
                    OPEN_STORY_HUB_DOMAIN=hub just serve
                  </code>{" "}
                  with NATS configured{" "}
                  <code className="bg-[#16161e] px-1 py-0.5 rounded">
                    jetstream {`{ domain: hub }`}
                  </code>
                  .
                </p>
              </div>
            )}
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Shape</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Connectivity from this device's vantage. The Fleet panel above
                shows <em>presence</em>; this shows how data flows.
              </p>
              <DataSourceNote
                endpoint="derived client-side from topology.nodes"
                derivation="drawn from the Fleet nodes — the solid self→hub edge rides the live /leafz probe; dashed edges are inferred from stored sessions"
                kind="mixed"
              />
            </header>
            <TopologyMap topology={topology} />
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Participants & roles</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Phase 6 role directory. Granting <code>Admin</code> here lets
                a principal mutate share policy + share-with-person; lower
                tiers (<code>Contributor</code>, <code>Observer</code>) are
                reserved for future routes. Bootstrap the first Admin from
                the CLI — every admin route 403s until one exists.
              </p>
              <DataSourceNote
                endpoint="GET/PUT /api/admin/participants"
                derivation="the EmbeddedRoleDirectory SQLite at roles_db_path; grants/edits are role-gated (admin-only)"
                kind="local"
              />
            </header>
            <ParticipantsPanel />
          </section>

          <section className="mb-6 rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Person clusters</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Fleet grouped by sovereign owner. A host can appear under
                multiple persons — a shared dev box is normal. Cross-person
                share edges are added in a follow-up.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › clusters_by_person"
                derivation="your local session store grouped by each session's stamped person_id"
                kind="local"
              />
            </header>
            <PersonClustersView
              clusters={topology.clusters_by_person ?? []}
              selfHost={topology.self.host}
            />
          </section>

          <section className="rounded-lg border border-[#24283b] bg-[#1a1b26] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[#c0caf5]">Share policy</h3>
              <p className="text-xs text-[#565f89] mt-0.5">
                Sessions originating on this device. <code>shared</code> means
                they flow into the federation aggregate; <code>private</code>{" "}
                means they never leave this device. The default is{" "}
                <strong className="text-[#9ece6a]">opt-in</strong>: a
                loopback-only instance defaults to <code>shared</code> (your
                local dashboard just works), but once networking is configured
                the default flips to <code>private</code> so going to a hub
                never auto-shares your history.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/share-policy · PUT /…/{session}"
                derivation="session store + share_policy table; an unset session's value is derived from config (loopback→shared, networked→private). Writes are admin-only"
                kind="local"
              />
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
