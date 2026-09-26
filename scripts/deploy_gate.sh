#!/usr/bin/env bash
# deploy_gate.sh — roll a new build only when the running node is well, and
# roll back when the new build is not (D-04).
#
# The gate reads the node's verdict through scripts/node_health_probe.py:
#   1. refuse to roll while the running node's verdict is critical;
#   2. run the roll command;
#   3. wait for the node to serve again (startup window);
#   4. read the verdict again; critical means run the rollback command.
#
#   scripts/deploy_gate.sh --api http://127.0.0.1:3002 \
#       --roll "brew upgrade openstory && brew services restart openstory" \
#       --rollback "brew switch openstory 0.4.0 && brew services restart openstory"
#   scripts/deploy_gate.sh --dry-run ...     # print each step, run nothing
#   scripts/deploy_gate.sh --test            # self-tests with stubbed probe and health
#
# Exit codes: 0 rolled and well; 2 refused (running node critical); 3 rolled
# back (new build critical); 4 the node never served within the window
# (rolled back); 1 usage.
#
# Overrides for tests and unusual hosts: OPEN_STORY_PROBE (the probe
# command; default `python3 <repo>/scripts/node_health_probe.py`) and
# OPEN_STORY_HEALTH_CMD (prints /api/health JSON; default curl).
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
API="http://127.0.0.1:3002"
ROLL=""
ROLLBACK=""
STARTUP_SECS=300
POLL_SECS=5
DRY=0
TEST=0

usage() { sed -n '2,24p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --api) API="$2"; shift 2 ;;
    --roll) ROLL="$2"; shift 2 ;;
    --rollback) ROLLBACK="$2"; shift 2 ;;
    --startup-secs) STARTUP_SECS="$2"; shift 2 ;;
    --poll-secs) POLL_SECS="$2"; shift 2 ;;
    --dry-run) DRY=1; shift ;;
    --test) TEST=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 1 ;;
  esac
done

# ── reads ────────────────────────────────────────────────────────────────────

# The verdict word from the probe: ok, warn, critical, or unknown.
verdict() {
  local probe="${OPEN_STORY_PROBE:-python3 $REPO/scripts/node_health_probe.py}"
  $probe --json --api "$API" 2>/dev/null | python3 -c 'import json,sys
try:
    print(json.load(sys.stdin).get("verdict", "unknown"))
except Exception:
    print("unknown")' || echo unknown
}

# The health body's boot phase and git sha: "<phase> <sha>".
health_phase_sha() {
  local cmd="${OPEN_STORY_HEALTH_CMD:-curl -sf --max-time 3 $API/api/health}"
  $cmd 2>/dev/null | python3 -c 'import json,sys
try:
    b = json.load(sys.stdin)
    print((b.get("boot") or {}).get("phase", "unknown"), b.get("git_sha", "unknown"))
except Exception:
    print("unreachable unknown")' || echo "unreachable unknown"
}

# ── pure decisions (tested) ──────────────────────────────────────────────────

# Given the verdict before the roll: "roll" or "refuse".
decide_before() { [ "$1" = "critical" ] && echo refuse || echo roll; }

# Given the verdict after the startup window: "keep" or "rollback".
decide_after() { case "$1" in critical|unknown) echo rollback ;; *) echo keep ;; esac; }

run_step() {
  local label="$1" cmd="$2"
  if [ "$DRY" = 1 ]; then echo "would run ($label): $cmd"; return 0; fi
  echo "running ($label): $cmd"
  bash -c "$cmd"
}

# ── the gate ─────────────────────────────────────────────────────────────────

gate() {
  [ -n "$ROLL" ] || { echo "--roll is required" >&2; return 1; }
  [ -n "$ROLLBACK" ] || { echo "--rollback is required" >&2; return 1; }

  read -r phase_before sha_before <<<"$(health_phase_sha)"
  local before; before="$(verdict)"
  echo "before: verdict=$before phase=$phase_before sha=$sha_before"
  if [ "$(decide_before "$before")" = refuse ]; then
    echo "refused: the running node is critical; fix it before rolling" >&2
    return 2
  fi

  run_step roll "$ROLL"

  local waited=0 phase sha
  while :; do
    read -r phase sha <<<"$(health_phase_sha)"
    if [ "$phase" = serving ]; then break; fi
    if [ "$waited" -ge "$STARTUP_SECS" ]; then
      echo "the node did not serve within ${STARTUP_SECS}s (last phase: $phase); rolling back" >&2
      run_step rollback "$ROLLBACK"
      return 4
    fi
    [ "$DRY" = 1 ] && { echo "would wait for serving (phase: $phase)"; break; }
    sleep "$POLL_SECS"; waited=$((waited + POLL_SECS))
  done

  local after; after="$(verdict)"
  echo "after: verdict=$after phase=$phase sha=$sha (was $sha_before)"
  if [ "$(decide_after "$after")" = rollback ]; then
    echo "the new build is $after; rolling back" >&2
    run_step rollback "$ROLLBACK"
    return 3
  fi
  echo "rolled: sha $sha_before -> $sha, verdict $after"
  return 0
}

# ── self-tests ───────────────────────────────────────────────────────────────

self_test() {
  local fails=0
  check() { if eval "$1"; then echo "  ok   $2"; else echo "  FAIL $2"; fails=$((fails+1)); fi; }
  check '[ "$(decide_before critical)" = refuse ]' "when_the_running_node_is_critical_it_refuses"
  check '[ "$(decide_before warn)" = roll ]' "when_the_running_node_warns_it_rolls"
  check '[ "$(decide_after critical)" = rollback ]' "when_the_new_build_is_critical_it_rolls_back"
  check '[ "$(decide_after unknown)" = rollback ]' "when_the_new_build_cannot_be_read_it_rolls_back"
  check '[ "$(decide_after ok)" = keep ]' "when_the_new_build_is_ok_it_keeps"

  local tmp; tmp="$(mktemp -d)"
  # A probe stub that answers from a queue of verdicts, one per call.
  cat > "$tmp/probe.sh" <<'STUB'
#!/usr/bin/env bash
q="$PROBE_QUEUE"; v="$(head -1 "$q")"; tail -n +2 "$q" > "$q.tmp"; mv "$q.tmp" "$q"
echo "{\"verdict\": \"${v:-unknown}\"}"
STUB
  chmod +x "$tmp/probe.sh"
  cat > "$tmp/health.sh" <<'STUB'
#!/usr/bin/env bash
echo "{\"boot\": {\"phase\": \"serving\"}, \"git_sha\": \"$HEALTH_SHA\"}"
STUB
  chmod +x "$tmp/health.sh"
  export OPEN_STORY_PROBE="$tmp/probe.sh" OPEN_STORY_HEALTH_CMD="$tmp/health.sh" PROBE_QUEUE="$tmp/q" HEALTH_SHA=abc1234
  ROLL="echo rolled >> $tmp/log"; ROLLBACK="echo rolledback >> $tmp/log"; STARTUP_SECS=0; POLL_SECS=0

  : > "$tmp/log"; printf 'critical\n' > "$tmp/q"
  local rc=0; gate >/dev/null 2>&1 || rc=$?
  check "[ $rc = 2 ] && [ ! -s $tmp/log ]" "when_before_is_critical_it_refuses_and_runs_nothing"

  : > "$tmp/log"; printf 'ok\nok\n' > "$tmp/q"
  rc=0; gate >/dev/null 2>&1 || rc=$?
  check "[ $rc = 0 ] && [ \"\$(cat $tmp/log)\" = rolled ]" "when_before_and_after_are_ok_it_rolls_and_keeps"

  : > "$tmp/log"; printf 'warn\ncritical\n' > "$tmp/q"
  rc=0; gate >/dev/null 2>&1 || rc=$?
  check "[ $rc = 3 ] && [ \"\$(tr '\n' ' ' < $tmp/log)\" = 'rolled rolledback ' ]" "when_after_is_critical_it_rolls_back"

  : > "$tmp/log"; printf 'ok\nok\n' > "$tmp/q"; DRY=1
  local out; out="$(gate 2>&1)"; rc=$?; DRY=0
  check "[ $rc = 0 ] && [ ! -s $tmp/log ] && printf '%s' \"\$out\" | grep -q 'would run (roll)'" "when_dry_run_it_prints_and_runs_nothing"

  rm -rf "$tmp"
  if [ "$fails" = 0 ]; then echo "ok: 9 checks"; else echo "$fails check(s) failed" >&2; return 1; fi
}

if [ "$TEST" = 1 ]; then self_test; exit $?; fi
gate
