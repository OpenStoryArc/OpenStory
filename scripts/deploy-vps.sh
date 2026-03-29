#!/usr/bin/env bash
# One-shot VPS setup: OpenClaw + Open Story + dev environment.
#
# Designed for Hetzner CCX33 (32GB) or CCX43 (64GB) dedicated CPU boxes.
# Installs everything: Docker, Tailscale, Caddy, dev tools, Rust, Node.
#
# Run as root on a fresh Debian 12 VPS:
#   scp scripts/deploy-vps.sh root@<ip>: && ssh root@<ip> bash deploy-vps.sh
#
# Total time: ~5 min on a fast box.

set -euo pipefail

DEPLOY_USER="deploy"
PROJECT_DIR="/home/${DEPLOY_USER}/openstory"

echo "=== [1/8] System packages ==="
apt-get update -qq
apt-get install -y -qq \
    ca-certificates curl gnupg ufw git \
    build-essential pkg-config libssl-dev \
    tmux htop jq unzip wget tree ripgrep fd-find \
    protobuf-compiler

echo "=== [2/8] Firewall ==="
ufw default deny incoming
ufw default allow outgoing
ufw allow ssh
echo "y" | ufw enable

echo "=== [3/8] Create deploy user ==="
if ! id "${DEPLOY_USER}" &>/dev/null; then
    adduser --disabled-password --gecos "" "${DEPLOY_USER}"
    usermod -aG sudo "${DEPLOY_USER}"
    mkdir -p /home/${DEPLOY_USER}/.ssh
    cp ~/.ssh/authorized_keys /home/${DEPLOY_USER}/.ssh/
    chown -R ${DEPLOY_USER}:${DEPLOY_USER} /home/${DEPLOY_USER}/.ssh
    chmod 700 /home/${DEPLOY_USER}/.ssh
    chmod 600 /home/${DEPLOY_USER}/.ssh/authorized_keys
    echo "${DEPLOY_USER} ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/${DEPLOY_USER}
fi

echo "=== [4/8] Docker ==="
if ! command -v docker &>/dev/null; then
    install -m 0755 -d /etc/apt/keyrings
    curl -fsSL https://download.docker.com/linux/debian/gpg \
        | gpg --dearmor -o /etc/apt/keyrings/docker.gpg
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/docker.gpg] \
        https://download.docker.com/linux/debian $(. /etc/os-release && echo $VERSION_CODENAME) stable" \
        > /etc/apt/sources.list.d/docker.list
    apt-get update -qq
    apt-get install -y -qq docker-ce docker-ce-cli containerd.io docker-compose-plugin
    usermod -aG docker ${DEPLOY_USER}
fi

echo "=== [5/8] Tailscale ==="
if ! command -v tailscale &>/dev/null; then
    curl -fsSL https://tailscale.com/install.sh | sh
fi

echo "=== [6/8] Caddy ==="
if ! command -v caddy &>/dev/null; then
    apt-get install -y -qq debian-keyring debian-archive-keyring apt-transport-https
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' \
        | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
    curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' \
        | tee /etc/apt/sources.list.d/caddy-stable.list
    apt-get update -qq && apt-get install -y -qq caddy
fi

echo "=== [7/8] Dev tools (Rust + Node) for deploy user ==="
# Install Rust for the deploy user
su - ${DEPLOY_USER} -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y'

# Install Node via fnm (fast node manager)
su - ${DEPLOY_USER} -c 'curl -fsSL https://fnm.vercel.app/install | bash'
su - ${DEPLOY_USER} -c 'export PATH="$HOME/.local/share/fnm:$PATH" && eval "$(fnm env)" && fnm install 22 && fnm default 22'

echo "=== [8/8] Finalize ==="
# NOTE: SSH lockdown is NOT done here. Do it manually AFTER you've
# confirmed you can SSH in as the deploy user with key-based auth.
# To lock down later:
#   sed -i 's/^PermitRootLogin.*/PermitRootLogin no/' /etc/ssh/sshd_config
#   sed -i 's/^#PasswordAuthentication.*/PasswordAuthentication no/' /etc/ssh/sshd_config
#   systemctl restart sshd

# Swap (safety net, even with 32GB+)
if [ ! -f /swapfile ]; then
    fallocate -l 4G /swapfile
    chmod 600 /swapfile
    mkswap /swapfile
    swapon /swapfile
    echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

# Project dir
mkdir -p "${PROJECT_DIR}"
chown ${DEPLOY_USER}:${DEPLOY_USER} "${PROJECT_DIR}"

# Caddy log dir
mkdir -p /var/log/caddy
chown caddy:caddy /var/log/caddy

cat << 'BANNER'

=============================================
  VPS ready. Here's the speed run:
=============================================

  1. Tailscale (do this now, as root):

     tailscale up
     # Authorize in browser, then:
     tailscale cert <HOSTNAME>.ts.net

  2. Switch to deploy user and clone:

     su - deploy
     cd ~/openstory
     git clone https://github.com/OpenStoryArc/OpenStory.git .
     git clone https://github.com/OpenStoryArc/OpenClaw.git ~/openclaw

  3. Create .env:

     cat > .env << 'EOF'
     ANTHROPIC_API_KEY=sk-ant-...
     OPEN_STORY_API_TOKEN=CHANGE_ME
     OPEN_STORY_ALLOWED_ORIGINS=https://<HOSTNAME>.ts.net
     TELEGRAM_BOT_TOKEN=<from-botfather>
     TELEGRAM_ALLOWED_USER_ID=<your-id>
     EOF

  4. Build images (parallel — use tmux split):

     docker build -t openclaw:latest ~/openclaw &
     docker build -f Dockerfile.prod -t open-story:prod . &
     wait

  5. Launch:

     docker compose -f docker-compose.prod.yml up -d

  6. Caddy:

     sudo cp Caddyfile /etc/caddy/Caddyfile
     echo 'CADDY_HOSTNAME=<HOSTNAME>.ts.net' | sudo tee /etc/default/caddy
     sudo systemctl restart caddy

  7. Test: message your Telegram bot!

  Dev tools available: rust, node 22, tmux, ripgrep, fd, jq, htop
  Source: ~/.cargo/env && eval "$(fnm env)"

BANNER
