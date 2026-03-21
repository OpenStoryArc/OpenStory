#!/usr/bin/env bash
# Quick dev reload: rebuild Rust server + start UI dev server.
# Usage: ./dev.sh          — start both
#        ./dev.sh server   — restart server only (UI stays running)
#        ./dev.sh ui       — restart UI only

set -e
ROOT="$(cd "$(dirname "$0")" && pwd)"

# Kill background processes on exit
cleanup() {
  echo ""
  echo "Shutting down..."
  kill $(jobs -p) 2>/dev/null || true
}
trap cleanup EXIT

start_server() {
  echo "Building Rust server..."
  cd "$ROOT/rs"
  cargo build -p open-story-cli
  echo "Starting server on :3002"
  cargo run -p open-story-cli &
}

start_ui() {
  echo "Starting UI dev server on :5173"
  cd "$ROOT/ui"
  npm run dev &
}

case "${1:-all}" in
  server)
    start_server
    wait
    ;;
  ui)
    start_ui
    wait
    ;;
  all|*)
    start_server
    sleep 2  # Let server bind port before UI proxy tries to connect
    start_ui
    wait
    ;;
esac
