#!/usr/bin/env bash
#
# smoke.sh — post-deploy verification against the VPS.
#
# Runs a series of read-only checks and prints a pass/fail report. Exits
# non-zero on any check failure so CI / driver scripts can abort.
#
# Checks:
#   1. All four services present and healthy (nats, openclaw, open-story,
#      telegram-bot)
#   2. NATS leaf listener bound
#   3. Open Story /api/sessions returns 200
#   4. Session count >= MIN_SESSIONS (default 10) — guards against data loss
#   5. openclaw-mcp container has /opt/mcp-server/server.py and `uv --version`
#      works inside it
#   6. openclaw.json at runtime contains the mcp.servers.openstory block
#
# Usage:
#   smoke.sh VPS_HOST
#   MIN_SESSIONS=50 smoke.sh deploy@<vps-host>
#
set -euo pipefail

usage() {
    cat <<'EOF'
smoke.sh — post-deploy verification

USAGE:
    smoke.sh VPS_HOST

ARGUMENTS:
    VPS_HOST    SSH target, e.g. deploy@<vps-host>

ENVIRONMENT:
    MIN_SESSIONS   minimum acceptable session count (default 10).
                   Guard against accidental data loss — if the deploy
                   drops us below this, smoke fails.

CHECKS:
    1. compose services present (nats, openclaw, open-story, telegram-bot)
    2. services healthy (where health checks are defined)
    3. NATS leaf listener bound (log scan for "Listening for leafnode")
    4. Open Story /api/sessions returns 200
    5. session count >= MIN_SESSIONS
    6. openclaw-mcp: /opt/mcp-server/server.py exists and uv --version works
    7. openclaw.json contains mcp.servers.openstory block

    Each check is marked [ok] or [FAIL] on its own line. Exits non-zero on
    any failure.

EXAMPLES:
    scripts/deploy/smoke.sh deploy@<vps-host>
    MIN_SESSIONS=50 scripts/deploy/smoke.sh deploy@<vps-host>
EOF
}

if [[ $# -eq 0 ]]; then
    usage
    exit 1
fi

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
esac

VPS_HOST="$1"
MIN_SESSIONS="${MIN_SESSIONS:-10}"
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

echo "==> smoke checks against ${VPS_HOST} (MIN_SESSIONS=${MIN_SESSIONS})"

# Stream a quoted heredoc directly to ssh. Quoted ('REMOTE') means the
# outer shell does NO expansion — every $ is literal on the remote side.
# We pass MIN_SESSIONS via env through ssh's remote environment by
# prepending an assignment to the remote command.
if ! ssh "${SSH_OPTS[@]}" "${VPS_HOST}" "MIN_SESSIONS=${MIN_SESSIONS} bash -s" <<'REMOTE'
set -uo pipefail

MIN_SESSIONS="${MIN_SESSIONS:-10}"
fail=0

pass() { printf '  [ok]   %s\n'   "$1"; }
bad()  { printf '  [FAIL] %s\n'   "$1"; fail=1; }

cd "$HOME/openstory" || { bad "cd ~/openstory"; exit 2; }

# 1. compose services present
svcs=$(docker compose -f docker-compose.prod.yml ps --format '{{.Service}}' 2>/dev/null | sort -u)
for svc in nats openclaw open-story telegram-bot; do
    if echo "${svcs}" | grep -qx "${svc}"; then
        pass "service present: ${svc}"
    else
        bad "service missing: ${svc}"
    fi
done

# 2. health
status_lines=$(docker compose -f docker-compose.prod.yml ps --format '{{.Service}} {{.Status}}' 2>/dev/null || true)
while IFS= read -r line; do
    [ -z "${line}" ] && continue
    svc=$(printf '%s' "${line}" | awk '{print $1}')
    rest=$(printf '%s' "${line}" | cut -d' ' -f2-)
    if   printf '%s' "${rest}" | grep -q 'unhealthy';  then
        bad "status unhealthy: ${svc} (${rest})"
    elif printf '%s' "${rest}" | grep -q 'Restarting'; then
        bad "status restarting: ${svc} (${rest})"
    elif printf '%s' "${rest}" | grep -q 'Exited';     then
        bad "status exited: ${svc} (${rest})"
    elif printf '%s' "${rest}" | grep -Eq 'healthy|Up|running'; then
        pass "status ok: ${svc} (${rest})"
    else
        bad "status unknown: ${svc} (${rest})"
    fi
done <<< "${status_lines}"

# 3. NATS leaf listener
if docker logs openstory-nats-1 2>&1 | grep -q "Listening for leafnode"; then
    pass "nats leaf listener bound"
else
    bad "nats leaf listener not found in logs"
fi

# 4. Open Story API reachable
api_code=$(curl -s -o /tmp/sm_sessions.$$ -w '%{http_code}' http://127.0.0.1:3002/api/sessions 2>/dev/null || echo "000")
if [ "${api_code}" = "200" ]; then
    pass "GET /api/sessions -> 200"
else
    bad "GET /api/sessions -> ${api_code}"
fi

# 5. session count
if [ -s /tmp/sm_sessions.$$ ]; then
    if command -v jq >/dev/null 2>&1; then
        count=$(jq -r '(.sessions // . // []) | length' /tmp/sm_sessions.$$ 2>/dev/null || echo "0")
    else
        count=$(grep -o '"id"' /tmp/sm_sessions.$$ | wc -l | awk '{print $1}')
    fi
    if [ "${count}" -ge "${MIN_SESSIONS}" ] 2>/dev/null; then
        pass "session count = ${count} (>= ${MIN_SESSIONS})"
    else
        bad  "session count = ${count} (< ${MIN_SESSIONS})"
    fi
fi
rm -f /tmp/sm_sessions.$$

# 6. openclaw-mcp container: mcp-server source + uv
if docker exec openstory-openclaw-1 test -f /opt/mcp-server/server.py 2>/dev/null; then
    pass "openclaw-mcp: /opt/mcp-server/server.py present"
else
    bad  "openclaw-mcp: /opt/mcp-server/server.py missing"
fi
if docker exec openstory-openclaw-1 uv --version >/dev/null 2>&1; then
    pass "openclaw-mcp: uv --version"
else
    bad  "openclaw-mcp: uv --version failed"
fi

# 7. openclaw.json contains the openstory MCP block
if docker exec openstory-openclaw-1 grep -q '"openstory"' /home/node/.openclaw/openclaw.json 2>/dev/null; then
    pass "openclaw.json: mcp.servers.openstory block present"
else
    bad  "openclaw.json: missing mcp.servers.openstory block"
fi

if [ "${fail}" -ne 0 ]; then
    echo
    echo "smoke: FAIL"
    exit 2
fi
echo
echo "smoke: ok"
REMOTE
then
    rc=$?
    echo
    echo "smoke: failed (rc=${rc})" >&2
    exit "${rc}"
fi

echo
echo "==> smoke: ok"
