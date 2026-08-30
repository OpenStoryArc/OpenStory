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

echo "-- open-story-mcp binary present (R6.1)"
docker exec "$C" sh -c 'command -v open-story-mcp >/dev/null' || { echo "no mcp binary"; exit 1; }

# Root-cause of the earlier 403 report (see task-4-report.md): the sandbox's
# open-story serve boots with api_token = "" (pass-through auth), so every
# route the MCP reads is on the plain auth_middleware tier and already
# answers 200 from localhost with no credential. Only the admin-role-gated
# PUT/DELETE /api/admin/participants routes 403 (verified below as a control
# — NOT a route the MCP calls), which is why the operator's docker-exec GETs
# never saw it. Assert both: the MCP's actual read routes are 200, and the
# admin-write control is still 403 (regression guard — if this ever flips to
# 200, the seal on policy-write routes silently disappeared).
echo "-- OpenStory read API reachable from inside on localhost, no token needed (R6.1)"
sid_probe='/api/sessions/does-not-exist/records'
for p in "/api/sessions" "/api/search?q=x" "/api/agent/search?q=x" "/api/insights/pulse" "$sid_probe"; do
  code=$(docker exec "$C" sh -c "curl -s -o /dev/null -w '%{http_code}' 'localhost:3002$p'")
  [ "$code" = 200 ] || { echo "MCP read route $p -> $code (expected 200)"; exit 1; }
done
echo "-- control: admin-write route still 403 without an admin credential (no regression on the seal)"
code=$(docker exec "$C" sh -c "curl -s -o /dev/null -w '%{http_code}' -X PUT localhost:3002/api/admin/participants -H 'Content-Type: application/json' -d '{}'")
[ "$code" = 403 ] || { echo "expected /api/admin/participants PUT to still be 403, got $code"; exit 1; }

echo "-- multi-harness watch: pi-mono transcript dir is observed too (R6)"
# Minimal VALID pi-mono line, not a made-up shape: translate_pi.rs's
# is_pi_mono_format() requires type == "session" with a "cwd" field (the
# pi-mono session header), and reader.rs auto-detects transcript format from
# the first line of each watched file. That header alone translates to a
# system.session_start CloudEvent (translate_pi_line), which is enough to
# register a session. A bare `{"type":"session_start"}` (Claude-Code-shaped)
# fails is_pi_mono_format and is silently dropped, so it would not exercise
# this path. The session id open-story assigns is the file stem
# (session_id_from_path), not the JSON payload's "id" field, so the fixture
# file itself is named after the session id we assert on.
docker exec "$C" sh -c 'mkdir -p "$HOME/.pi/agent/sessions/proj" && printf "{\"type\":\"session\",\"id\":\"smoke-pi-entry\",\"cwd\":\"/home/dev/workspace\",\"provider\":\"anthropic\",\"modelId\":\"smoke-model\"}\n" > "$HOME/.pi/agent/sessions/proj/smoke-pi-session.jsonl"'
docker exec "$C" sh -c 'tries=15; until curl -s localhost:3002/api/sessions | jq -e "[.sessions[]? | select(.session_id == \"smoke-pi-session\" and .origin_agent == \"pi-mono\")] | length >= 1" >/dev/null 2>&1; do tries=$((tries-1)); [ "$tries" -le 0 ] && exit 1; sleep 1; done' \
  || { echo "pi harness dir not observed"; exit 1; }

# A second synthetic transcript, this time Claude-Code-shaped, so the MCP
# cross-harness assertion below (R6) has a real claude session to find
# alongside the real pi session seeded above — nothing in this fresh
# container has actually driven the interactive `claude` REPL yet, so
# without this there would be no genuine claude-origin session on record.
# Minimal VALID Claude Code line: reader.rs's format detection falls
# through to ClaudeCode as the default (it isn't Hermes/Grok/Codex/pi-mono
# shaped), and translate.rs's is_known_type() accepts a bare
# `type: "system"` line — this mirrors the `turn_duration` system line
# already used as a minimal fixture in rs/tests/fixtures/synth_hooks.jsonl.
# Session id is the file stem (session_id_from_path's default-format
# fallback), so the fixture file is named after the session id asserted on,
# same convention as the pi-mono fixture above.
echo "-- multi-harness watch: a claude-code transcript is observed too (R6)"
docker exec "$C" sh -c 'mkdir -p "$HOME/.claude/projects/smoke-proj" && printf "{\"uuid\":\"11111111-1111-4111-8111-111111111111\",\"sessionId\":\"smoke-claude-session\",\"timestamp\":\"2026-01-01T00:00:00.000Z\",\"cwd\":\"/home/dev/workspace\",\"type\":\"system\",\"subtype\":\"turn_duration\",\"durationMs\":1}\n" > "$HOME/.claude/projects/smoke-proj/smoke-claude-session.jsonl"'
docker exec "$C" sh -c 'tries=15; until curl -s localhost:3002/api/sessions | jq -e "[.sessions[]? | select(.session_id == \"smoke-claude-session\" and .origin_agent == \"claude-code\")] | length >= 1" >/dev/null 2>&1; do tries=$((tries-1)); [ "$tries" -le 0 ] && exit 1; sleep 1; done' \
  || { echo "claude-code session not observed"; exit 1; }

# --- R6: the openstory MCP is wired into the in-box agent, read-only ---
#
# Launch contract verified against rs/mcp/src/bin/open-story-mcp.rs: the
# binary takes no CLI flags (no --help, no --smoke — there is no arg
# parser at all) and speaks line-delimited JSON-RPC 2.0 over stdio only.
# It reads OPENSTORY_API_URL (default http://localhost:3002, matching
# this sandbox's server) for every query tool via HttpEventStore, and
# connects to OPENSTORY_NATS_URL (default nats://localhost:4222, matching
# this sandbox's --manage-nats instance) at startup — it exits before
# ever reading stdin if that connection fails, so both services need to
# already be up, which "open-story API answers" above already confirmed.
# This is exactly the config skel/mcp.json + welcome.sh's merge hand to
# Claude Code: command "open-story-mcp", env OPENSTORY_API_URL only (NATS
# and API defaults already line up with how this sandbox boots them).

echo "-- mcp registered in ~/.claude.json (R6)"
# NB: exit 0 = found, exit 1 = missing — this is inverted from the task
# brief's Step 5 snippet, which had `?1:0` (exits 1 when PRESENT, the
# opposite of what a shell `||` failure check needs). Verified against a
# real container: the file demonstrably has mcpServers.openstory, and the
# brief's exact expression still made this check fail.
docker exec "$C" sh -c 'node -e "process.exit(JSON.parse(require(\"fs\").readFileSync(process.env.HOME+\"/.claude.json\")).mcpServers?.openstory?0:1)"' \
  || { echo "mcp not wired into claude config"; exit 1; }

echo "-- openstory MCP answers tools/list the way Claude Code will launch it (R6)"
docker exec "$C" sh -c 'printf "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}\n" | OPENSTORY_API_URL=http://localhost:3002 timeout 5 open-story-mcp 2>/dev/null | grep -q list_sessions' \
  || { echo "MCP did not list tools"; exit 1; }

echo "-- openstory MCP list_sessions spans claude-code + pi-mono (R6, cross-harness)"
docker exec "$C" sh -c 'printf "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"list_sessions\",\"arguments\":{}}}\n" \
    | OPENSTORY_API_URL=http://localhost:3002 timeout 5 open-story-mcp 2>/dev/null > /tmp/mcp-list-sessions.out; \
  grep -q smoke-claude-session /tmp/mcp-list-sessions.out && grep -q smoke-pi-session /tmp/mcp-list-sessions.out' \
  || { echo "MCP list_sessions did not span both harnesses"; exit 1; }

echo "SMOKE PASS"
