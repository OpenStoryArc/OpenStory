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
   - **Type**: CX22 (2 vCPU, 4GB RAM, 40GB SSD) — ~$5/mo
   - **SSH Key**: Add your public key
   - **Name**: `openstory` (or whatever you like)
4. Note the server's IPv4 address

## Step 2: Harden the Server

SSH in as root:

```bash
ssh root@<server-ip>
```

Create a non-root user:

```bash
adduser deploy
usermod -aG sudo deploy

# Copy SSH keys
mkdir -p /home/deploy/.ssh
cp ~/.ssh/authorized_keys /home/deploy/.ssh/
chown -R deploy:deploy /home/deploy/.ssh
chmod 700 /home/deploy/.ssh
chmod 600 /home/deploy/.ssh/authorized_keys
```

Lock down SSH:

```bash
sed -i 's/^PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
sed -i 's/^#PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
systemctl restart sshd
```

Firewall (allow SSH only — Tailscale handles the rest):

```bash
apt-get update && apt-get install -y ufw
ufw default deny incoming
ufw default allow outgoing
ufw allow ssh
ufw enable
```

## Step 3: Install Docker

```bash
apt-get install -y ca-certificates curl gnupg
install -m 0755 -d /etc/apt/keyrings
curl -fsSL https://download.docker.com/linux/debian/gpg \
  | gpg --dearmor -o /etc/apt/keyrings/docker.gpg

echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
  https://download.docker.com/linux/debian $(. /etc/os-release && echo $VERSION_CODENAME) stable" \
  > /etc/apt/sources.list.d/docker.list

apt-get update
apt-get install -y docker-ce docker-ce-cli containerd.io docker-compose-plugin
usermod -aG docker deploy
```

## Step 4: Install Tailscale

```bash
curl -fsSL https://tailscale.com/install.sh | sh
tailscale up
```

Follow the auth URL to add this VPS to your tailnet. Also install Tailscale on your phone and laptop.

Get your MagicDNS hostname:

```bash
tailscale status
# Example output: "openstory" — full hostname: openstory.your-tailnet.ts.net
```

Generate TLS certs:

```bash
tailscale cert openstory.your-tailnet.ts.net
# Certs at: /var/lib/tailscale/certs/
```

## Step 5: Install Caddy

```bash
apt-get install -y debian-keyring debian-archive-keyring apt-transport-https
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
  | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
  | tee /etc/apt/sources.list.d/caddy-stable.list
apt-get update && apt-get install -y caddy
```

## Step 6: Create Telegram Bot

1. Open Telegram, message [@BotFather](https://t.me/BotFather)
2. Send `/newbot`
3. Choose a name (e.g., "My OpenClaw Agent")
4. Choose a username (e.g., `my_openclaw_bot`)
5. Save the **bot token** BotFather gives you

Get your Telegram user ID:
1. Message [@userinfobot](https://t.me/userinfobot) on Telegram
2. It replies with your user ID (a number like `123456789`)

## Step 7: Deploy the Stack

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

Build the images:

```bash
# Open Story (server + UI)
docker build -f Dockerfile.prod -t open-story:prod .

# OpenClaw (from your local checkout)
# If openclaw source is at ~/projects/openclaw:
docker build -t openclaw:latest ~/projects/openclaw
```

Start the stack:

```bash
docker compose -f docker-compose.prod.yml up -d
```

## Step 8: Configure Caddy

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

## Step 9: Verify

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
| Hetzner CX22 | ~$5 |
| Tailscale | Free (personal) |
| Telegram bot | Free |
| Anthropic API | Usage-based (~$0.30-6.75/session) |
| **Total hosting** | **~$5/mo** + API usage |

## Troubleshooting

**Bot not responding**: Check `docker compose logs telegram-bot`. Verify `TELEGRAM_BOT_TOKEN` and `TELEGRAM_ALLOWED_USER_ID` in `.env`.

**Open Story not showing sessions**: Check that OpenClaw is writing JSONL files. Run `docker compose exec openclaw find /home/node/.openclaw -name "*.jsonl"`. Verify `OPEN_STORY_PI_WATCH_DIR` is set.

**Can't reach dashboard**: Verify Tailscale is connected on both devices (`tailscale status`). Check Caddy logs (`journalctl -u caddy`).

**TLS errors**: Regenerate certs with `tailscale cert <hostname>` and `systemctl reload caddy`.
