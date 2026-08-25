#!/usr/bin/env bash
# Smoke-test the arena sandbox image. Requires Docker. Run: just arena-smoke
set -euo pipefail
IMG=${1:-arena-sandbox:dev}
C=arena-smoke-$$
docker run -d --rm --name "$C" \
  -e ANTHROPIC_API_KEY=sk-smoke -e ANTHROPIC_BASE_URL=http://localhost:9 \
  -e ARENA_USERNAME=smoke \
  --tmpfs /tmp "$IMG"
trap 'docker rm -f "$C" >/dev/null 2>&1 || true' EXIT

# Poll instead of a fixed sleep — this project prefers polling over guessing
# a boot-time constant in tests. ~30s ceiling, 1s interval.
wait_for() {
  local desc=$1 url=$2 tries=30
  until docker exec "$C" curl -sf -o /dev/null "$url" 2>/dev/null; do
    tries=$((tries - 1))
    if [ "$tries" -le 0 ]; then
      echo "TIMEOUT waiting for $desc ($url)" >&2
      return 1
    fi
    sleep 1
  done
}

echo "-- ttyd answers"
wait_for "ttyd" http://localhost:7681
echo "-- open-story API answers"
wait_for "open-story API" http://localhost:3002/api/sessions
echo "-- claude present"
docker exec "$C" claude --version
echo "-- runs as non-root"
[ "$(docker exec "$C" id -u)" != "0" ]

echo "-- double-spawn: a second client attach doesn't fork a second claude"
# Approximates a second browser tab connecting to ttyd: ttyd would exec this
# same `tmux new-session -A -s main welcome.sh` command per client. Since the
# session already exists, tmux should just attach — not run welcome.sh again.
docker exec -d "$C" tmux new-session -A -s main /usr/local/bin/welcome.sh
sleep 2
claude_count=$(docker exec "$C" pgrep -c claude)
[ "$claude_count" = "1" ] || {
  echo "expected exactly 1 claude process after a second attach, got $claude_count" >&2
  exit 1
}

echo "SMOKE PASS"
