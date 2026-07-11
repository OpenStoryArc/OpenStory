#!/usr/bin/env bash
# scripts/validate-projection-cache.sh — validate the bounded read-through
# projection cache against a COPY of real data before it ever touches prod.
#
# What this does:
#   1. Clones the `openstory-os-data` volume into a scratch volume (copy,
#      never the live volume — this script only ever reads from prod data).
#   2. Boots the candidate image against the scratch copy with a small
#      OPEN_STORY_PROJECTION_CACHE_BYTES ceiling, so eviction/rebuild behavior
#      is exercised instead of everything just fitting in RAM.
#   3. Reports container RSS (`docker stats`) and the new cache gauges from
#      `/metrics` (see rs/server/src/metrics.rs::render_cache_metrics), then
#      leaves a manual UI checklist to eyeball.
#
# This is a local/manual harness, not CI — it needs a built image and a
# volume with real session data, neither of which exist in a fresh checkout.
#
# Usage:
#   scripts/validate-projection-cache.sh [--image TAG] [--cache-bytes N] \
#     [--source-volume NAME] [--port PORT]
#
#   --image TAG           candidate image to boot (default: open-story:cache-test)
#   --cache-bytes N       OPEN_STORY_PROJECTION_CACHE_BYTES value (default: 1500000000, ~1.5GB)
#   --source-volume NAME  prod-equivalent volume to clone from (default: openstory-os-data)
#   --port PORT           host port to publish the container's port 3002 on (default: 3999)
#
# The scratch container and volume are left running/present on success — the
# whole point is to hand them to a human for the manual UI checklist below.
# Tear down afterward with:
#   docker rm -f os-validate && docker volume rm <scratch-volume-name-printed-above>
#
# Exit codes:
#   0 = ran to completion (still requires the manual UI checklist below)
#   1 = a precondition failed (missing docker, missing source volume, container never came up)
#   2 = bad arguments

set -euo pipefail

IMAGE="open-story:cache-test"
CACHE_BYTES="1500000000"
SOURCE_VOLUME="openstory-os-data"
PORT="3999"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)          IMAGE="${2:-}"; shift 2 ;;
    --cache-bytes)    CACHE_BYTES="${2:-}"; shift 2 ;;
    --source-volume)  SOURCE_VOLUME="${2:-}"; shift 2 ;;
    --port)           PORT="${2:-}"; shift 2 ;;
    -h|--help)        sed -n '2,30p' "$0"; exit 0 ;;
    *)                echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

CONTAINER_NAME="os-validate"
SCRATCH_VOLUME="os-data-copy-$$"

# On a failed boot (never reaches /health), tear the scratch container/volume
# down so failed runs don't leave stale state behind. On success we leave
# both running deliberately — see the teardown note in the usage header.
fail_cleanup() {
  echo "cleaning up scratch container/volume after failure..."
  docker rm -f "${CONTAINER_NAME}" >/dev/null 2>&1 || true
  docker volume rm "${SCRATCH_VOLUME}" >/dev/null 2>&1 || true
}

if ! command -v docker >/dev/null 2>&1; then
  echo "ERROR: docker not found on PATH." >&2
  exit 1
fi

if ! docker info >/dev/null 2>&1; then
  echo "ERROR: docker daemon not reachable (is Docker running?)." >&2
  exit 1
fi

if ! docker volume inspect "${SOURCE_VOLUME}" >/dev/null 2>&1; then
  echo "ERROR: source volume '${SOURCE_VOLUME}' not found." >&2
  echo "  This script clones REAL data — it needs an existing os-data volume" >&2
  echo "  (e.g. restored from a scripts/deploy/backup.sh tarball) to copy from." >&2
  exit 1
fi

echo "== validate-projection-cache =="
echo "image:          ${IMAGE}"
echo "cache bytes:    ${CACHE_BYTES}"
echo "source volume:  ${SOURCE_VOLUME}"
echo "scratch volume: ${SCRATCH_VOLUME}"
echo "port:           ${PORT}"
echo

# Clean up any stale scratch state from a previous crashed run of the same name.
docker rm -f "${CONTAINER_NAME}" >/dev/null 2>&1 || true
docker volume rm "${SCRATCH_VOLUME}" >/dev/null 2>&1 || true

echo "-- cloning ${SOURCE_VOLUME} -> ${SCRATCH_VOLUME} (copy only, source untouched) --"
docker volume create "${SCRATCH_VOLUME}" >/dev/null
docker run --rm \
  -v "${SOURCE_VOLUME}:/from:ro" \
  -v "${SCRATCH_VOLUME}:/to" \
  alpine sh -c 'cd /from && cp -a . /to/'

echo "-- booting ${IMAGE} against the copy, cache ceiling ${CACHE_BYTES} bytes --"
docker run -d --name "${CONTAINER_NAME}" \
  -v "${SCRATCH_VOLUME}:/data" \
  -p "${PORT}:3002" \
  -e OPEN_STORY_PROJECTION_CACHE_BYTES="${CACHE_BYTES}" \
  "${IMAGE}"

echo "-- waiting for boot (replay_boot_sessions can take a moment on real data) --"
BOOTED=0
for _ in $(seq 1 30); do
  if curl -sf "http://localhost:${PORT}/health" >/dev/null 2>&1; then
    BOOTED=1
    break
  fi
  sleep 1
done

if [[ $BOOTED -ne 1 ]]; then
  echo "ERROR: ${CONTAINER_NAME} never answered /health within 30s. Recent logs:" >&2
  docker logs --tail 100 "${CONTAINER_NAME}" >&2 || true
  fail_cleanup
  exit 1
fi

echo
echo "-- container memory (RSS) --"
docker stats --no-stream --format '{{.Name}}\t{{.MemUsage}}\t{{.MemPerc}}' "${CONTAINER_NAME}"

echo
echo "-- cache gauges (rs/server/src/metrics.rs::render_cache_metrics) --"
curl -s "http://localhost:${PORT}/metrics" | grep -E 'openstory_(projection|payload)_' || \
  echo "  (no openstory_ cache lines found — check --image is the candidate build, not a stale one)"

echo
echo "-- manual UI checklist (open http://localhost:${PORT} and confirm) --"
cat <<'EOF'
  [ ] a recent/live session renders big files, code, and tool output IN FULL inline
      (hot set — should never be truncated or need an expand click)
  [ ] a session older than the working-set window (>30d) opens cold and rebuilds
      to an identical view as before (read-through from SQLite, same content)
  [ ] expanding a large tool_result on a cold session works and is fast
  [ ] re-running `docker stats --no-stream` after browsing several sessions shows
      RSS holding near the ceiling, not climbing unbounded
EOF

echo
echo "Done. Container '${CONTAINER_NAME}' and volume '${SCRATCH_VOLUME}' are left"
echo "running for manual poking. Tear down when finished with:"
echo "  docker rm -f ${CONTAINER_NAME} && docker volume rm ${SCRATCH_VOLUME}"
