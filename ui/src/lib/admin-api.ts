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

export interface Topology {
  readonly shape: TopologyShape;
  readonly self: NodeInfo;
}

export async function fetchTopology(signal?: AbortSignal): Promise<Topology> {
  const res = await fetch("/api/admin/topology", { signal });
  if (!res.ok) {
    throw new Error(`fetchTopology: ${res.status} ${res.statusText}`);
  }
  return res.json();
}
