# Deploy OpenClaw + Open Story on Hetzner VPS

Deploy a personal coding agent you can message from Telegram, with real-time observability via Open Story.

## Architecture

```
Telegram (phone/laptop)
  |
  v
Telegram Bot (Python, HTTP bridge)
  |
  v  POST /v1/chat/completions
OpenClaw gateway (:18789)     -- coding agent, writes JSONL sessions
  |
  v (shared Docker volume)
Open Story server (:3002)     -- observes, stores, broadcasts
  |                               |
  +-- Dashboard UI                v
  +-- REST API              NATS hub (:4222 local, :7422 leaf nodes)
  +-- WebSocket (live)        |
                              +-- leaf nodes from other machines (via Tailscale)

Caddy reverse proxy on Tailscale IP (HTTPS, private network)
```

For distributed streaming (multiple machines), see [distributed.md](distributed.md).

## Prerequisites

- Hetzner Cloud account
- Tailscale account (free tier works — no TLS certs on free tier)
- Telegram account
- Anthropic API key
- SSH key pair (`ssh-keygen -t ed25519` if you don't have one)

## Step 1: Create the VPS

1. Log into [Hetzner Cloud Console](https://console.hetzner.cloud)
2. Create a new project (or use existing)
3. Add Server:
   - **Location**: Any (US Ashburn/West, or nearest to you)
   - **Image**: Debian 13 (or 12)
   - **Type**: CX32 (4 vCPU, 8GB RAM) minimum. CX42 (8 vCPU, 16GB) recommended for coding on the box.
   - **SSH Key**: Add your public key (paste output of `cat ~/.ssh/id_ed25519.pub`)
   - **Name**: `openstory` (or whatever you like)
4. Note the server's IPv4 address

**Important**: You must add your SSH key during server creation. If you skip this, Hetzner emails a root password instead and you'll need to set one up manually.

## Step 2: Run the setup script

From your local machine:

```bash
scp scripts/deploy-vps.sh root@<server-ip>:
ssh root@<server-ip> bash deploy-vps.sh
```

This takes ~5 min and installs: Docker, Tailscale, Caddy, Rust, Node, dev tools, firewall, deploy user with your SSH key.

**Note**: The script does NOT lock down SSH. Do that manually after confirming you can SSH in as the deploy user.

## Step 3: Set up Tailscale

Still as root on the server:

```bash
tailscale up
```

Open the URL it prints in your browser to authorize the device. Install Tailscale on your phone/laptop too so they're on the same private network.

Get the Tailscale IP (you'll need this for Caddy):

```bash
tailscale ip -4
```

**Note**: Tailscale free tier does NOT support TLS certs. We use plain HTTP over the encrypted Tailscale network instead.

## Step 4: Create Telegram Bot

1. Open Telegram, message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a name and username (username must end in `bot`)
4. Save the **bot token** BotFather gives you

Get your Telegram user ID:
1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. It replies with your user ID (a number like `123456789`)

To allow multiple users, collect each person's user ID the same way.

## Step 5: Clone repos and configure

SSH in as the deploy user:

```bash
ssh deploy@<server-ip>
cd ~/openstory
git clone https://github.com/OpenStoryArc/OpenStory.git .
git checkout fix/deployment-updates
git clone https://github.com/openclaw/openclaw.git ~/openclaw
```

Create `.env` using nano (heredocs break over SSH — use nano):

```bash
nano .env
```

Paste these lines with your real values (no quotes, no leading spaces, no line wrapping):

```
ANTHROPIC_API_KEY=sk-ant-your-key-here
OPEN_STORY_API_TOKEN=
TELEGRAM_BOT_TOKEN=123456:ABC-your-token-here
TELEGRAM_ALLOWED_USER_IDS=123456789,987654321
OPENCLAW_AUTH_TOKEN=pick-any-random-secret-string
```

Save with `Ctrl+O`, enter, `Ctrl+X`. Then:

```bash
chmod 600 .env
```

### .env reference

| Variable | Required | Description |
|----------|----------|-------------|
| `ANTHROPIC_API_KEY` | Yes | Your Anthropic API key |
| `OPEN_STORY_API_TOKEN` | No | Leave empty to disable auth (safe on private network) |
| `TELEGRAM_BOT_TOKEN` | Yes | From BotFather |
| `TELEGRAM_ALLOWED_USER_IDS` | Yes | Comma-separated Telegram user IDs |
| `OPENCLAW_AUTH_TOKEN` | Yes | Shared secret between OpenClaw and the Telegram bot |
| `NATS_LEAF_TOKEN` | No | Token for remote NATS leaf node auth (see [distributed.md](distributed.md)) |
| `TAILSCALE_IP` | No | Tailscale IP to bind NATS leaf port (e.g., `100.64.0.1`) |

## Step 6: Build images

Install the Docker BuildKit plugin (required for OpenClaw):

```bash
sudo apt-get install -y docker-buildx-plugin
```

Build both images (OpenClaw takes ~4 min, Open Story takes ~10 min):

```bash
docker build -t openclaw:latest ~/openclaw
docker build -f Dockerfile.prod -t open-story:prod .
```

**Warning**: Docker builds create millions of temporary files that consume filesystem inodes. If you see "no space left on device" errors but `df -h` shows free space, check `df -i /` for inode exhaustion. Fix with `docker system prune -a --force`.

## Step 7: Launch the stack

```bash
docker compose -f docker-compose.prod.yml up -d
```

Verify all three containers are healthy:

```bash
docker compose -f docker-compose.prod.yml ps
```

You should see `openclaw`, `open-story`, `nats`, and `telegram-bot` all running.

## Step 8: Configure Caddy

As root (or with sudo), edit the Caddy config:

```bash
sudo nano /etc/caddy/Caddyfile
```

Replace the entire contents with (use your Tailscale IP from Step 3):

```
http://<tailscale-ip> {
    reverse_proxy localhost:3002
}
```

Save and restart:

```bash
sudo systemctl restart caddy
```

## Step 9: Verify

**Telegram**: Send a message to your bot from your phone. You should get a response from the coding agent.

**Dashboard**: From any device on your Tailscale network, open `http://<tailscale-ip>` in a browser. You should see the agent's session appear in real time.

**API**:

```bash
curl http://<tailscale-ip>/api/sessions
```

## Step 10: Give OpenClaw access to Open Story

The agent can query its own session history via the Open Story API. On the server:

```bash
docker exec openstory-openclaw-1 sh -c 'cat > /home/node/.openclaw/workspace/TOOLS.md << TOOLEOF
# Open Story API

Open Story watches everything you do. Query your history:

  curl http://open-story:3002/api/sessions
  curl http://open-story:3002/api/sessions/{id}/records
TOOLEOF'
```

Or just tell the bot: "There's an Open Story server at http://open-story:3002 — try curling /api/sessions"

For full docs, have the agent clone the repo into its workspace:

```
Clone https://github.com/OpenStoryArc/OpenStory.git into /home/node/.openclaw/workspace/openstory and read CLAUDE.md
```

## Operations

### View logs

```bash
cd ~/openstory
docker compose -f docker-compose.prod.yml logs -f
docker compose -f docker-compose.prod.yml logs -f openclaw
docker compose -f docker-compose.prod.yml logs -f open-story
docker compose -f docker-compose.prod.yml logs -f telegram-bot
```

### Update Open Story

```bash
cd ~/openstory
git pull
docker build -f Dockerfile.prod -t open-story:prod .
docker compose -f docker-compose.prod.yml up -d
```

### Update OpenClaw

```bash
cd ~/openclaw
git pull
docker build -t openclaw:latest .
cd ~/openstory
docker compose -f docker-compose.prod.yml up -d
```

### Backup

```bash
docker compose -f docker-compose.prod.yml exec open-story \
  tar czf - /data > backup-$(date +%Y%m%d).tar.gz
```

### Restart

```bash
docker compose -f docker-compose.prod.yml restart
```

### Check resources

```bash
docker stats        # container CPU/memory
df -h               # disk space
df -i /             # inodes (if builds are failing)
```

### Add a Telegram user

Edit `.env`, add their user ID to the comma-separated list:

```
TELEGRAM_ALLOWED_USER_IDS=123456789,987654321,111222333
```

Then restart the bot:

```bash
docker compose -f docker-compose.prod.yml up -d --no-deps telegram-bot
```

## Cost Summary

| Item | Monthly Cost |
|------|-------------|
| Hetzner CX42 (16GB) | ~$16 |
| Tailscale | Free (personal) |
| Telegram bot | Free |
| Anthropic API | Usage-based (~$0.30-6.75/session) |
| **Total hosting** | **~$16/mo** + API usage |

## Troubleshooting

**Bot not responding**: `docker compose logs telegram-bot`. Check `TELEGRAM_BOT_TOKEN` and `TELEGRAM_ALLOWED_USER_IDS` in `.env`. Verify `OPENCLAW_AUTH_TOKEN` matches in both the bot env and the OpenClaw config.

**OpenClaw unhealthy / 100% CPU**: Check `docker logs openstory-openclaw-1`. If empty, the entrypoint may be wrong — must use `openclaw.mjs` not `dist/index.js`. Also check inodes: `df -i /`.

**"No space left on device" but disk has space**: Inode exhaustion. Run `df -i /`. Fix with `docker system prune -a --force`. A runaway OpenClaw container can create millions of files in its state volume — `docker volume rm openstory_openclaw-state` to clear (loses agent memory).

**Open Story not showing sessions**: `docker compose logs open-story`. Verify OpenClaw is writing JSONL: `docker compose exec openclaw find /home/node/.openclaw -name "*.jsonl"`.

**Can't reach dashboard**: Verify Tailscale is connected on both devices (`tailscale status`). Check Caddy config points to your Tailscale IP. Check `journalctl -u caddy`.

**401 on API/WebSocket**: `OPEN_STORY_API_TOKEN` is set but the UI doesn't send it. Either clear the token (safe on private Tailscale network) or use bearer auth for API calls: `curl -H "Authorization: Bearer <token>" http://...`.

**OpenClaw auth token changes on restart**: Pin the token in the compose config via `OPENCLAW_AUTH_TOKEN` in `.env`. The compose file writes it into the OpenClaw config JSON on startup.
