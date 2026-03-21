#!/bin/bash
# Setup Open Story development environment in WSL Linux.
#
# Usage:
#   # Run inside WSL (Ubuntu 24.04):
#   bash scripts/setup_wsl.sh
#
# What this does:
#   1. Installs build dependencies (C compiler, SSL, protobuf)
#   2. Installs Rust via rustup
#   3. Installs Node.js 22.x
#   4. Clones the repo to ~/open-story (WSL-native filesystem for speed)
#   5. Builds and tests Rust + UI
#   6. Installs Claude Code globally
#
# Prerequisites:
#   - WSL with Ubuntu 24.04 (wsl --install -d Ubuntu-24.04)
#   - ANTHROPIC_API_KEY set in environment (for Claude Code)
#
# IMPORTANT: Do NOT build from /mnt/c/ — the 9P bridge is 5-10x slower.
# Use the WSL-native filesystem (~/open-story) and git push/pull to sync.

set -euo pipefail

REPO_URL="https://github.com/open-story-arc/open-story.git"
REPO_DIR="$HOME/open-story"
DATA_DIR="$HOME/open-story-data"

# --- Colors ---
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

step() { echo -e "${GREEN}==> $1${NC}"; }
warn() { echo -e "${YELLOW}    $1${NC}"; }

# --- 1. System dependencies ---
step "Installing system dependencies..."
sudo apt-get update
sudo apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev protobuf-compiler \
    git curl ca-certificates

# --- 2. Rust ---
if command -v rustc &>/dev/null; then
    warn "Rust already installed: $(rustc --version)"
else
    step "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
fi

# --- 3. Node.js ---
if command -v node &>/dev/null; then
    warn "Node.js already installed: $(node --version)"
else
    step "Installing Node.js 22.x..."
    curl -fsSL https://deb.nodesource.com/setup_22.x | sudo -E bash -
    sudo apt-get install -y nodejs
fi

# --- 4. Clone repo ---
if [ -d "$REPO_DIR" ]; then
    warn "Repo already exists at $REPO_DIR — pulling latest..."
    cd "$REPO_DIR" && git pull
else
    step "Cloning Open Story to $REPO_DIR..."
    git clone "$REPO_URL" "$REPO_DIR"
fi

# --- 5. Build and test ---
step "Building Rust workspace..."
cd "$REPO_DIR/rs" && cargo build

step "Running Rust tests..."
cargo test

step "Installing UI dependencies..."
cd "$REPO_DIR/ui" && npm install

step "Running UI tests..."
npm test -- --run

# --- 6. Claude Code ---
if command -v claude &>/dev/null; then
    warn "Claude Code already installed: $(claude --version 2>/dev/null || echo 'unknown version')"
else
    step "Installing Claude Code..."
    npm install -g @anthropic-ai/claude-code
fi

# --- 7. Configure hooks ---
step "Configuring Claude Code hooks for Open Story..."
mkdir -p "$HOME/.claude"
cat > "$HOME/.claude/settings.json" << 'SETTINGS'
{
  "hooks": {
    "Stop": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }],
    "PostToolUse": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }],
    "SubagentStop": [{ "hooks": [{ "type": "http", "url": "http://localhost:3002/hooks", "timeout": 5 }] }]
  }
}
SETTINGS

# --- 8. Create data directory ---
mkdir -p "$DATA_DIR"

# --- Done ---
step "Setup complete!"
echo ""
echo "  Repo:       $REPO_DIR"
echo "  Data dir:   $DATA_DIR"
echo "  Next steps:"
echo "    1. export ANTHROPIC_API_KEY=\"sk-ant-...\""
echo "    2. cd $REPO_DIR/rs && cargo run -p open-story-cli -- serve --data-dir $DATA_DIR"
echo "    3. In another terminal: claude -p \"Read README.md\" --dangerously-skip-permissions"
echo "    4. curl http://localhost:3002/api/sessions | python3 -m json.tool"
echo ""
