# Deploy OpenClaw + Open Story on Hetzner VPS

Deploy a personal coding agent you can message from Telegram, with real-time observability via Open Story.

## Architecture

```
Telegram (phone/laptop)
  |
  v
Telegram Bot (Python bridge)
  |
  v
OpenClaw gateway (:18789)     -- coding agent, writes JSONL sessions
  |
  v (shared Docker volume)
Open Story server (:3002)     -- observes, stores, broadcasts
  |
  +-- Dashboard UI
  +-- REST API
  +-- WebSocket (live stream)

Caddy reverse proxy + Tailscale VPN (no public exposure)
```

## Prerequisites

- Hetzner Cloud account
- Tailscale account (free tier is fine)
- Telegram account
- Anthropic API key
- SSH key pair

## Step 1: Create the VPS

1. Log into [Hetzner Cloud Console](https://console.hetzner.cloud)
2. Create a new project (or use existing)
3. Add Server:
   - **Location**: Ashburn, VA (or nearest to you)
   - **Image**: Debian 12
   - **Type**: CCX33 (8 dedicated vCPU, 32GB RAM, 240GB SSD) — ~$55/mo. Or CCX43 (16 vCPU, 64GB) for more power.
   - **SSH Key**: Add your public key
   - **Name**: `openstory` (or whatever you like)
4. Note the server's IPv4 address

## Step 2: Run the setup script

The setup script installs everything: Docker, Tailscale, Caddy, Rust, Node, dev tools, firewall, deploy user.

```bash
ssh root@<server-ip>
# Copy the script (or paste it):
scp scripts/deploy-vps.sh root@<server-ip>:
bash deploy-vps.sh
```

This takes ~5 min and handles Steps 2-5 from the original manual process.

After it finishes:

```bash
# Still as root:
tailscale up          # Authorize in browser
tailscale cert <hostname>.ts.net
```

## Step 3: Create Telegram Bot

1. Open Telegram, message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a name (e.g., "My OpenClaw Agent")
4. Choose a username (e.g., `my_openclaw_bot`)
5. Save the **bot token** BotFather gives you

Get your Telegram user ID:
1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. It replies with your user ID (a number like `123456789`)

## Step 4: Deploy the Stack

Switch to the deploy user and set up the project:

```bash
su - deploy
mkdir -p ~/openstory && cd ~/openstory
```

Clone the repo (or copy the compose files):

```bash
git clone https://github.com/OpenStoryArc/OpenStory.git .
```

Create `.env` with your secrets:

```bash
cat > .env << 'EOF'
ANTHROPIC_API_KEY=sk-ant-your-key-here
OPEN_STORY_API_TOKEN=change-me-to-a-random-string
OPEN_STORY_ALLOWED_ORIGINS=https://openstory.your-tailnet.ts.net
TELEGRAM_BOT_TOKEN=123456:ABC-DEF1234ghIkl-zyx57W2v1u123ew11
TELEGRAM_ALLOWED_USER_ID=123456789
EOF
chmod 600 .env
```

Build both images in parallel (use tmux or background jobs):

```bash
docker build -t openclaw:latest ~/openclaw &
docker build -f Dockerfile.prod -t open-story:prod . &
wait
```

Start the stack:

```bash
docker compose -f docker-compose.prod.yml up -d
```

## Step 5: Configure Caddy

As root:

```bash
cp /home/deploy/openstory/Caddyfile /etc/caddy/Caddyfile
```

Set the hostname environment variable:

```bash
echo 'CADDY_HOSTNAME=openstory.your-tailnet.ts.net' >> /etc/default/caddy
systemctl restart caddy
```

Set up monthly cert renewal:

```bash
cat > /etc/cron.monthly/tailscale-cert << 'CRON'
#!/bin/sh
tailscale cert openstory.your-tailnet.ts.net
systemctl reload caddy
CRON
chmod +x /etc/cron.monthly/tailscale-cert
```

## Step 6: Verify

From your phone or laptop (on the same Tailscale network):

**Telegram**: Send a message to your bot. You should get a response from the coding agent.

**Dashboard**: Open `https://openstory.your-tailnet.ts.net/` in a browser. You should see the agent's session appear.

**API**:

```bash
curl -H "Authorization: Bearer <your-api-token>" \
  https://openstory.your-tailnet.ts.net/api/sessions
```

**WebSocket**: Connect to `wss://openstory.your-tailnet.ts.net/ws` for live event streaming.

## Operations

### View logs

```bash
cd ~/openstory
docker compose -f docker-compose.prod.yml logs -f
docker compose -f docker-compose.prod.yml logs -f open-story
docker compose -f docker-compose.prod.yml logs -f telegram-bot
```

### Update

```bash
cd ~/openstory
git pull
docker build -f Dockerfile.prod -t open-story:prod .
docker compose -f docker-compose.prod.yml up -d
```

### Backup

```bash
# Copy the data volume (SQLite + JSONL)
docker compose -f docker-compose.prod.yml exec open-story \
  tar czf - /data > backup-$(date +%Y%m%d).tar.gz
```

### Restart

```bash
docker compose -f docker-compose.prod.yml restart
```

### Resource monitoring

```bash
docker stats
```

## Cost Summary

| Item | Monthly Cost |
|------|-------------|
| Hetzner CCX33 (32GB) | ~$55 |
| Tailscale | Free (personal) |
| Telegram bot | Free |
| Anthropic API | Usage-based (~$0.30-6.75/session) |
| **Total hosting** | **~$55/mo** + API usage |

## Troubleshooting

**Bot not responding**: Check `docker compose logs telegram-bot`. Verify `TELEGRAM_BOT_TOKEN` and `TELEGRAM_ALLOWED_USER_ID` in `.env`.

**Open Story not showing sessions**: Check that OpenClaw is writing JSONL files. Run `docker compose exec openclaw find /home/node/.openclaw -name "*.jsonl"`. Verify `OPEN_STORY_PI_WATCH_DIR` is set.

**Can't reach dashboard**: Verify Tailscale is connected on both devices (`tailscale status`). Check Caddy logs (`journalctl -u caddy`).

**TLS errors**: Regenerate certs with `tailscale cert <hostname>` and `systemctl reload caddy`.
