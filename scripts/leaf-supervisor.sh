#!/usr/bin/env bash
# Local NATS leaf supervisor — keep the leaf process alive AND keep its
# leaf-node connection to the Hetzner hub healthy.
#
# Why this exists
#   On 2026-05-02 we hit a NATS slow-consumer cascade: when the leaf
#   reconnected to the hub it received a backlog burst, our local persist
#   consumer (per-event Mongo writes) couldn't drain fast enough, the leaf's
#   WriteDeadline expired several times, the hub closed our leaf as "Stale
#   Connection", and the leaf's own auto-reconnect logic gave up. From that
#   point on, no federated events flowed in despite the leaf process
#   appearing healthy. See `docs/research/CONSTELLATION.md` follow-ups.
#
# Death modes this supervisor guards against
#   1. Process crash       → restart nats-server.
#   2. Stale-connection    → process is alive but `/leafz` shows leafs:0
#                            for an extended window; kill and restart.
#
# Idempotent: starts a leaf only if one is not already on :4222.
#
# Usage
#   scripts/leaf-supervisor.sh                  # blocking foreground loop
#   scripts/leaf-supervisor.sh > log 2>&1 &     # background (just up uses this)

set -u

CONF="${LEAF_CONF:-deploy/nats-leaf.conf}"
GRACE_SECS="${LEAF_GRACE_SECS:-60}"   # tolerate transient hub disconnects (~6 polls)
POLL_SECS="${LEAF_POLL_SECS:-10}"
MONITOR_URL="${LEAF_MONITOR_URL:-http://localhost:8222/leafz}"

# Source .env so NATS_LEAF_URL substitutes inside the conf.
if [ -f .env ]; then
  set -a
  # shellcheck disable=SC1091
  source .env
  set +a
fi
: "${NATS_LEAF_URL:?NATS_LEAF_URL missing — see deploy/nats-leaf.conf header}"

leaf_pid() {
  pgrep -f "nats-server -c $CONF" | head -1
}

start_leaf() {
  echo "[leaf-supervisor] starting nats-server -c $CONF"
  nats-server -c "$CONF" > /tmp/nats-leaf.log 2>&1 &
  disown $!
  sleep 3
}

is_connected() {
  local count
  count=$(curl -s --max-time 3 "$MONITOR_URL" 2>/dev/null \
    | python3 -c 'import json,sys; print(len(json.load(sys.stdin).get("leafs",[])))' 2>/dev/null \
    || echo 0)
  [ "${count:-0}" -gt 0 ]
}

# Initial start if no leaf is currently up.
if [ -z "$(leaf_pid)" ]; then
  start_leaf
fi

echo "[leaf-supervisor] watching leaf (poll=${POLL_SECS}s, grace=${GRACE_SECS}s)"
disconnected_for=0
while true; do
  PID=$(leaf_pid)
  if [ -z "$PID" ]; then
    echo "[leaf-supervisor] leaf process gone, restarting"
    start_leaf
    disconnected_for=0
    continue
  fi

  if is_connected; then
    if [ "$disconnected_for" -gt 0 ]; then
      echo "[leaf-supervisor] leaf reconnected to hub after ${disconnected_for}s"
    fi
    disconnected_for=0
  else
    disconnected_for=$((disconnected_for + POLL_SECS))
    if [ "$disconnected_for" -ge "$GRACE_SECS" ]; then
      echo "[leaf-supervisor] leaf disconnected from hub for ${disconnected_for}s — kicking PID $PID"
      kill -9 "$PID" 2>/dev/null || true
      sleep 2
      disconnected_for=0
      # Top of loop will detect missing process and restart it.
    fi
  fi
  sleep "$POLL_SECS"
done
