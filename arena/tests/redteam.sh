#!/usr/bin/env bash
# Arena standing red-team seal probes.
#
# Run after arena/tests/e2e.sh (or any two `/launch`es) so two real sandbox
# containers exist. Every probe below MUST FAIL (non-zero exit) from
# *inside* the sandbox for the seal to hold — a probe that *succeeds* is a
# breach, not a passing test. See docker_driver.rs's module doc for the
# mechanism: each sandbox gets its own `internal: true` Docker network with
# only the edge proxy and the LiteLLM gateway attached, so cross-sandbox
# reach and raw internet egress fail at the network layer.
#
# VACUITY GUARDS — a "held" verdict is worthless if it holds for the wrong
# reason (no HTTP client installed, the target container never existed,
# env introspection itself is broken). Every probe below is preceded by a
# positive control that must independently prove the check mechanism
# works, before its "held"/"BREACH" verdict is trusted. Any positive
# control failing ABORTS the whole run loud and non-zero — it never lets a
# missing tool or a dead target masquerade as SEAL HOLDS.
#
# gVisor (ARENA_DOCKER_RUNTIME=runsc) narrows the syscall surface further,
# but none of these five probes attempt a syscall-level escape — they're
# all network- or env-level checks — so they hold identically with or
# without gVisor. Locally (no gVisor on most dev machines, incl. this one:
# arm64 macOS/Docker Desktop) this still exercises the seal's actual
# mechanism (the internal networks), just without the extra kernel-isolation
# layer a real deploy host adds on top. Run it against a runsc-backed
# sandbox on a deploy host for the full picture.
set -uo pipefail

SB=${1:-sandbox-alice-e2e}
SB2=${2:-sandbox-mallory-e2e}
fail=0

abort() {
  echo "ABORT: $1" >&2
  echo "SEAL PROBES DID NOT RUN — this is not a held seal, it's an unrun test." >&2
  exit 2
}

probe() { # name, shell command that must FAIL (non-zero) for the seal to hold
  if docker exec "$SB" sh -c "$2" >/dev/null 2>&1; then
    echo "BREACH: $1"
    fail=1
  else
    echo "held:   $1"
  fi
}

echo "== redteam preflight =="

# --- Guard: both targets must actually be running, not merely named. ---
# Without this, probe (b)'s "held" could mean "the seal blocked it" or
# could just as easily mean "sandbox-mallory-e2e was never launched" — the
# two are indistinguishable from curl's exit code alone. Fail loud on
# either being absent or stopped, so a "held" downstream is provably about
# isolation, not about a target that was never there.
for c in "$SB" "$SB2"; do
  running=$(docker inspect -f '{{.State.Running}}' "$c" 2>/dev/null) || running="absent"
  if [ "$running" != "true" ]; then
    abort "$c is not a running container (docker inspect State.Running=$running). \
Run arena/tests/e2e.sh first — it launches both sandboxes these probes need."
  fi
done
echo "preflight: $SB and $SB2 are both confirmed running"

# --- Guard: a shell must actually work inside the sandbox. ---
if ! docker exec "$SB" sh -c 'true' >/dev/null 2>&1; then
  abort "no working shell (sh) in $SB — every probe below execs through sh -c and would silently no-op."
fi

# --- Guard: an HTTP client must actually be present. ---
# Probes (b)/(c)/(c2) run curl-or-wget with no fallback beyond that pair.
# If neither binary exists, every one of those probes reports "held" for a
# reason that has nothing to do with the network seal — proving nothing.
if ! docker exec "$SB" sh -c 'command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1'; then
  abort "neither curl nor wget found in $SB — probes (b)/(c)/(c2) would report 'held' vacuously (command-not-found, not network-blocked)."
fi

# Echoes a raw HTTP status code from $1 hitting arena-litellm's
# /key/generate (curl's %{http_code} always reports the real status, even
# on a 4xx/5xx — that's the point: a 401 here proves the request reached
# the server and got a real answer, distinct from "000", which means the
# connection never completed at all). Falls back to a coarser wget-based
# approximation when curl is unavailable but wget is: wget's exit code
# doesn't expose the status line as simply as curl's, so this reports
# "200" for a clean fetch and "401" for a connected-but-rejected response
# (this endpoint always 401s without a master key), reserving "000" for
# text-matched connection failures.
#
# NOTE: curl's -w '%{http_code}' prints "000" to stdout on a total
# connection failure AND still exits non-zero — so a naive
# `curl ... || echo 000` doesn't replace that output, it concatenates onto
# it ("000000"). Using `if code=$(...); then echo "$code"; else echo 000;
# fi` checks the assignment's exit status separately and only echoes our
# own literal "000" on the failure branch, discarding whatever partial
# output curl already produced.
litellm_status_from() {
  docker exec "$1" sh -c '
    if command -v curl >/dev/null 2>&1; then
      if code=$(curl -s -o /dev/null -w "%{http_code}" -X POST \
           http://arena-litellm:4000/key/generate \
           -H "Content-Type: application/json" -d "{}" 2>/dev/null); then
        echo "$code"
      else
        echo 000
      fi
    elif command -v wget >/dev/null 2>&1; then
      if wget -q -O /dev/null --post-data="{}" \
           --header="Content-Type: application/json" \
           http://arena-litellm:4000/key/generate 2>/tmp/redteam-wget-err; then
        echo 200
      elif grep -qiE "refused|unreachable|not known|timed out|no route" /tmp/redteam-wget-err 2>/dev/null; then
        echo 000
      else
        echo 401
      fi
    else
      echo NOCLIENT
    fi
  '
}

# --- Positive control: the HTTP client must reach something it SHOULD
# reach (arena-litellm — the sandbox's one legitimate network peer besides
# the edge). Without this, probes (b)/(c)/(c2) reporting "held" would be
# indistinguishable from "the client can't make outbound requests at all,
# for a reason unrelated to the seal" (a stale DNS cache, an image with a
# broken libcurl, whatever). Any 3-digit HTTP status here — even the 401
# probe (e) expects — proves the client and the sandbox's legitimate
# network path both work; only THEN are the negative probes below
# trustworthy.
control_code=$(litellm_status_from "$SB")
case "$control_code" in
  [1-5][0-9][0-9])
    echo "positive control: HTTP client in $SB reached arena-litellm (status $control_code) — held/BREACH verdicts below are meaningful"
    ;;
  *)
    abort "no working HTTP client in sandbox (or arena-litellm unreachable when it should be reachable) — got '$control_code' instead of a real HTTP status. Cannot run seal probes: a 'held' verdict would be meaningless."
    ;;
esac

echo
echo "== seal probes =="

probe "(a) docker socket reachable" \
  'test -S /var/run/docker.sock'

probe "(b) another sandbox reachable by container DNS" \
  "timeout 3 sh -c 'curl -sf http://${SB2}:7681 || wget -qO- http://${SB2}:7681'"

probe "(c) direct internet egress (api.anthropic.com)" \
  'timeout 5 curl -sf https://api.anthropic.com'

probe "(c2) direct internet egress (arbitrary host)" \
  'timeout 5 curl -sf https://example.com'

# --- Positive control for (d): env must actually be introspectable AND
# the sandbox must actually have a virtual key. If `env` itself is broken,
# or the sandbox has no ANTHROPIC_API_KEY at all (e.g. a driver regression
# stopped injecting it), a clean "no sk-ant-* found" would be trivially,
# vacuously true — the check never looked at anything real.
if ! docker exec "$SB" sh -c 'env | grep -q "^ANTHROPIC_API_KEY="'; then
  abort "ANTHROPIC_API_KEY not found in $SB's env (or env unreadable) — probe (d)'s 'no real key' verdict would be meaningless with nothing to check."
fi
echo "positive control: ANTHROPIC_API_KEY is present and env is readable in $SB — probe (d)'s verdict is meaningful"

# The sandbox only ever gets a LiteLLM *virtual* key (docker_driver.rs sets
# ANTHROPIC_API_KEY to spec.api_key, which is what LiteLlmMinter::mint
# returned — never the real key, which lives only in the litellm
# container's own env). Real Anthropic keys are shaped "sk-ant-...";
# LiteLLM virtual keys are not. grep -v on ANTHROPIC_BASE_URL is
# belt-and-suspenders so the litellm URL's own env-var name can never
# accidentally satisfy the pattern.
probe "(d) real ANTHROPIC key recoverable from env" \
  'env | grep -v ANTHROPIC_BASE_URL | grep -qE "sk-ant-"'

probe "(e) litellm admin (/key/generate) reachable without master key" \
  'curl -sf -X POST http://arena-litellm:4000/key/generate -H "Content-Type: application/json" -d "{}"'

echo
if [ "$fail" -eq 0 ]; then
  echo "SEAL HOLDS"
else
  echo "SEAL BROKEN"
  exit 1
fi
