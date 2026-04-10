# Distributed Streaming with NATS Leaf Nodes

Stream OpenStory events from multiple machines to a central dashboard via NATS and Tailscale.

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
docker compose -f docker-compose.prod.yml up -d
```

Verify NATS is running:

```bash
curl -s http://localhost:8222/varz | jq '.leafnodes'
```

### 3. Configure a leaf node (local machine)

Edit `deploy/nats-leaf.conf`. Replace the placeholders:

```
leafnodes {
    remotes [
        {
            url: "nats://<your-token>@<vps-tailscale-hostname>:7422"
        }
    ]
}
```

The Tailscale hostname is your VPS's MagicDNS name (e.g., `debian-16gb-ash-1`) or its Tailscale IP (e.g., `100.64.0.1`).

#### Option A: Native NATS + native Open Story (recommended for Mac)

```bash
# Install NATS
brew install nats-server

# Start leaf node
nats-server -c deploy/nats-leaf.conf &

# Start Open Story (it defaults to nats://localhost:4222)
cd rs && cargo run -p open-story-cli -- serve
```

#### Option B: Docker Compose

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
