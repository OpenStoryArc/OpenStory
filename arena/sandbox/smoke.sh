#!/usr/bin/env bash
# Smoke-test the arena sandbox image. Requires Docker. Run: just arena-smoke
set -euo pipefail
IMG=${1:-arena-sandbox:dev}
C=arena-smoke-$$
V=arena-smoke-home-$$
# Mount a named volume at /home/dev, mirroring the DockerDriver's per-user
# $HOME volume in production (docker_driver.rs binds "{volume}:/home/dev").
# A brand-new, never-touched named volume is NOT a faithful stand-in here:
# Docker auto-copies an image directory's content into a volume the first
# time it's mounted, but only when the volume is completely empty. That
# would silently mask a shadow bug (anything baked under /home/dev in the
# image looks present) in a way a real returning user never benefits from —
# their volume already has content (workspace/, data/, .claude/, ...) from a
# prior boot, so Docker does NOT re-copy newer image files into it. Seed the
# volume with a marker file first so it's non-empty before the real
# container starts, forcing the same no-copy path a real relaunch takes.
# welcome.sh's boot-time mkdir (and /etc/tmux.conf living outside /home/dev
# entirely) is what has to make the checks below pass, not image-layer copy.
#
# The seeding container must NOT be $IMG itself: Docker's copy-on-first-use
# is keyed to the volume's own lifecycle (empty at first-ever mount, from
# whichever image mounts it first), so seeding with $IMG would trigger the
# very auto-populate this is trying to rule out, silently re-masking the
# bug. node:22-bookworm-slim is the sandbox's own runtime base layer (always
# local already — the image build pulls it), has nothing baked at /home/dev,
# and so writes the marker without populating anything else. Chown to
# uid/gid 1000 afterward to match a real returning user's volume, which is
# always dev-owned (every welcome.sh mkdir runs as USER dev) — left
# root-owned, the sandbox's dev user couldn't write into it, a permission
# artifact of seeding unrelated to the shadow bug under test.
docker volume create "$V" >/dev/null
docker run --rm -v "$V":/home/dev node:22-bookworm-slim \
  sh -c 'touch /home/dev/.arena-smoke-preexisting-volume && chown -R 1000:1000 /home/dev' >/dev/null
docker run -d --rm --name "$C" \
  -e ANTHROPIC_API_KEY=sk-smoke -e ANTHROPIC_BASE_URL=http://localhost:9 \
  -e ARENA_USERNAME=smoke \
  --tmpfs /tmp -v "$V":/home/dev "$IMG"
trap 'docker rm -f "$C" >/dev/null 2>&1 || true; docker volume rm "$V" >/dev/null 2>&1 || true' EXIT

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

echo "-- CLI tools present (R7)"
for t in jq sqlite3 unzip zip vim less tree; do
  docker exec "$C" sh -c "command -v $t >/dev/null" || { echo "MISSING: $t"; exit 1; }
done
echo "-- exec-capable scratch dir (R3)"
docker exec "$C" sh -c 'd=$HOME/.scratch; mkdir -p "$d"; printf "#!/bin/sh\necho ok\n" > "$d/t.sh"; chmod +x "$d/t.sh"; [ "$("$d/t.sh")" = "ok" ]' || { echo "scratch not exec-capable"; exit 1; }
echo "-- ~/.local/bin on PATH (R4)"
docker exec "$C" bash -lc 'case ":$PATH:" in *:"$HOME/.local/bin":*) : ;; *) exit 1 ;; esac' || { echo ".local/bin not on PATH"; exit 1; }
echo "-- tmux mouse on (R11)"
docker exec "$C" tmux show -g mouse | grep -q 'mouse on' || { echo "tmux mouse not on"; exit 1; }

echo "-- git identity out of the box (R5)"
docker exec "$C" sh -c 'cd "$(mktemp -d)" && git init -q && echo x > f && git add f && git commit -q -m t && git log -1 --format="%an <%ae>"' | grep -q 'smoke' \
  || { echo "git identity not seeded from ARENA_USERNAME"; exit 1; }

echo "-- workspace CLAUDE.md shipped (R8)"
docker exec "$C" sh -c 'grep -q "cannot install" "$HOME/workspace/CLAUDE.md"' || { echo "no CLAUDE.md"; exit 1; }
echo "-- settings.json allowlist present (R8)"
docker exec "$C" sh -c 'grep -q "git status" "$HOME/.claude/settings.json"' || { echo "no settings allowlist"; exit 1; }
echo "-- README points at the real -story URL (R9)"
docker exec "$C" sh -c 'grep -q -- "-story" "$HOME/workspace/README.md" && grep -q "https://" "$HOME/workspace/README.md"' || { echo "README not fixed"; exit 1; }

echo "SMOKE PASS"
