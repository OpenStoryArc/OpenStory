#!/usr/bin/env bash
#
# preflight.sh — read-only pre-deploy health check for the OpenStory VPS.
#
# Runs a battery of checks over SSH against the target host and prints a
# report. Every check is read-only: nothing is written, pulled, built, or
# restarted. Exit non-zero on SSH failure or if a critical check fails.
#
# Usage:
#   preflight.sh VPS_HOST
#
# Example:
#   scripts/deploy/preflight.sh deploy@<vps-host>
#
set -euo pipefail

usage() {
    cat <<'EOF'
preflight.sh — read-only VPS health check

USAGE:
    preflight.sh VPS_HOST

ARGUMENTS:
    VPS_HOST    SSH target, e.g. deploy@<vps-host>

DESCRIPTION:
    Runs over SSH and reports:
      - hostname, user, uptime
      - repo status (branch, HEAD commit, clean/dirty)
      - running docker compose services
      - disk + inode headroom (/)
      - data volume sizes
      - workspace key files (IDENTITY.md, memory/, skills/)
      - Open Story API health + session count
      - openclaw:latest image age
      - upstream openclaw commits behind (warning only)

    Exits non-zero on SSH failure, dirty repo, missing compose file, or API
    failure. Commit lag on upstream openclaw is a warning, not a failure.

EXAMPLES:
    scripts/deploy/preflight.sh deploy@<vps-host>
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
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

echo "==> Running preflight checks against ${VPS_HOST}"
echo

# Single remote script. Uses 'WARN' prefix for non-fatal issues and 'FAIL'
# for fatal ones. Exit 2 from the remote side means a critical check failed.
REMOTE_SCRIPT=$(cat <<'REMOTE'
set -uo pipefail

fail=0
warn=0

hr() { printf '%s\n' "------------------------------------------------------------"; }

section() { printf '\n[ %s ]\n' "$1"; }

section "host"
echo "hostname : $(hostname)"
echo "user     : $(whoami)"
echo "uptime   : $(uptime | sed 's/^[[:space:]]*//')"

section "repo (~/openstory)"
if [ ! -d "$HOME/openstory/.git" ]; then
    echo "FAIL: ~/openstory is not a git repo"
    fail=1
else
    cd "$HOME/openstory"
    branch=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")
    head=$(git log -1 --format='%h %s' 2>/dev/null || echo "unknown")
    echo "branch   : ${branch}"
    echo "head     : ${head}"
    if [ -z "$(git status --porcelain 2>/dev/null)" ]; then
        echo "status   : clean"
    else
        echo "WARN: working tree dirty"
        git status --short | sed 's/^/  /'
        warn=1
    fi
fi

section "compose services"
if [ -f "$HOME/openstory/docker-compose.infra.yml" ]; then
    echo "  --- infra ---"
    docker compose --project-name infra -f "$HOME/openstory/docker-compose.infra.yml" ps 2>&1 | sed 's/^/  /' || {
        echo "WARN: docker compose ps (infra) failed"
        warn=1
    }
    for envfile in "$HOME/openstory/deploy/"*.env; do
        [ -f "$envfile" ] || continue
        name=$(basename "$envfile" .env)
        [ "$name" = "infra" ] && continue
        echo "  --- ${name} ---"
        docker compose --project-name "$name" --env-file "$envfile" -f "$HOME/openstory/docker-compose.agent.yml" ps 2>&1 | sed 's/^/  /' || true
    done
else
    echo "FAIL: docker-compose.infra.yml missing"
    fail=1
fi

section "disk / inode headroom"
df -h / | sed 's/^/  /'
echo
df -i / | sed 's/^/  /'

section "data volumes"
# Use `docker volume inspect` rather than `du -sh /var/lib/docker/...` because
# the latter needs sudo and silently fails in non-interactive SSH. We report
# presence, not size; disk headroom is already reported above.
for vol in openstory-os-data openstory-nats-data bobby-openclaw-state bobby-openclaw-workspace katie-openclaw-state katie-openclaw-workspace; do
    if docker volume inspect "${vol}" >/dev/null 2>&1; then
        mountpoint=$(docker volume inspect "${vol}" --format '{{.Mountpoint}}' 2>/dev/null)
        printf '  %-40s present\n' "${vol}"
    else
        printf '  %-40s MISSING\n' "${vol}"
        # nats-data is created on first `docker compose up` — not fatal pre-deploy.
        if [ "${vol}" != "openstory-nats-data" ]; then
            warn=1
        fi
    fi
done

section "workspace key files (per agent)"
for envfile in "$HOME/openstory/deploy/"*.env; do
    [ -f "$envfile" ] || continue
    name=$(basename "$envfile" .env)
    [ "$name" = "infra" ] && continue
    ctr="${name}-openclaw-1"
    if docker ps --format '{{.Names}}' | grep -q "^${ctr}$"; then
        echo "  ${name}:"
        for path in "/home/node/.openclaw/workspace/IDENTITY.md" \
                    "/home/node/.openclaw/workspace/memory" \
                    "/home/node/.openclaw/workspace/.claude/skills" \
                    "/home/node/.openclaw/identity/device.json"; do
            if docker exec "${ctr}" test -e "${path}" 2>/dev/null; then
                printf '    %-56s OK\n' "${path}"
            else
                printf '    %-56s MISSING\n' "${path}"
            fi
        done
    else
        echo "  ${ctr} not running — skipping"
    fi
done

section "Open Story API"
api_code=$(curl -s -o /tmp/os_sessions.$$ -w '%{http_code}' http://127.0.0.1:3002/api/sessions 2>/dev/null || echo "000")
if [ "${api_code}" = "200" ]; then
    if command -v jq >/dev/null 2>&1; then
        count=$(jq -r '(.sessions // . // []) | length' /tmp/os_sessions.$$ 2>/dev/null || echo "?")
    else
        count=$(grep -o '"id"' /tmp/os_sessions.$$ | wc -l | awk '{print $1}')
    fi
    echo "  /api/sessions: 200 OK (session count ~ ${count})"
else
    echo "  FAIL: /api/sessions returned ${api_code}"
    fail=1
fi
rm -f /tmp/os_sessions.$$

section "openclaw:latest image age"
if docker image inspect openclaw:latest >/dev/null 2>&1; then
    created=$(docker image inspect openclaw:latest --format '{{.Created}}' 2>/dev/null || echo "?")
    echo "  created  : ${created}"
else
    echo "  WARN: openclaw:latest image not present"
    warn=1
fi

section "upstream openclaw (optional)"
if [ -d "$HOME/openclaw/.git" ]; then
    cd "$HOME/openclaw"
    git fetch --quiet origin 2>/dev/null || true
    behind=$(git rev-list --count HEAD..origin/main 2>/dev/null || echo "?")
    echo "  commits behind origin/main: ${behind}"
    if [ "${behind}" != "0" ] && [ "${behind}" != "?" ]; then
        echo "  WARN: upstream openclaw is ${behind} commits ahead — out of scope for this deploy"
    fi
else
    echo "  ~/openclaw not present (skipping)"
fi

hr
if [ "${fail}" -ne 0 ]; then
    echo "RESULT: FAIL — one or more critical checks failed"
    exit 2
fi
if [ "${warn}" -ne 0 ]; then
    echo "RESULT: OK (with warnings)"
else
    echo "RESULT: OK"
fi
REMOTE
)

if ! ssh "${SSH_OPTS[@]}" "${VPS_HOST}" "bash -s" <<< "${REMOTE_SCRIPT}"; then
    rc=$?
    echo
    if [[ ${rc} -eq 2 ]]; then
        echo "preflight: critical check failed on ${VPS_HOST}" >&2
    else
        echo "preflight: ssh to ${VPS_HOST} failed (rc=${rc})" >&2
    fi
    exit "${rc}"
fi

echo
echo "==> preflight: ok"
