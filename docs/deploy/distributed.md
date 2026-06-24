# Distributed Streaming with NATS Leaf Nodes

Stream OpenStory events from multiple machines to a central dashboard via NATS and Tailscale.

## What sharing means (and what it doesn't)

Read this before deploying. The model is deliberately simple, and the simplicity is the honest part.

**OpenStory does not enforce per-session or per-person sharing permissions — and it cannot.** There is no "private session," no share toggle, no owner-consent, no per-session ACL. Once an event is published to the NATS bus, every connected node receives a full copy of it; there is no point at which the app could withhold it from a reader who is already on the bus. An earlier design tried to gate this per session and was removed because it could not be enforced.

**Sharing means joining a network. Access is enforced at the network, not the session — by two layers, both outside the app:**

1. **Tailscale membership** — who can reach the NATS leaf/hub port at all. If a machine isn't on your tailnet, it can't connect.
2. **NATS token / accounts** — who can connect once reachable, and which subjects a credential is allowed to publish or subscribe to.

You secure the network, not the session. **Within a network, propagation is bidirectional:** every connected node ends up with a full copy of what other nodes publish. If you're on the bus, you see what's published to it.

**The one node-level switch: `publish_sessions`** (config `publish_sessions`, env `OPEN_STORY_PUBLISH_SESSIONS`, default `true`).

| `publish_sessions` | This node's own observed sessions | Other nodes' sessions |
|---|---|---|
| `true` (default) | Published into the network **and** stored locally | Received and visible |
| `false` | Stored **locally only** — never leave the machine | Still received and visible |

This is a node-level switch over your *own* data. It is **not** per-session and **not** a permission enforced on readers — it decides whether this machine puts its sessions on the bus in the first place. (Implementation: local-only events go to a `local.>` NATS stream that federation never sources. The `nats_leaf_url` / hub config remains the separate "do I join a network at all" switch.)

**The honest guidance:** do not put a node on a network with people who shouldn't see its sessions. If a machine has sensitive sessions, run it solo (no `nats_leaf_url`) or set `publish_sessions = false`. The Admin dashboard is read-only — it observes topology and the fleet; it changes nothing.

## Architecture

```
Your Mac                    Hetzner VPS (Hub)              Friend's Mac
┌──────────────┐           ┌──────────────────┐           ┌──────────────┐
│ Claude Code  │           │ OpenClaw         │           │ Claude Code  │
│     ↓        │           │     ↓            │           │     ↓        │
│ Open Story   │           │ Open Story       │           │ Open Story   │
│ (local UI)   │           │ (common UI)      │           │ (local UI)   │
│     ↓        │           │     ↓            │           │     ↓        │
│ NATS leaf    │──Tailscale──→ NATS hub ←──Tailscale──│ NATS leaf    │
│ :4222        │           │ :4222 + :7422    │           │ :4222        │
└──────────────┘           └──────────────────┘           └──────────────┘
```

**Local UI** (`localhost:3002`) — full mirror of all team activity.
**Common UI** (`https://<vps-tailscale-host>`) — same data, served from the VPS.

Each leaf node works independently. If the VPS goes down, your local Open Story keeps working with everything you'd already received. When the VPS comes back, events catch up automatically.

## How It Works

- Each machine runs a local NATS server configured as a **leaf node**
- Open Story publishes to `localhost:4222` — no code changes, no special config
- The leaf forwards messages to the hub's JetStream on port 7422 via Tailscale
- The hub persists all events from all machines
- Session IDs are UUIDs — globally unique, no namespace collisions

### Bidirectional Propagation (Important!)

NATS leaf nodes with JetStream propagate streams **bidirectionally**. When alice publishes a session on her leaf, it flows to the hub *and* to bob's leaf. Each machine eventually has a complete copy of all team activity in its local SQLite + JSONL backup.

This is intentional — it gives every team member full sovereignty over their data without depending on the VPS being up. Your local Open Story is a full mirror, not a partial view. The "common UI" on the VPS is just another instance of the same data.

**Implications:**

- **Storage**: every machine stores all events. For small teams that's fine; large teams should consider NATS accounts for partitioning (see Backlog).
- **Privacy**: every machine can read every other machine's session content (prompts, file contents, tool outputs). Tailscale + token auth prevents outsiders from reading, but team members can read each other.
- **Resilience**: any machine going offline doesn't affect the others. When it reconnects, NATS catches it up.
- **Search**: when you search locally, you're searching across all machines' sessions, not just yours.

## Subject naming & upgrading

Federation events use a **host-prefixed** NATS subject:

```
events.{host}.{project}.{session}.main           # main agent
events.{host}.{project}.{session}.agent.{id}     # subagent
```

The leading `{host}` segment is what makes the hub aggregate safe: each leaf binds
only its own `events.{host}.>` namespace, so a session two machines both hold is
never counted twice. (Session IDs are UUIDs and don't collide — but the
*aggregate* still needs per-host scoping to avoid double-delivery.)

When a node runs with `publish_sessions = false`, its own observed sessions are
written to a separate **`local.>`** stream instead of `events.{host}.>`.
Federation only sources `events.{host}.>`, so `local.>` never leaves the machine —
it's the storage path for sessions you keep off the network. That node still
subscribes to `events.>` and receives every other node's sessions normally.

This schema changed: earlier builds published `events.{project}.{session}.…` with
no host. If you're upgrading an existing deployment, here's what is and isn't
affected.

**Your stored data is safe — there is no data migration.** The subject is a
transport/routing detail; it is never persisted. JSONL backups and the
SQLite/Mongo event store key on event *content*, not on the NATS subject, so
nothing on disk is re-keyed or rewritten.

**What you do need to know:**

- **Upgrade a fleet together.** A node on the old build publishes
  `events.{project}.…` (no host); a hub on the new build binds `events.{host}.>`
  and won't see those events. Don't run a mixed-version fleet — upgrade every leaf
  and the hub in the same window.
- **Existing JetStream streams.** A standalone instance's `events` stream was
  created with the host-agnostic filter `events.>`, which still matches the new
  host-prefixed subjects, so a **solo upgrade just works**. Federated mode adds
  per-host / aggregate streams; if a stream already exists with a different
  subject binding, JetStream refuses to silently reconfigure it — delete it and
  let Open Story recreate it (`nats stream rm events` on the affected node), or
  start the hub on a fresh JetStream store.
- **Custom subscribers.** Internal consumers already subscribe to the
  host-agnostic `events.>`, so they're unaffected. Any *hand-rolled* external
  subscriber pinned to `events.{project}.>` will silently stop matching — repoint
  it to `events.>` or `events.{host}.>`.

Beyond clearing/recreating any pre-existing stream whose subject binding changed
and upgrading all nodes together, there's nothing else to do.

## How to Use

### For humans

Once your leaf is running and connected to the hub:

- **Local dashboard**: open `http://localhost:3002` in a browser — your full view of team activity, updated in real time
- **Common dashboard**: open `https://<vps-tailscale-hostname>` from any device on your tailnet — same data, served from the VPS (useful from a phone or a machine without Open Story)
- **Search**: the search box queries across all sessions from all machines locally — no network call to the VPS
- **Filter by your work**: there's no built-in machine filter yet (see Backlog: NATS accounts for team partitioning), but session IDs are stable across machines, so you can bookmark URLs

### For agents (via MCP)

Open Story exposes 19 read-only MCP tools for agent self-awareness. With distributed streaming, agents can query *any* instance — local or remote — depending on what they need.

Two MCP servers are configured in `.mcp.json`:

| Server | Endpoint | Use when |
|--------|----------|----------|
| `openstory` | `localhost:3002` | Querying your own machine — fastest, always current |
| `openstory-remote` | VPS hub | Querying the team view — same data, useful when you want to be explicit about scope |

Both expose the same 19 tools (`list_sessions`, `session_synopsis`, `tool_journey`, `search`, `agent_search`, `token_usage`, etc.). With bidirectional propagation, both return the same results — pick whichever is conceptually clearest for your task.

**Setup on a new machine:**

```bash
# Set the remote URL in your shell environment
export OPENSTORY_REMOTE_URL=http://<vps-tailscale-hostname>:3002
```

Then restart Claude Code (or your agent runner) so it picks up the MCP server registration.

**Calling the tools:**

```
mcp__openstory__list_sessions          # local instance
mcp__openstory_remote__list_sessions   # remote/hub instance
```

The server name appears in the tool name, so the agent always knows which instance it's hitting. The MCP server's `OPENSTORY_LABEL` env var also makes the instance identity visible in the server description.

## Setup

### Prerequisites

- [Tailscale](https://tailscale.com/) installed on all machines, same tailnet
- Hub VPS set up per [hetzner.md](hetzner.md)

### 1. Generate a shared token

On any machine:

```bash
openssl rand -hex 24
```

Save this token — it goes in both the hub and leaf configs.

### 2. Configure the hub (VPS)

Edit `deploy/nats-hub.conf` on the VPS. Replace the placeholder token:

```
leafnodes {
    listen: "0.0.0.0:7422"
    authorization {
        token: "<your-generated-token>"
    }
}
```

Add to your `.env` on the VPS:

```bash
NATS_LEAF_TOKEN=<your-generated-token>
TAILSCALE_IP=<your VPS Tailscale IP, e.g. 100.64.0.1>
```

Start the stack:

```bash
docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml up -d
```

Verify NATS is running:

```bash
curl -s http://localhost:8222/varz | jq '.leafnodes'
```

### 3. Configure a leaf node (local machine)

A leaf node is just Open Story's managed NATS pointed at the hub. **Networking is
off by default** — a fresh install is single-machine and loopback-only. One
setting turns it on: `nats_leaf_url` (config.toml) or `OPEN_STORY_NATS_LEAF_URL`
(env). Set it to your hub URL and `--manage-nats` launches NATS as a JetStream
leaf instead of a standalone server; leave it empty to stay local.

The URL format is `nats://<your-token>@<vps-tailscale-hostname>:7422`. The
Tailscale hostname is your VPS's MagicDNS name (e.g., `debian-16gb-ash-1`) or its
Tailscale IP (e.g., `100.64.0.1`).

**Keeping this node's sessions off the network.** Joining a network publishes
this node's observed sessions to it by default. To join the network (so you
*receive* everyone else's sessions) but keep your *own* sessions on this machine,
set `publish_sessions = false`:

```bash
# In config.toml:
#   publish_sessions = false
# …or via the environment:
OPEN_STORY_PUBLISH_SESSIONS=false
```

With this set, your sessions are stored locally only (in the `local.>` stream) and
never reach the hub or other leaves; you still see all of theirs. See
[What sharing means](#what-sharing-means-and-what-it-doesnt) above for why this is
a node-level switch and not a per-session permission.

#### Option A: Homebrew, single command (recommended for Mac)

The brew service already runs `serve --manage-nats`. Just give it a hub URL — no
separate `nats-server` process, no hand-written config; Open Story generates the
leaf config and supervises the child for you:

```bash
# In $(brew --prefix)/var/openstory/config.toml:
#   nats_leaf_url = "nats://<your-token>@debian-16gb-ash-1:7422"
# …or pass it via the environment instead:
OPEN_STORY_NATS_LEAF_URL="nats://<your-token>@debian-16gb-ash-1:7422" \
  brew services restart openstory
```

To go back to single-machine, clear the setting and restart — the managed NATS
returns to loopback-only.

#### Option B: Standalone NATS + native Open Story (for dev checkouts)

```bash
brew install nats-server
nats-server -c deploy/nats-leaf.conf &          # edit the remote URL first
cd rs && cargo run -p open-story-cli -- serve   # defaults to nats://localhost:4222
```

#### Option C: Docker Compose

```bash
docker compose -f docker-compose.leaf.yml up -d
```

### 4. Add a friend's machine

1. Share your Tailscale network with them (Tailscale admin console → Share a node, or use `tailscale share`)
2. Give them your `deploy/nats-leaf.conf` with the token and hostname filled in
3. They install NATS + Open Story and run the leaf config

They'll see their own sessions locally at `localhost:3002`, and their events appear on the common dashboard.

## Ports

| Port | Purpose | Exposed to |
|------|---------|------------|
| 4222 | NATS client connections | localhost only |
| 7422 | NATS leaf node connections | Tailscale network (via `TAILSCALE_IP` binding) |
| 8222 | NATS HTTP monitoring | localhost only |
| 3002 | Open Story API + UI | localhost (leaf) or Tailscale via Caddy (hub) |

## Security

### What's protected

- **Tailscale** encrypts all traffic and authenticates devices at the network level
- **Token auth** on leaf connections prevents accidental cross-service connections
- **Port 7422** is bound to the Tailscale IP in Docker, unreachable from the public internet
- **ufw** on the VPS blocks all incoming traffic except SSH; Tailscale bypasses ufw at the iptables level
- Client port 4222 is localhost-only on both hub and leaf
- **Federation is pinned to the tailnet** — the hub advertises *only* its Tailscale address (`leafnodes { advertise: "<hub-tailnet-addr>:7422" }`). Without this, NATS gossips the hub's other interfaces to the leaf via `connect_urls`, and a severed tailnet lets the leaf reconnect over *any* shared path (LAN, VPC, docker bridge) — bypassing the Tailscale ACL entirely. This was found and fixed by the falsifiable ablation in `docs/research/tailnet-federation/VALIDATION.md`; set it on every hub.

### Known limitations

For close friends and small trusted teams, the current setup is fine. For larger teams or sensitive workloads, be aware:

- **No per-user isolation**: every machine sees every machine's sessions in plaintext (a consequence of bidirectional propagation). NATS accounts would fix this — see Backlog: "Distributed Deployment Security Hardening."
- **Single shared token**: rotating it requires updating every leaf config. Credential files would help.
- **JSONL backups are unencrypted**: every machine has plaintext team data on disk. SQLCipher (already supported via `db_key` config) protects the database but not the JSONL fallback.
- **No read auditing**: you know who *published* a session but not who *read* it.
- **Token shows up in process listings**: `nats://TOKEN@host:port` is visible in `ps`, `docker inspect`, and Open Story's startup log.

The Backlog item "Distributed Deployment Security Hardening" tracks the fixes for each of these.

## Troubleshooting

### Leaf can't connect to hub

```bash
# Check Tailscale connectivity
tailscale ping <vps-tailscale-hostname>

# Check NATS is listening on the hub
ssh deploy@<vps-ip> 'curl -s http://localhost:8222/varz | jq .leafnodes'

# Check leaf connection status
curl -s http://localhost:8222/leafz | jq .
```

### Events not appearing on common dashboard

```bash
# Check leaf is connected
curl -s http://localhost:8222/leafz | jq '.leafs[].connected'

# Check hub sees the leaf
ssh deploy@<vps-ip> 'curl -s http://localhost:8222/leafz | jq .'

# Verify events are in the hub's JetStream
ssh deploy@<vps-ip> 'nats stream info events'
```

### One instance shows sessions but another doesn't

With bidirectional propagation, every connected node should converge to the same view. If they don't:

1. Leaf is connected: `curl -s http://localhost:8222/leafz | jq '.leafs[].subscriptions'` (subscriptions > 0 means interest is propagating)
2. The Open Story instance that's missing data is subscribing to `events.>`: check its startup logs for `NATS bus: nats://...` (vs the warning `Falling back to local mode (no bus)`)
3. The instance's JetStream stream exists: `docker exec <container> sh -c 'echo done'` and check NATS monitoring at `:8222/jsz`

A common cause: `ensure_streams()` failing because the JetStream `max_file` config is smaller than the 1GB the `events` stream wants. Check NATS startup logs for storage errors.

### Token mismatch

If the leaf logs show auth errors, verify the token in `nats-leaf.conf` URL matches the token in `nats-hub.conf` authorization block exactly.

## Verification & Reference Tests

The full distributed deployment is exercised by integration tests using testcontainers. These tests are **living reference documentation** — if you're not sure how a deployment state should behave, read the corresponding test.

Build the test image first:

```bash
docker build -f rs/Dockerfile -t open-story:test rs/
```

Then run the tests (they're `#[ignore]`d by default since they need Docker):

| Test file | Compose file | What it proves |
|-----------|--------------|----------------|
| `rs/tests/test_leaf_cluster.rs` | `docker-compose.leafcluster.yml` | Single leaf forwards to hub; hub has full view records |
| `rs/tests/test_multi_leaf.rs` | `docker-compose.multileaf.yml` | Two leaves (alice, bob) aggregate on hub; both have at least their own sessions |
| `rs/tests/test_deployment_states.rs` | All three | The deployment state machine: solo → solo+VPS → team → team+guests |

Run individual states:

```bash
# Just the solo local case
cargo test -p open-story --test test_deployment_states -- --include-ignored state_solo_local

# The full state machine, serially (parallel exhausts Docker resources)
cargo test -p open-story --test test_deployment_states -- --include-ignored --test-threads=1
```

The test compose files are also useful as **deployment templates** if you're setting up a real cluster — they're the simplest possible working configurations of each state.

### Deployment State Machine

```text
  ┌──────────────┐
  │  Solo Local   │  One machine, file watcher, no NATS
  └──────┬───────┘
         │ add VPS hub
  ┌──────▼───────┐
  │  Solo + VPS   │  One leaf + hub, sessions stream to central dashboard
  └──────┬───────┘
         │ add teammate's leaf
  ┌──────▼───────┐
  │  Team Hub     │  Multiple leaves + hub, every node is a full mirror
  └──────┬───────┘
         │ add read-only viewer
  ┌──────▼───────┐
  │ Team + Guests │  Team + viewer (no leaf, connects to hub NATS directly)
  └──────────────┘
```

Each transition is a configuration change — no code changes, no migrations. Move forward (add machines) or backward (shut some down) without affecting the others.
