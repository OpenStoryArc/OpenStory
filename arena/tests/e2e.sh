#!/usr/bin/env bash
# Arena end-to-end: register -> launch -> sandbox reachable, wrong user denied.
#
# Auth is join-code REGISTRATION (POST /register with join_code, username,
# password), not a pre-seeded login — see arena/src/routes.rs post_register.
# So this script first makes sure an event with a known join code exists
# (arena/deploy/events/e2e-test.toml, applied via `docker exec arena-cp
# arena up ...` — there is no HTTP upload path for manifests, only the
# control plane's own CLI reading them from inside its container).
#
# HOST RESOLUTION WITHOUT /etc/hosts (no sudo required): the session cookie
# is `Secure` and scoped to `.${ARENA_BASE_DOMAIN}`, so every request has to
# go over real HTTPS to a `*.arena.test`-shaped host — but nothing has to
# actually be in DNS or /etc/hosts for that. curl's `--resolve host:443:IP`
# pins exactly the hostnames this run needs straight to 127.0.0.1 (where
# Caddyfile.dev + `docker compose`'s published 80/443 are listening),
# without touching the resolver at all. Caddy and arena-cp only ever look at
# the TLS SNI / Host header curl sends — which is the real hostname, `curl`
# just skips asking a nameserver where to connect it to — so `@story`/`@user`
# routing (Caddyfile.dev) and authorize_host's Host-suffix parsing
# (arena/src/authz.rs) see exactly what they would with a real /etc/hosts
# entry. `-k` is needed because Caddyfile.dev's `tls internal` cert is
# signed by Caddy's own local CA, which curl doesn't trust by default.
#
# Run against a real deploy host instead of local dev by exporting
# ARENA_BASE_DOMAIN (a real domain, already in DNS) and skipping the
# `docker exec ... arena up` step below (run it yourself against a real
# manifest, or pass EVENT_MANIFEST/EVENT_JOIN_CODE for one already applied).
set -euo pipefail

BASE_DOMAIN=${ARENA_BASE_DOMAIN:-arena.test}
BASE="https://${BASE_DOMAIN}"
ALICE=${ARENA_E2E_USER:-alice-e2e}
MALLORY=${ARENA_E2E_MALLORY:-mallory-e2e}
PASSWORD=${ARENA_E2E_PASSWORD:-pw-123456}

ARENA_CP_CONTAINER=${ARENA_CP_CONTAINER:-arena-cp}
# Path as seen *inside* arena-cp (docker-compose.yml mounts ./events:/events:ro).
EVENT_MANIFEST=${EVENT_MANIFEST:-/events/e2e-test.toml}
JOIN_CODE=${EVENT_JOIN_CODE:-e2e-test-code}

ALICE_HOST="${ALICE}.${BASE_DOMAIN}"
ALICE_STORY_HOST="${ALICE}-story.${BASE_DOMAIN}"
MALLORY_HOST="${MALLORY}.${BASE_DOMAIN}"

RESOLVE=(
  --resolve "${BASE_DOMAIN}:443:127.0.0.1"
  --resolve "${ALICE_HOST}:443:127.0.0.1"
  --resolve "${ALICE_STORY_HOST}:443:127.0.0.1"
  --resolve "${MALLORY_HOST}:443:127.0.0.1"
)

JAR=$(mktemp); JAR2=$(mktemp)
trap 'rm -f "$JAR" "$JAR2"' EXIT

# One-shot request, exact status code required.
#
# NOTE on the `if ! code=$(...)` shape (here and in every helper below): a
# naive `code=$(curl ... || echo "curl-error")` looks like it produces a
# clean fallback, but curl's `-w '%{http_code}'` ALWAYS writes something to
# stdout — "000" on a total connection failure — before exiting non-zero.
# `cmd || echo x` runs `echo x` in addition to (not instead of) whatever
# `cmd` already printed, so that shape actually yields "000curl-error", a
# garbled diagnostic, on exactly the failure case it exists to handle.
# Checking the assignment's own exit status with `if !` and only THEN
# overwriting `code` avoids the concatenation: the "000" curl printed is
# discarded, not appended to. This still matters under `set -e`: a simple
# assignment's exit status is the exit status of its command substitution,
# so an unguarded connection failure would otherwise abort the whole script
# with no context about which check was running.
assert_code() {
  local desc="$1" expected="$2"; shift 2
  local code
  if ! code=$(curl -sk -o /dev/null -w '%{http_code}' "${RESOLVE[@]}" "$@" 2>/dev/null); then
    code="curl-error"
  fi
  if [ "$code" = "$expected" ]; then
    echo "PASS: $desc ($code)"
  else
    echo "FAIL: $desc — expected $expected, got $code" >&2
    exit 1
  fi
}

# Poll instead of a fixed sleep (project convention — see arena/sandbox/smoke.sh
# and justfile). `/launch` returns as soon as the container is *created*;
# ttyd and open-story inside it need a moment to actually start listening.
wait_for_code() {
  local desc="$1" expected="$2" tries="${3:-30}"; shift 3
  local code=""
  for ((i = 0; i < tries; i++)); do
    if ! code=$(curl -sk -o /dev/null -w '%{http_code}' "${RESOLVE[@]}" "$@" 2>/dev/null); then
      code="curl-error"
    fi
    if [ "$code" = "$expected" ]; then
      echo "PASS: $desc ($code, ${i}s)"
      return 0
    fi
    sleep 1
  done
  echo "FAIL: $desc — expected $expected within ${tries}s, last=$code" >&2
  return 1
}

# Like wait_for_code, but also requires the body to match a pattern once the
# expected code shows up — so a 200 from the wrong upstream (a misrouted
# Caddy match, a stale cache, a blank placeholder) can't masquerade as the
# real endpoint actually answering. Body goes to a temp file and is grepped
# there rather than piped through `printf | grep -q`: `grep -q` exits the
# instant it finds a match, which SIGPIPEs the writer on a large body (e.g.
# ttyd's ~700KB page) — and under `set -o pipefail` (this script has it),
# that broken-pipe failure on printf poisons the pipeline's exit status even
# though grep itself matched, turning a real PASS into a false FAIL.
wait_for_code_and_body() {
  local desc="$1" expected="$2" pattern="$3" tries="${4:-30}"; shift 4
  local code="" body_file
  body_file=$(mktemp)
  for ((i = 0; i < tries; i++)); do
    if ! code=$(curl -sk -o /dev/null -w '%{http_code}' "${RESOLVE[@]}" "$@" 2>/dev/null); then
      code="curl-error"
    fi
    if [ "$code" = "$expected" ]; then
      curl -sk -o "$body_file" "${RESOLVE[@]}" "$@" 2>/dev/null || true
      if grep -qiE "$pattern" "$body_file"; then
        echo "PASS: $desc ($code, body matches /${pattern}/i, ${i}s)"
        rm -f "$body_file"
        return 0
      fi
    fi
    sleep 1
  done
  rm -f "$body_file"
  echo "FAIL: $desc — expected $expected + body matching /${pattern}/i within ${tries}s, last code=$code" >&2
  return 1
}

# register-or-login: makes the register step rerun-safe against a stack
# that's still up from a previous invocation. A plain re-POST to /register
# for a username that already exists returns 409 (routes.rs: DbError::
# Duplicate), not 303 — so a bare assert_code would wrongly FAIL on a
# second run even though nothing is actually broken. Chosen over
# unique-per-run usernames (a run-tag suffix) because it needs no extra
# state file or env var to stay valid across runs, and it exercises a real
# path (/login) that a legitimate returning participant would also use —
# arena/events "up" is already documented as idempotent-tolerant the same
# way (README §5 / cmd_up test coverage), so this keeps the whole script's
# idempotence story consistent top to bottom.
register_or_login() {
  local jar="$1" user="$2"
  local code
  if ! code=$(curl -sk -o /dev/null -w '%{http_code}' "${RESOLVE[@]}" \
    -c "$jar" -X POST \
    --data-urlencode "join_code=${JOIN_CODE}" \
    --data-urlencode "username=${user}" \
    --data-urlencode "password=${PASSWORD}" \
    "${BASE}/register" 2>/dev/null); then
    code="curl-error"
  fi
  case "$code" in
    303)
      echo "PASS: register $user -> 303 (303)"
      ;;
    409)
      echo "   ($user already registered from a previous run on this stack — logging in instead)"
      assert_code "login $user -> 303" 303 \
        -c "$jar" -X POST \
        --data-urlencode "username=${user}" \
        --data-urlencode "password=${PASSWORD}" \
        "${BASE}/login"
      ;;
    *)
      echo "FAIL: register $user — expected 303 (or 409-then-login), got $code" >&2
      exit 1
      ;;
  esac
}

echo "== arena e2e =="

echo "-- provisioning event ${EVENT_MANIFEST} (tolerates 'already exists' on a live stack)"
docker exec "$ARENA_CP_CONTAINER" arena up "$EVENT_MANIFEST" \
  || echo "   (arena up returned non-zero — assuming already provisioned; register below will fail loudly if not)"

echo "-- register alice"
register_or_login "$JAR" "$ALICE"

echo "-- launch alice's sandbox"
assert_code "launch alice -> 303" 303 \
  -b "$JAR" -X POST "${BASE}/launch"

echo "-- alice reaches her terminal (ttyd via caddy)"
wait_for_code_and_body "alice terminal 200" 200 'ttyd|terminal' 60 \
  -b "$JAR" "https://${ALICE_HOST}/"

echo "-- alice reaches her -story dashboard (open-story API via caddy)"
wait_for_code_and_body "alice story /api/sessions 200" 200 '"sessions"' 60 \
  -b "$JAR" "https://${ALICE_STORY_HOST}/api/sessions"

echo "-- anonymous is redirected off alice's host"
assert_code "anonymous -> 302" 302 "https://${ALICE_HOST}/"

echo "-- register mallory"
register_or_login "$JAR2" "$MALLORY"

echo "-- mallory is denied alice's sandbox"
assert_code "mallory -> alice's host: 403" 403 \
  -b "$JAR2" "https://${ALICE_HOST}/"

# Not in the brief's reference script, but arena/tests/redteam.sh's
# cross-sandbox probe needs a second REAL sandbox to prove isolation
# against. Without launching one, "another sandbox unreachable" would hold
# trivially because the target doesn't exist yet, not because the network
# seal (per-user internal Docker network, docker_driver.rs) stopped
# anything. Mallory launching her own sandbox is legitimate — she's denied
# *alice's*, never her own.
echo "-- launch mallory's own sandbox (redteam.sh's cross-sandbox target)"
assert_code "launch mallory -> 303" 303 \
  -b "$JAR2" -X POST "${BASE}/launch"
wait_for_code_and_body "mallory terminal 200" 200 'ttyd|terminal' 60 \
  -b "$JAR2" "https://${MALLORY_HOST}/"

echo "E2E PASS"
