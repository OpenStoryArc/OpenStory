# Three hubs, no Raft

**Date:** 2026-09-25 · **Status:** plan · **Branch:** `feat/federation-domains` (stacked on `feat/consistency-hands`) · **Worktree:** `~/projects/openstory-wt-federation` · **Companion:** the "Two Layers of NATS" page, and `docs/research/jetstream-sources-federation.md` (Idea A, prototype).

## The decision

The fleet gets three hubs, the Hetzner box, the owner's mini, and a1, as a **mesh of JetStream domains**, not as a Raft cluster. Each hub runs its own domain and its own aggregate, and each aggregate sources from the other two. Leaves attach to whichever hub is close. A hub that goes dark stops pulling until it returns, then pulls the gap by cursor. There is no election, no majority commit across the WAN, and no agreed global order, because the store is a set of immutable events keyed by id and union is the merge. Raft is reserved for the day one hub must survive its own box with all its members in one datacenter.

## What exists

- The node reads `OPEN_STORY_HUB_DOMAIN` and, when it is a hub, `OPEN_STORY_PEER_HUB_DOMAINS`; `NatsBus::connect_hub` plus `ensure_aggregate(peer_hub_domains)` build the aggregates with a source per leaf and a source per peer hub (`rs/cli/src/main.rs`, `rs/bus/src/nats_bus.rs`). Leaves get `events-mirror` and `presence-mirror` from their hub's aggregate (`events_mirror_config`, `presence_mirror_config`); the mesh variants exist for hub-less peers (`FederationPeers::Mesh`).
- `rs/tests/test_federation_lab.rs` asserts every machine sees every session; `test_federation_scale.rs` and `test_tailnet_federation.rs` cover scale and the tailnet.
- Neither deployed NATS config sets a domain (`deploy/nats-hub.conf.template`, `deploy/nats-leaf.conf` in openstory-deploy), and the brew-managed NATS on the laptop renders none. So today everything runs on leaf-link fan-out with no replay.
- Per-host watermarks, the consistency report, and `node_converge` (rows C-02, C-03, C-05 on the consistency branch) read the same cursors this plan turns on.

## Rows

Protocol as in `REQUIREMENTS.md`. Every roll below restarts one machine's NATS once, which drops and reconnects its leaves; nothing restarts a node's store. The gate on every row is the lab test plus a reading through the ops hands, not a screenshot.

| id | requirement | red spec |
|---|---|---|
| F-01 | **Size the aggregate.** A pure function `fleet::aggregate_cap(leaf_caps, headroom) -> bytes` and a `stream_cap` finding that names the aggregate when it is within 70 % of `max_file`. The deploy templates take `JETSTREAM_MAX_FILE` and `EVENTS_AGG_MAX_BYTES` from the env file instead of literals. | pure test; the health verdict test with an aggregate near cap; a rendered-config test in openstory-deploy's scripts |
| F-02 | **Domains on the laptop and the Hetzner hub.** The managed NATS renders `domain: <host>`; the hub template renders `domain: hub`; the nodes get `OPEN_STORY_HUB_DOMAIN=hub`. The hub's `events-agg` gains a source with the laptop's domain and a moving cursor; the laptop's `events-mirror` fills from the hub. | `rs/tests/test_federation_lab.rs` runs with domains on two nodes; health on both shows the mirror and the source; `node_streams` lists them with caps |
| F-03 | **Two more hubs.** The mini and a1 render `domain: mini` and `domain: a1` and run as hubs with `OPEN_STORY_PEER_HUB_DOMAINS` naming the other two; each aggregate sources from the other two. The Hetzner hub names them as peers too. | lab test with three hubs and one leaf per hub: every node's roll-up (C-01) is equal after the leaves publish; a hub taken down and brought back reaches the same roll-up without any hand |
| F-04 | **Leaves choose a hub.** The node compose and the brew wizard take `NATS_LEAF_URL` as a list; the leaf tries them in order. A peer on another tailnet can be shared any one hub and still federate. | rendered-config test; lab test where a leaf's first hub is down and it attaches to the second |
| F-05 | **Backfill without a hand.** A leaf dark for a window comes back and its mirror pulls the window by cursor. `consistency_report` shows `behind:<host>` closing to nothing with no `converge` call. | lab test with a partition long enough to cross the report's threshold, then healed; `subscribe_convergence` emits one transition to agreed |
| F-06 | **Beyond the cap.** When the window exceeds the aggregate's retention, the report says so as `beyond_bus:<host>` and `node_converge` falls back to catch-up by digest against a node's store. | lab test with a cap small enough to roll off; the finding names the host; converge heals it from a peer store |
| F-07 | **Rollout, gated.** Order: laptop and Hetzner hub (F-02), then the mini as a hub, then a1, then the peer's leaf. Each step: the lab test green on the branch, the deploy PR merged by the owner, the roll through the deploy workflow or the wizard, and the fleet's roll-ups equal on the Fleet tab (C-07) before the next step. | loop log with `date -u` times and the consistency verdict at each step |

## What this does not do

No consensus. No change to who writes observed history: the translator on the origin host is the only publisher of `events.*`. No change to the tier rule: turning a domain on is a tier-2 act carried out by a person through the deploy repo, and every step after it is readable through the hands.

## Loop log
