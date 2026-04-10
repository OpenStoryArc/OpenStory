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

**Local UI** (`localhost:3002`) — shows only your sessions.
**Common UI** (`https://<vps-tailscale-host>`) — shows all sessions from all machines.

Each leaf node works independently. If the VPS goes down, local sessions keep working. When it comes back, events forward automatically.

## How It Works

- Each machine runs a local NATS server configured as a **leaf node**
- Open Story publishes to `localhost:4222` — no code changes, no special config
- The leaf forwards messages to the hub's JetStream on port 7422 via Tailscale
- The hub persists all events from all machines
- Session IDs are UUIDs — globally unique, no namespace collisions

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

- **Tailscale** encrypts all traffic and authenticates devices at the network level
- **Token auth** on leaf connections prevents accidental cross-service connections
- **Port 7422** is bound to the Tailscale IP in Docker, unreachable from the public internet
- **ufw** on the VPS blocks all incoming traffic except SSH; Tailscale bypasses ufw at the iptables level
- Client port 4222 is localhost-only on both hub and leaf

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

### Local UI shows sessions but common UI doesn't

The leaf NATS forwards published messages to the hub. Check:
1. Leaf is connected (see above)
2. Hub's Open Story is subscribing to `events.>` (check logs)
3. Hub's JetStream stream has messages: `nats stream info events`

### Token mismatch

If the leaf logs show auth errors, verify the token in `nats-leaf.conf` URL matches the token in `nats-hub.conf` authorization block exactly.
