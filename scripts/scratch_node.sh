#!/usr/bin/env bash
# scratch_node.sh — boot an isolated OpenStory node for tests, never the live one.
#
# The node ops loop (docs/prompts/node-ops-loop.md, REQUIREMENTS L-08) needs a
# running server to exercise health, presence, and the ops hands. The owner's
# live node on :3002 / :4222 is never restarted, so this boots a scratch node:
# its own data dir, its own port, its own managed loopback NATS on a scratch
# port, JSON logs to a file, and a pidfile so --stop tears it down cleanly.
#
#   scripts/scratch_node.sh                       # boot on :3106 / NATS :4322, temp data dir
#   scripts/scratch_node.sh --port 3107 --nats-port 4323 --data-dir /tmp/os-scratch
#   scripts/scratch_node.sh --stop --data-dir /tmp/os-scratch
#   scripts/scratch_node.sh --dry-run             # print what would run
#   scripts/scratch_node.sh --test                # self-tests (no node is booted)
#
# Binary resolution: $OPEN_STORY_BIN, else <repo>/rs/target/release/open-story,
# else $CARGO_TARGET_DIR/release/open-story, else `open-story` on PATH.
# Output on success: one JSON line with port, nats_port, data_dir, pid, log, nats_log.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PORT=3106
NATS_PORT=4322
DATA_DIR=""
BIN="${OPEN_STORY_BIN:-}"
STOP=0
DRY=0
TEST=0
WAIT_SECS=120

usage() { sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --port) PORT="$2"; shift 2 ;;
    --nats-port) NATS_PORT="$2"; shift 2 ;;
    --data-dir) DATA_DIR="$2"; shift 2 ;;
    --bin) BIN="$2"; shift 2 ;;
    --wait) WAIT_SECS="$2"; shift 2 ;;
    --stop) STOP=1; shift ;;
    --dry-run) DRY=1; shift ;;
    --test) TEST=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# ── pure helpers (tested by --test) ──────────────────────────────────────────

# Refuse the live node's ports. G-04: the loop never touches :3002 / :4222.
guard_ports() {
  local port="$1" nats="$2"
  if [ "$port" = "3002" ] || [ "$nats" = "4222" ]; then
    echo "refusing live ports (3002 / 4222); pick scratch ports" >&2
    return 1
  fi
  if [ "$port" = "$nats" ]; then
    echo "port and nats-port must differ" >&2
    return 1
  fi
}

# The scratch node's config: JSON logs, loopback NATS on the scratch port,
# no leaf, no watch dirs (tests seed their own data), metrics on.
render_config() {
  local port="$1" nats="$2" data_dir="${3:-}"
  # Empty watch-dir strings fall back to the real defaults (~/.claude/projects
  # and friends), which would read the owner's transcripts into a scratch
  # store. Point every watcher at an empty folder inside the data dir instead;
  # tests seed their own data (G-05).
  cat <<EOF
host = "127.0.0.1"
port = ${port}
nats_url = "nats://127.0.0.1:${nats}"
nats_leaf_url = ""
publish_sessions = false
watch_dir = "${data_dir}/watch/claude"
watch_dirs = []
claude_watch_dir = "${data_dir}/watch/claude"
codex_watch_dir = "${data_dir}/watch/codex"
grok_watch_dir = "${data_dir}/watch/grok"
watch_backfill_hours = 0
metrics_enabled = true
log_format = "json"
EOF
}

# The serve command line for a data dir.
render_command() {
  local bin="$1" port="$2" data_dir="$3"
  printf '%s serve --manage-nats --port %s --data-dir %s\n' "$bin" "$port" "$data_dir"
}

resolve_bin() {
  if [ -n "$BIN" ]; then echo "$BIN"; return; fi
  for candidate in \
    "$REPO/rs/target/release/open-story" \
    "${CARGO_TARGET_DIR:-/nonexistent}/release/open-story"; do
    if [ -x "$candidate" ]; then echo "$candidate"; return; fi
  done
  command -v open-story 2>/dev/null || { echo "no open-story binary found; pass --bin or set OPEN_STORY_BIN" >&2; return 1; }
}

# ── self-tests ───────────────────────────────────────────────────────────────

self_test() {
  local fails=0
  check() { if eval "$1"; then echo "  ok   $2"; else echo "  FAIL $2"; fails=$((fails+1)); fi; }

  check '! guard_ports 3002 4322 2>/dev/null' "when_port_is_the_live_port_it_refuses"
  check '! guard_ports 3106 4222 2>/dev/null' "when_nats_port_is_the_live_port_it_refuses"
  check '! guard_ports 3106 3106 2>/dev/null' "when_ports_collide_it_refuses"
  check 'guard_ports 3106 4322' "when_ports_are_scratch_it_allows"

  local cfg; cfg="$(render_config 3107 4323 /tmp/d)"
  check '[ "$(printf "%s" "$cfg" | grep -c "^log_format = \"json\"$")" = 1 ]' "when_config_renders_it_sets_json_logs"
  check '[ "$(printf "%s" "$cfg" | grep -c "^nats_url = \"nats://127.0.0.1:4323\"$")" = 1 ]' "when_config_renders_it_points_nats_at_the_scratch_port"
  check '[ "$(printf "%s" "$cfg" | grep -c "^nats_leaf_url = \"\"$")" = 1 ]' "when_config_renders_it_has_no_leaf"
  check '[ "$(printf "%s" "$cfg" | grep -c "^port = 3107$")" = 1 ]' "when_config_renders_it_binds_the_scratch_port"
  check '[ "$(printf "%s" "$cfg" | grep -c "^publish_sessions = false$")" = 1 ]' "when_config_renders_it_never_publishes"
  check '[ "$(printf "%s" "$cfg" | grep -c "^claude_watch_dir = \"/tmp/d/watch/claude\"$")" = 1 ]' "when_config_renders_it_watches_empty_folders_inside_the_data_dir_not_the_real_ones"

  local cmd; cmd="$(render_command /x/open-story 3107 /tmp/d)"
  check '[ "$cmd" = "/x/open-story serve --manage-nats --port 3107 --data-dir /tmp/d" ]' "when_command_renders_it_manages_nats_on_the_scratch_data_dir"

  local out; out="$(BIN=/x/open-story DATA_DIR=/tmp/d PORT=3107 NATS_PORT=4323 DRY=1 dry_run)"
  check '[ "$(printf "%s" "$out" | grep -c "would run: /x/open-story serve --manage-nats --port 3107 --data-dir /tmp/d")" = 1 ]' "when_dry_run_it_prints_the_command_without_booting"
  check '[ "$(printf "%s" "$out" | grep -c "would write: /tmp/d/config.toml")" = 1 ]' "when_dry_run_it_names_the_config_it_would_write"

  if [ "$fails" = 0 ]; then echo "ok: 13 checks"; else echo "$fails check(s) failed" >&2; return 1; fi
}

dry_run() {
  guard_ports "$PORT" "$NATS_PORT"
  echo "would write: ${DATA_DIR}/config.toml"
  render_config "$PORT" "$NATS_PORT" "$DATA_DIR" | sed 's/^/  /'
  echo "would run: $(render_command "$BIN" "$PORT" "$DATA_DIR")"
  echo "would log to: ${DATA_DIR}/logs/server.log and ${DATA_DIR}/nats/nats.log"
}

# ── actions ──────────────────────────────────────────────────────────────────

stop_node() {
  local data_dir="$1"
  local pidfile="${data_dir}/scratch.pid"
  if [ -f "$pidfile" ]; then
    local pid; pid="$(cat "$pidfile")"
    if kill -0 "$pid" 2>/dev/null; then
      kill "$pid" 2>/dev/null || true
      for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.25; done
      kill -9 "$pid" 2>/dev/null || true
    fi
    rm -f "$pidfile"
  fi
  # The managed NATS child is killed by the server's guard on a clean exit;
  # sweep any orphan bound to this data dir's store.
  pkill -f "nats-server .*${data_dir}/nats" 2>/dev/null || true
  echo "{\"stopped\": true, \"data_dir\": \"${data_dir}\"}"
}

start_node() {
  guard_ports "$PORT" "$NATS_PORT"
  BIN="$(resolve_bin)"
  mkdir -p "$DATA_DIR/logs" "$DATA_DIR/nats" "$DATA_DIR/watch/claude" "$DATA_DIR/watch/codex" "$DATA_DIR/watch/grok"
  render_config "$PORT" "$NATS_PORT" "$DATA_DIR" > "$DATA_DIR/config.toml"
  local log="$DATA_DIR/logs/server.log"
  (
    cd "$DATA_DIR"
    OPEN_STORY_LOG_FORMAT=json nohup $(render_command "$BIN" "$PORT" "$DATA_DIR") >> "$log" 2>&1 &
    echo $! > "$DATA_DIR/scratch.pid"
  )
  local pid; pid="$(cat "$DATA_DIR/scratch.pid")"
  for _ in $(seq 1 "$WAIT_SECS"); do
    if curl -sf -o /dev/null "http://127.0.0.1:${PORT}/health"; then
      printf '{"port": %s, "nats_port": %s, "data_dir": "%s", "pid": %s, "log": "%s", "nats_log": "%s"}\n' \
        "$PORT" "$NATS_PORT" "$DATA_DIR" "$pid" "$log" "$DATA_DIR/nats/nats.log"
      return 0
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      echo "scratch node exited before /health answered; last log lines:" >&2
      tail -20 "$log" >&2
      return 1
    fi
    sleep 1
  done
  echo "scratch node did not answer /health within ${WAIT_SECS}s; see $log" >&2
  return 1
}

# ── main ─────────────────────────────────────────────────────────────────────

if [ "$TEST" = 1 ]; then self_test; exit $?; fi
if [ -z "$DATA_DIR" ]; then DATA_DIR="$(mktemp -d "${TMPDIR:-/tmp}/os-scratch.XXXXXX")"; fi
if [ "$STOP" = 1 ]; then stop_node "$DATA_DIR"; exit 0; fi
if [ "$DRY" = 1 ]; then [ -n "$BIN" ] || BIN="$(resolve_bin)"; dry_run; exit 0; fi
start_node
