/**
 * /api/admin/* — operator-facing endpoints (topology, share/store policy).
 *
 * v0 surface is read-only: this device's view of the federation it boots
 * into. Share/store policy mutations land in a follow-up step.
 */

/** One of the four topology shapes the federation can be in. */
export type TopologyShape = "solo" | "t1" | "t2" | "t3";

/** What this node is doing in the topology. */
export type NodeRole = "solo" | "hub" | "leaf";

export interface NodeInfo {
  readonly host: string;
  readonly role: NodeRole;
  readonly domain: string | null;
  readonly hub_domain: string | null;
  readonly peer_hub_domains: readonly string[];
  readonly peer_domains: readonly string[];
}

export type NodeEvidence =
  | "self-node"
  | "sessions"
  | "peer-config"
  | "hub-config"
  | "nats-leafnode-hub";

export interface NodeSummary {
  readonly host: string;
  readonly is_self: boolean;
  readonly session_count: number;
  readonly source: NodeEvidence;
}

export interface LiveSourceSummary {
  readonly name: string;
  readonly host: string | null;
  readonly api_prefix: string | null;
  readonly lag: number;
  readonly active_ms: number | null;
}

/** Phase 5.7+ — fleet roster grouped by the person who owns sessions on each host.
 *  A host can appear in multiple clusters (one per person who has sessions there).
 *  Empty/absent when no sessions in the local store have a person_id stamp. */
export interface PersonCluster {
  readonly person_id: string;
  readonly hosts: readonly string[];
}

export interface Topology {
  readonly shape: TopologyShape;
  readonly self: NodeInfo;
  readonly nodes: readonly NodeSummary[];
  /** Authoritative fleet roster from JetStream's events-agg.sources[].
   *  null when this node has no JetStream context (NoopBus) or isn't a hub. */
  readonly live_sources?: readonly LiveSourceSummary[] | null;
  /** Phase 5.7 — hosts grouped by sovereign owner. May be absent on older
   *  server versions; treat missing/empty as "no clustering data, render
   *  the flat fleet view." */
  readonly clusters_by_person?: readonly PersonCluster[];
}

export async function fetchTopology(signal?: AbortSignal): Promise<Topology> {
  const res = await fetch("/api/admin/topology", { signal });
  if (!res.ok) {
    throw new Error(`fetchTopology: ${res.status} ${res.statusText}`);
  }
  return res.json();
}

// ── Participants (Phase 6 polish) — read-only in the UI ─────────────────
//
// Role grants/revocations (PUT/DELETE /api/admin/participants) exist in the
// backend but are intentionally not wired into the UI; the admin surface is
// read-only until the sharing/identity model is hardened end-to-end. Mutations
// happen via the CLI (`open-story grant-role`).

export type Role = "observer" | "contributor" | "admin";

export interface Participant {
  readonly principal_id: string;
  readonly person_id: string;
  readonly role: Role;
  readonly created_at: string;
}

export async function fetchParticipants(signal?: AbortSignal): Promise<readonly Participant[]> {
  const res = await fetch("/api/admin/participants", { signal });
  if (!res.ok) throw new Error(`fetchParticipants: ${res.status} ${res.statusText}`);
  const body = await res.json();
  return body.participants ?? [];
}
