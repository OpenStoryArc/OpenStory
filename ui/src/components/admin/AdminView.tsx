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
import { BetaBadge } from "@/components/admin/BetaBadge";
import { DataSourceNote } from "@/components/admin/DataSourceNote";
import { HowItWorks } from "@/components/admin/HowItWorks";
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
        <h2 className="text-xl font-semibold text-[color:var(--text)] mb-1 flex items-center">
          Admin
          <BetaBadge note="Beta — this is a read-only view of the federation and identity model. It reflects state; it does not change it. The write surface (sharing, role grants) is intentionally not wired into the UI yet." />
        </h2>
        <p className="text-sm text-[color:var(--text-muted)]">
          This device's <strong>read-only</strong> view of the federation it's
          running in — topology, fleet, and identity. Everything here is
          computed by <em>this node</em> from two REST endpoints
          (<code className="text-[color:var(--cyan-bright)]">/api/admin/topology</code>,{" "}
          <code className="text-[color:var(--cyan-bright)]">/api/admin/participants</code>). It{" "}
          <em>observes</em> state — nothing on this page mutates sharing, roles,
          or any session. Each subsection notes its endpoint and how its data is
          derived, with a dot showing whether it's a deterministic read of local
          state or a live network probe:
        </p>
        <div className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-[11px] text-[color:var(--text-muted)]">
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[color:var(--green)]" />
            deterministic — local store / config / roles DB
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[color:var(--accent)]" />
            mostly local · one live probe
          </span>
          <span className="flex items-center gap-1.5">
            <span className="inline-block h-1.5 w-1.5 rounded-full bg-[color:var(--orange)]" />
            live network — best-effort, varies
          </span>
        </div>
        <div className="mt-2 rounded border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-xs text-amber-200">
          <strong>Beta · read-only:</strong> the sharing &amp; consent and
          role-grant machinery exists in the backend but is{" "}
          <strong>not driven from this UI yet</strong>. This page shows you what
          the system currently believes — it makes no claim to change it.
          Mutations stay on the CLI until the model is hardened end-to-end.
        </div>
      </header>

      {loading && (
        <p className="text-sm text-[color:var(--text-muted)]">Loading topology…</p>
      )}
      {error && (
        <div className="rounded border border-red-900 bg-red-950/30 p-3 text-sm text-red-300">
          Failed to load topology: <code className="text-red-200">{error}</code>
        </div>
      )}

      {topology && (
        <>
          <section className="mb-6 rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-4">
            <div className="flex items-center justify-between mb-3">
              <h3 className="text-sm font-medium text-[color:var(--text)]">
                This node
              </h3>
              <ShapeBadge shape={topology.shape} />
            </div>
            <DataSourceNote
              endpoint="GET /api/admin/topology › self"
              derivation="host, role & JS/hub domain read straight from config.toml (role is derived from config, not probed)"
              kind="local"
            />
            <HowItWorks summary="How this node's identity is determined">
              <p>
                <code>compute_topology</code> reads <code>EnvInputs</code> +
                config: host, role, and JS/hub domain. <strong>Role is
                derived from your configured peer/hub domains</strong> — it is{" "}
                <em>not</em> probed from the live NATS. That's why a box whose
                NATS is leaf-connected to a hub can still read{" "}
                <code>solo</code> here: open-story hasn't been told it's a leaf
                (the <code>/leafz</code> auto-detection is a backlog item).
              </p>
              <p>
                The snapshot is computed by a background actor and pushed to a{" "}
                <code>watch</code> channel; <code>GET /api/admin/topology</code>{" "}
                just serves the latest frame — no compute, NATS calls, or
                config reads on the request hot path.
              </p>
            </HowItWorks>
            <NodeIdentity topology={topology} />
          </section>

          <section className="mb-6 rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[color:var(--text)]">Fleet</h3>
              <p className="text-xs text-[color:var(--text-muted)] mt-0.5">
                Every host this device has evidence of — self, hosts seen in
                stored sessions, and configured peers/hubs.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › nodes"
                derivation="self + hosts tallied from your local session store (deterministic); the NATS upstream hub is a live /leafz probe of this node's own NATS"
                kind="mixed"
              />
              <HowItWorks summary="How the fleet roster is assembled">
                <p>
                  <code>compute_topology</code> merges evidence from four
                  sources and dedupes by host:
                </p>
                <ul className="ml-4 list-disc space-y-1">
                  <li>
                    <strong>self</strong> — this device, from config.
                  </li>
                  <li>
                    <strong>seen in sessions</strong> — every distinct origin
                    host tallied from your local session store
                    (<code>list_sessions</code>), with its session count.
                    Durable, not live — "I hold their data," not "they're
                    online."
                  </li>
                  <li>
                    <strong>NATS upstream hub</strong> —{" "}
                    <code>discover_leafnode_upstream()</code> does a best-effort
                    HTTP GET to your local NATS monitor
                    (<code>:8222/leafz</code>) and reads the leaf connection
                    your NATS currently holds. The one live bit.
                  </li>
                  <li>
                    <strong>configured peers/hubs</strong> — from{" "}
                    <code>OPEN_STORY_PEER_DOMAINS</code> / hub env (presence
                    only).
                  </li>
                </ul>
                <p>
                  Recomputed on each pulse (a coalesced "session set changed"
                  signal), served from the cached <code>watch</code> snapshot.
                </p>
              </HowItWorks>
            </header>
            <FleetGrid nodes={topology.nodes} />
          </section>

          <section className="mb-6 rounded-lg border border-[color:var(--purple)]/30 bg-[color:var(--bg)] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[color:var(--purple)]">
                Live sources <span className="text-xs text-[color:var(--text-muted)]">— from JetStream events-agg</span>
              </h3>
              <p className="text-xs text-[color:var(--text-muted)] mt-0.5">
                Authoritative fleet roster — every leaf currently registered with the hub aggregate, with delivery state.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › live_sources"
                derivation="read live from THIS node's JetStream events-agg aggregate; populated only when this node IS the hub. Null on solo / T1 / leaf — including a leaf that's connected to a hub (like this node), since only the hub holds events-agg"
                kind="live"
              />
              <HowItWorks summary="How the live roster is read">
                <p>
                  <code>fetch_live_sources()</code> calls{" "}
                  <code>js.get_stream("events-agg").info()</code> on{" "}
                  <em>this node's own</em> JetStream, then pairs the stream's{" "}
                  <code>config.sources</code> against its runtime{" "}
                  <code>sources[]</code> (lag, active, delivered) via{" "}
                  <code>derive_live_sources()</code>.
                </p>
                <p>
                  It returns <code>None</code> when{" "}
                  <code>events-agg</code> doesn't exist locally — i.e. on solo,
                  T1, and <strong>leaf</strong> nodes — because only a{" "}
                  <strong>hub</strong> hosts that aggregate. A leaf{" "}
                  <em>sources</em> from the hub's events-agg into its own{" "}
                  <code>events-mirror</code>, but doesn't hold events-agg, so
                  this panel stays empty even while federated. Transient errors
                  also yield <code>None</code>; the next pulse retries.
                </p>
              </HowItWorks>
            </header>
            {topology.live_sources ? (
              <LiveSourcesPanel sources={topology.live_sources} />
            ) : (
              <div className="rounded border border-dashed border-[color:var(--border)] p-3 text-sm text-[color:var(--text-muted)]">
                <p>
                  <strong className="text-[color:var(--text)]">No hub aggregate here.</strong>{" "}
                  <code>events-agg</code> lives on the <em>hub</em> — this node
                  isn't hosting one. It's solo, T1, or a leaf; a leaf can be
                  connected to a hub (like this node) and still show nothing
                  here, because only the hub holds <code>events-agg</code>.
                  Either there's no NATS context (NoopBus), or no events-agg
                  exists in this node's JetStream. The wire is in place; the
                  panel populates when this node runs the hub aggregate.
                </p>
                <p className="mt-2 text-xs">
                  To light it up locally:{" "}
                  <code className="bg-[color:var(--bg)] px-1 py-0.5 rounded">
                    OPEN_STORY_HUB_DOMAIN=hub just serve
                  </code>{" "}
                  with NATS configured{" "}
                  <code className="bg-[color:var(--bg)] px-1 py-0.5 rounded">
                    jetstream {`{ domain: hub }`}
                  </code>
                  .
                </p>
              </div>
            )}
          </section>

          <section className="mb-6 rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[color:var(--text)]">Shape</h3>
              <p className="text-xs text-[color:var(--text-muted)] mt-0.5">
                Connectivity from this device's vantage. The Fleet panel above
                shows <em>presence</em>; this shows how data flows.
              </p>
              <DataSourceNote
                endpoint="derived client-side from topology.nodes"
                derivation="drawn from the Fleet nodes — the solid self→hub edge rides the live /leafz probe; dashed edges are inferred from stored sessions"
                kind="mixed"
              />
              <HowItWorks summary="How the shape is drawn">
                <p>
                  Pure client-side SVG — no extra endpoint. It switches on{" "}
                  <code>topology.shape</code> (<code>solo</code>/<code>t1</code>/
                  <code>t2</code>/<code>t3</code>) for the federation geometry.
                </p>
                <p>
                  When the node reports <code>solo</code> but{" "}
                  <code>nodes</code> has other hosts (your case),{" "}
                  <code>FleetShape</code> infers the graph from evidence rather
                  than drawing a lonely card: the detected hub as the center,
                  self + device hosts around it. Edge styling encodes
                  confidence — <strong>self→hub solid</strong> (observed via the
                  live <code>/leafz</code> probe), <strong>other hosts→hub
                  dashed</strong> ("seen via the hub", inferred from their
                  stored sessions). So it's archaeology, not radar — until the{" "}
                  <code>/leafz</code> self-detection lands and the real T2 shape
                  is reported.
                </p>
              </HowItWorks>
            </header>
            <TopologyMap topology={topology} />
          </section>

          <section className="mb-6 rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[color:var(--text)]">Participants & roles</h3>
              <p className="text-xs text-[color:var(--text-muted)] mt-0.5">
                Phase 6 role directory, shown <strong>read-only</strong>. Each
                principal's role (<code>Observer</code> &lt;{" "}
                <code>Contributor</code> &lt; <code>Admin</code>) is listed as
                stored; grants and revocations are done from the CLI
                (<code>open-story grant-role</code>), not here. The first Admin
                must be bootstrapped from the CLI — every admin <em>write</em>{" "}
                route 403s until one exists.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/participants"
                derivation="read-only list from the EmbeddedRoleDirectory SQLite at roles_db_path; grants/edits happen via the CLI, not this UI"
                kind="local"
              />
              <HowItWorks summary="How roles are stored and enforced">
                <p>
                  <strong>Store.</strong> The{" "}
                  <code>EmbeddedRoleDirectory</code> is a SQLite{" "}
                  <code>participants</code> table at <code>roles_db_path</code>{" "}
                  (defaults to <code>{"{data_dir}/openstory-roles.db"}</code>):
                  one row per principal → person → role
                  (<code>observer</code> &lt; <code>contributor</code> &lt;{" "}
                  <code>admin</code>).
                </p>
                <p>
                  <strong>Enforcement.</strong> Every admin route runs through{" "}
                  <code>require_admin_role_middleware</code>, which looks up{" "}
                  <code>role_for_principal(local_principal_id)</code> and 403s
                  unless it's ≥ <code>Admin</code>. GET lists; PUT upserts;
                  DELETE revokes (all admin-gated).
                </p>
                <p>
                  <strong>Bootstrap.</strong> On a <em>trusted-local</em> first
                  boot (loopback, no api_token, no hub) the local principal is
                  auto-granted Admin and <code>local_principal_id</code> is
                  auto-filled — so it just works. A networked or exposed
                  instance is not trusted-local, so it keeps the deliberate{" "}
                  <code>open-story grant-role</code> CLI bootstrap (no ambient
                  admin reachable over the network).
                </p>
              </HowItWorks>
            </header>
            <ParticipantsPanel />
          </section>

          <section className="mb-6 rounded-lg border border-[color:var(--bg-surface)] bg-[color:var(--bg)] p-4">
            <header className="mb-3">
              <h3 className="text-sm font-medium text-[color:var(--text)]">Person clusters</h3>
              <p className="text-xs text-[color:var(--text-muted)] mt-0.5">
                Fleet grouped by sovereign owner. A host can appear under
                multiple persons — a shared dev box is normal. Cross-person
                share edges are added in a follow-up.
              </p>
              <DataSourceNote
                endpoint="GET /api/admin/topology › clusters_by_person"
                derivation="your local session store grouped by each session's stamped person_id"
                kind="local"
              />
              <HowItWorks summary="How clusters are grouped">
                <p>
                  <code>compute_topology_with_owners</code> takes{" "}
                  <code>(host, person_id)</code> pairs from your session store
                  and groups hosts by <code>person_id</code> — the identity
                  stamped on each event at ingest by the principal resolver
                  (<code>[person].principals</code> matchers: agent / host /
                  user / watch-dir).
                </p>
                <p>
                  A host can appear under multiple persons (a shared dev box is
                  normal), and sessions with no stamped person fall outside any
                  cluster. Store-derived and deterministic; recomputed on pulse.
                </p>
              </HowItWorks>
            </header>
            <PersonClustersView
              clusters={topology.clusters_by_person ?? []}
              selfHost={topology.self.host}
            />
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
    <span className="rounded bg-[color:var(--border)] px-2 py-0.5 text-xs font-medium text-[color:var(--text)]">
      {label}
    </span>
  );
}

function NodeIdentity({ topology }: { topology: Topology }) {
  const { self } = topology;
  return (
    <dl className="grid grid-cols-2 gap-x-6 gap-y-2 text-sm">
      <Row k="Host" v={<code className="text-[color:var(--accent)]">{self.host}</code>} />
      <Row
        k="Role"
        v={
          <span
            className={`inline-block rounded px-2 py-0.5 text-xs ${
              self.role === "hub"
                ? "bg-[color:var(--accent)]/20 text-[color:var(--accent)]"
                : self.role === "leaf"
                  ? "bg-[color:var(--green)]/20 text-[color:var(--green)]"
                  : "bg-[color:var(--text-muted)]/30 text-[color:var(--text)]"
            }`}
          >
            {self.role}
          </span>
        }
      />
      <Row
        k="JS domain"
        v={self.domain ? <code className="text-[color:var(--purple)]">{self.domain}</code> : <em className="text-[color:var(--text-muted)]">none (solo)</em>}
      />
      <Row
        k="Hub domain"
        v={self.hub_domain ? <code className="text-[color:var(--purple)]">{self.hub_domain}</code> : <em className="text-[color:var(--text-muted)]">—</em>}
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
      <dt className="text-[color:var(--text-muted)]">{k}</dt>
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
