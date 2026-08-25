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
sleep 8
echo "-- ttyd answers"
docker exec "$C" curl -sf -o /dev/null http://localhost:7681
echo "-- open-story API answers"
docker exec "$C" curl -sf -o /dev/null http://localhost:3002/api/sessions
echo "-- claude present"
docker exec "$C" claude --version
echo "-- runs as non-root"
[ "$(docker exec "$C" id -u)" != "0" ]
echo "SMOKE PASS"
