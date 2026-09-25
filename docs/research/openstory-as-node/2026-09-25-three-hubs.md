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

## Deploy diff (proposed, not applied)

What the deploy repo and the laptop need for F-01 and F-02, as diffs to propose, not changes made. Every line below is a tier-2 act a person carries out; the lab on this branch is the evidence for it. The node code on this branch already reads every variable named here.

### openstory-deploy, the Hetzner hub

`deploy/nats-hub.conf.template`:

```diff
 jetstream {
     store_dir: /data/jetstream
     max_mem: 256MB
-    # 4GB covers configured stream sizes (events 1GB + patterns 256MB) plus
-    # metadata/compaction headroom. NatsBus::ensure_streams() hardcodes the
-    # events stream at max_bytes 1GB, so values below ~1.3GB crash startup.
-    max_file: 4GB
+    # JetStream reserves every stream's max_bytes against this, so it must
+    # exceed the node's own caps (events 1GB, local 1GB, patterns 256MB, the
+    # 64MB families) plus EVENTS_AGG_MAX_BYTES; the health verdict raises
+    # stream_cap:events-agg when the aggregate claims 70 % of it.
+    max_file: ${JETSTREAM_MAX_FILE}
+    # The hub's JetStream domain: leaves reach $JS.hub.API to register their
+    # events as sources on events-agg and to mirror it back down.
+    domain: ${JETSTREAM_DOMAIN}
 }
```

`scripts/deploy.sh`:

```diff
-envsubst '${NATS_LEAF_TOKEN}' < deploy/nats-hub.conf.template > deploy/nats-hub.conf
+envsubst '${NATS_LEAF_TOKEN} ${JETSTREAM_DOMAIN} ${JETSTREAM_MAX_FILE}' < deploy/nats-hub.conf.template > deploy/nats-hub.conf
```

`deploy/infra.env.example` (and `deploy/infra.env` on the box):

```diff
+# JetStream on the hub NATS (three hubs F-01/F-02).
+JETSTREAM_DOMAIN=hub
+# 8GB: the node's ~2.5GB of local caps + EVENTS_AGG_MAX_BYTES + headroom.
+JETSTREAM_MAX_FILE=8GB
+# fleet::aggregate_cap(leaf caps, 25 % headroom): three 1GB leaves today.
+EVENTS_AGG_MAX_BYTES=4026531840
```

`docker-compose.infra.yml`, the `open-story` service's `environment`:

```diff
       - NATS_URL=nats://${NATS_LEAF_TOKEN}@nats:4222
+      # F-02: this node is the hub of the federation. It runs role full
+      # (it watches Bobby and Katie), so it says it is the hub by naming
+      # the domain its NATS serves; connect_hub then pins $JS.hub.API,
+      # creates events-agg / presence-agg, and registers its own events
+      # and presence on them filtered to its host.
+      - OPEN_STORY_HUB_DOMAIN=${JETSTREAM_DOMAIN:-hub}
+      - OPEN_STORY_JETSTREAM_DOMAIN=${JETSTREAM_DOMAIN:-hub}
+      # F-01: the aggregate's cap and, for the health verdict, the file
+      # store size (bytes; the monitor's /jsz answers it first).
+      - OPEN_STORY_EVENTS_AGG_MAX_BYTES=${EVENTS_AGG_MAX_BYTES:-1073741824}
+      - OPEN_STORY_NATS_MONITOR_URL=http://nats:8222
```

A rendered-config test for `scripts/` in the deploy repo (F-01's third red spec) belongs there: render the template with a fixture env and assert `domain: hub`, `max_file: 8GB`, and the token line, so the box never receives a config with an unexpanded variable.

### openstory-deploy, a node (`docker-compose.node.yml`, `deploy/nats-leaf.conf`)

```diff
 jetstream {
     store_dir: /tmp/nats-jetstream-leaf
     max_mem: 256MB
     max_file: 4GB
+    # This machine's JetStream domain: must equal OPEN_STORY_HOST, the
+    # host token the node stamps and binds events.<host>.> with.
+    domain: $OPEN_STORY_HOST
 }
```

```diff
       - OPEN_STORY_HOST=${OPEN_STORY_HOST:-}
+      # F-02: a leaf of the hub; the bus pins $JS.<host>.API locally,
+      # sources events-agg from $JS.hub.API into events-mirror, and
+      # registers its events on the hub aggregate.
+      - OPEN_STORY_HUB_DOMAIN=hub
+      - OPEN_STORY_NATS_MONITOR_URL=http://nats:8222
```

### The laptop (brew / foreground `open-story serve`)

Today the laptop's NATS is `data/nats/leaf-fixed.conf`, launched by hand, not the managed one; it has no domain. Two ways, the owner's pick:

- Managed: run `open-story serve --manage-nats` with `OPEN_STORY_HUB_DOMAIN=hub` in the environment (`nats_leaf_url` stays in `data/config.toml`). `render_leaf_config` now writes `domain: "Maxs-Air"` (this host's token) into `data/nats/leaf.conf`. No config file edit.
- By hand: add `domain: "Maxs-Air"` inside `jetstream { }` of `leaf-fixed.conf`, restart that nats-server, then start the node with `OPEN_STORY_HUB_DOMAIN=hub`.

Either way the node must see `OPEN_STORY_HUB_DOMAIN=hub` (env only; there is no config.toml key), and the hub must already serve `domain: hub`, or `ensure_streams` fails at self-registration and the node does not boot: roll the hub first (F-07's order).

### Known edges for the roll

- `get_or_create_stream` never edits an existing stream. A store that already has `events` bound to `events.>` keeps that binding after the switch to leaf mode; the node's event-id dedup absorbs the double delivery (core propagation plus the mirror), at the cost of bytes. To narrow it: `nats stream edit events --subjects 'events.<host>.>'` on that machine, a tier-2 act.
- The hub's `events` stream stays bound to `events.>` too; the own-source on `events-agg` is filtered to `events.<hub host>.>`, so leaf events that reach the hub's `events` by core propagation are not sourced twice.
- `OPEN_STORY_HOST` on the box must equal the host the hub's own-source filter uses (the node reads the same `host()` for both), and the box's events must carry it in their subject, which they do since the host went into the subject.
- `max_file` is reserved, not used: JetStream refuses stream creation when the sum of caps exceeds it (the lab hit `insufficient storage resources`, error 10047, at 256MB), so size it from the caps, not from disk usage.

## Loop log

- `2026-09-25T19:21Z` start. Worktree `feat/federation-domains`, fresh `rs/target`. Read the plan, REQUIREMENTS (loop protocol, G), the sources doc, the lab test, CLAUDE.md. Findings against the code: (1) `rs/tests/test_federation_lab.rs` is Docker compose only; the scratch `nats-server` pattern lives in `rs/bus/tests/test_bus_health.rs` (`Scratch::start_on`), so the domain lab reuses that and `open_story::server::run_server` in-process. (2) The monitor `/jsz` answers `config.max_storage` and `config.domain` (probed on :8501); async-nats `query_account()` carries `limits.max_storage` and `domain` too, so the bus can read `max_file` without an `http_port`. (3) `SourceInfo` in async-nats 0.49 has `name`, `lag`, `active` but not `external`, so a source's domain comes from the stream config's `sources[].external.api` through `parse_js_api_prefix`. (4) `open-story-mcp` has no path back to `open-story`, so it can be a dev-dependency and the lab can call `tools::ops::node_streams` against the in-process nodes.
- `2026-09-25T19:48Z` F-01 green. `fleet::aggregate_cap` (sum of capped leaves plus headroom, rounded up), `Bus::jetstream_limits` (account info: domain, and `max_file` only when the account carries a storage limit), the health body's `jetstream` block (bus, then the monitor's `/jsz`, then `jetstream_max_file` from config, then unknown), `nats_monitor_url` so a scratch monitor port can be read, and the verdict's aggregate rule: `stream_cap:<name>-agg` when the aggregate's cap claims 70 % (warn) or 90 % (critical) of `max_file`, one finding per stream, worst of claim and fill. Plan was wrong on one point: the account info answers `max_storage: -1` on a server without account limits, so the bus alone cannot read `max_file`; the monitor is the working source, config the fallback. Environmental catch: the laptop's live NATS answers `/leafz` and `/jsz` on :8222 with a leaf link, and the pre-existing H-06 spec `it_reports_not_connected` assumed nothing answers; it and the new config-fallback spec now point at a dead port. Deploy templates untouched (see Deploy diff below).
- `2026-09-25T20:13Z` F-02 red. `rs/tests/test_federation_domains.rs` (`#[ignore]`, three scratch nats-servers on 4510/6510/8510, 4511/8511, 4512/8512 from config files with `domain: hub|leaf-a|leaf-b`, two in-process nodes through `run_server`, a bare leaf bus for the second leaf). Already holding before any new code: the hub's `events-agg` has a source per leaf domain and each cursor moves (lag 0, count 3 → 5); the leaf's `events-mirror` fills from the hub; the leaf node lists the other leaf's session with the right count and its own once. Two plan adjustments: (1) the plan said two nats-servers; a second leaf is needed to show the mirror carrying *another* node's history, so three. (2) The sources doc's self-origin rule does not hold across the aggregate hop on nats-server 2.14.3: the mirror carries the leaf's own events back (5, not 3); the node's event-id dedup keeps the store honest, and the spec asserts that instead. Red for the right reason at step 3: health's `streams` carry no `sources`, `render_leaf_config` takes no domain, `node_streams` drops sources. `just test` on this branch: `cargo test` green (one flaky container test, `pi_mono_session_metadata_persisted`, passed alone), scripts and clippy green; the UI step fails on `tests/lib/fleet-map.test.ts` (TS7006), the C-07 red spec committed earlier on this branch, not this row.
- `2026-09-25T20:22Z` F-02 green: the lab passes twice (21.5 s, 21.6 s). Health streams carry `sources` (name, domain from `external.api`, lag, seconds active), `node_streams` passes them through with the node's `jetstream` block, and the managed NATS renders `domain:` (hub domain on a hub, host on a leaf or mesh device, none solo).
- `2026-09-25T20:30Z` The plan is wrong against the code for the Hetzner node, found reading the deploy repo (read-only): the box's `open-story` runs the image default, role `full`, because it watches Bobby and Katie. The CLI makes a node a hub only when its role is `consumer`, so `OPEN_STORY_HUB_DOMAIN=hub` on the box would make it a *leaf* pinned to `$JS.<its host>.API` against a server whose domain is `hub`: boot fails. And even as a hub, its own history (Bobby's, Katie's) sits in its `events` stream, which nothing sources onto `events-agg`, so leaves would get it only by the racy core propagation this plan retires. Smallest honest adjustment, F-02b: the node is the hub when its role is `consumer` *or* `OPEN_STORY_JETSTREAM_DOMAIN` equals `OPEN_STORY_HUB_DOMAIN`; a hub registers its own `events` and `presence` on its aggregates as same-domain sources filtered to `events.<host>.>` / `presence.<host>.>` (the filter keeps leaf events that reached the hub's `events.>` by core propagation from being sourced twice). Red: `own_source` / `ensure_own_source` (bus), `is_hub_node` (cli), and the lab publishing the hub's own session and asserting the leaf reads it by cursor.
- `2026-09-25T21:05Z` F-02b green (lab 20.7 s, 21.3 s, 20.8 s after the cap knob). F-01 completed with the aggregate's own cap knob, `OPEN_STORY_EVENTS_AGG_MAX_BYTES`, so the deploy env can carry what `fleet::aggregate_cap` computes. `just test` on this branch: the Rust step green apart from two Docker flakes that pass alone (`pi_mono_session_metadata_persisted`, `persist_consumer_receives_and_can_store_events`: testcontainers `PortNotExposed`), the four audit scripts green, workspace clippy green; the UI step still fails on the C-07 red specs (`tests/lib/fleet-map.test.ts`, `tests/components/fleet-map.test.tsx`), committed on this branch before this loop and untouched by it. Stopping here: F-02's lab is green; the roll (domains on the box and the laptop) is the owner's, through the Deploy diff above. Nothing deployed was touched: no ssh, no restart of :4222 or :3002, no merge.
