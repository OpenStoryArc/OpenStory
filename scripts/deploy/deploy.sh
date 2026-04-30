#!/usr/bin/env bash
#
# deploy.sh — end-to-end deploy driver for the OpenStory VPS stack.
#
# Orchestrates the deploy of a given git branch to a remote host:
#
#   1. preflight.sh              (read-only, abort on failure)
#   2. backup.sh <tag>           (snapshot volumes)
#   3. git fetch && git checkout (on the VPS)
#   4. set-nats-env.sh           (NATS token + tailscale IP + nats-hub.conf)
#   5. docker build Dockerfile.openclaw -t openclaw-mcp:latest
#   6. docker build Dockerfile.prod     -t open-story:prod
#   7. docker compose up (infra + each agent)
#   8. smoke.sh                  (post-deploy verification)
#
# NOTE: this script does NOT rebuild the upstream openclaw:latest image from
# ~/openclaw. That's a separate concern (upstream moves independently and is
# usually many commits ahead). To refresh upstream openclaw, do it in its
# own change window:
#
#     ssh VPS_HOST 'cd ~/openclaw && git pull && docker build -t openclaw:latest .'
#
# then rerun this script to rebuild openclaw-mcp on top.
#
# Usage:
#   deploy.sh VPS_HOST BRANCH
#
# Example:
#   scripts/deploy/deploy.sh deploy@<vps-host> feat/openclaw-mcp-deploy
#
set -euo pipefail

usage() {
    cat <<'EOF'
deploy.sh — end-to-end deploy driver

USAGE:
    deploy.sh VPS_HOST BRANCH

ARGUMENTS:
    VPS_HOST    SSH target, e.g. deploy@<vps-host>
    BRANCH      git branch to deploy, e.g. feat/openclaw-mcp-deploy or master

ENVIRONMENT:
    SKIP_PREFLIGHT=1     skip preflight.sh (not recommended)
    SKIP_BACKUP=1        skip backup.sh (not recommended)
    SKIP_SMOKE=1         skip smoke.sh
    MIN_SESSIONS=N       passed through to smoke.sh

STEPS:
    1. preflight.sh       read-only health check
    2. backup.sh <tag>    snapshot volumes (tag derived from branch)
    3. git fetch+checkout on the VPS
    4. set-nats-env.sh    provision NATS token + tailscale IP
    5. docker build Dockerfile.openclaw -> openclaw-mcp:latest
    6. docker build Dockerfile.prod     -> open-story:prod
    7. docker compose up -d
    8. smoke.sh           post-deploy verification

    Upstream openclaw:latest is NOT rebuilt here. See comments in source.

EXAMPLES:
    scripts/deploy/deploy.sh deploy@<vps-host> feat/openclaw-mcp-deploy
    SKIP_PREFLIGHT=1 scripts/deploy/deploy.sh deploy@<vps-host> master
EOF
}

if [[ $# -eq 0 ]]; then
    usage
    exit 1
fi

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
esac

if [[ $# -lt 2 ]]; then
    usage
    exit 1
fi

VPS_HOST="$1"
BRANCH="$2"
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

HERE="$(cd "$(dirname "$0")" && pwd)"

# Sanitize branch name for tag usage.
BRANCH_TAG=$(printf '%s' "${BRANCH}" | tr '/' '-' | tr -c 'A-Za-z0-9_.-' '-')

echo "==> deploy: ${VPS_HOST} <- ${BRANCH}"
echo

# ---- 1. preflight ----------------------------------------------------------
if [[ "${SKIP_PREFLIGHT:-0}" != "1" ]]; then
    echo "==> [1/8] preflight"
    "${HERE}/preflight.sh" "${VPS_HOST}"
    echo
else
    echo "==> [1/8] preflight: SKIPPED"
fi

# ---- 2. backup -------------------------------------------------------------
if [[ "${SKIP_BACKUP:-0}" != "1" ]]; then
    echo "==> [2/8] backup"
    "${HERE}/backup.sh" "${VPS_HOST}" "${BRANCH_TAG}"
    echo
else
    echo "==> [2/8] backup: SKIPPED"
fi

# ---- 3. git fetch + checkout on VPS ---------------------------------------
echo "==> [3/8] git fetch && git checkout ${BRANCH}"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<REMOTE
set -euo pipefail
cd "\$HOME/openstory"
if [ -n "\$(git status --porcelain)" ]; then
    echo "deploy: working tree dirty on VPS — aborting" >&2
    git status --short >&2
    exit 1
fi
git fetch origin "${BRANCH}"
git checkout "${BRANCH}"
git reset --hard "origin/${BRANCH}"
git log -1 --format='  head: %h %s'

# sanity: required files for this branch
for f in deploy/nats-hub.conf deploy/nats-leaf.conf Dockerfile.openclaw mcp-server/SKILL.md docker-compose.infra.yml docker-compose.agent.yml; do
    if [ ! -e "\$f" ]; then
        echo "deploy: expected file missing after checkout: \$f" >&2
        exit 1
    fi
done
REMOTE
echo

# ---- 4. NATS env -----------------------------------------------------------
echo "==> [4/8] set-nats-env"
"${HERE}/set-nats-env.sh" "${VPS_HOST}"
echo

# ---- 5 & 6. build images ---------------------------------------------------
# We build openclaw-mcp first because it layers on top of openclaw:latest,
# then open-story:prod. The upstream openclaw:latest is assumed to already
# exist; rebuilding it is a separate concern (see header comment).
echo "==> [5/8] docker build -f Dockerfile.openclaw -t openclaw-mcp:latest"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<'REMOTE'
set -euo pipefail
cd "$HOME/openstory"
if ! docker image inspect openclaw:latest >/dev/null 2>&1; then
    echo "deploy: openclaw:latest base image missing — run:" >&2
    echo "  ssh VPS 'cd ~/openclaw && docker build -t openclaw:latest .'" >&2
    exit 1
fi
docker build -f Dockerfile.openclaw -t openclaw-mcp:latest .
REMOTE
echo

echo "==> [6/8] docker build -f Dockerfile.prod -t open-story:prod"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<'REMOTE'
set -euo pipefail
cd "$HOME/openstory"
docker build -f Dockerfile.prod -t open-story:prod .
REMOTE
echo

# ---- 7. compose up (infra + agents) ----------------------------------------
echo "==> [7/8] docker compose up -d (infra + agents)"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<'REMOTE'
set -euo pipefail
cd "$HOME/openstory"

# Ensure the shared network exists for agent compose files.
docker network create openstory 2>/dev/null || true

# Infra: NATS + Open Story
docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml up -d

# Agents: bring up each agent that has a deploy/*.env file.
for envfile in deploy/*.env; do
    [ -f "$envfile" ] || continue
    name=$(basename "$envfile" .env)
    [ "$name" = "infra" ] && continue
    echo "  agent: ${name}"
    docker compose --project-name "$name" --env-file "$envfile" -f docker-compose.agent.yml up -d
done

sleep 3
echo
echo "--- infra ---"
docker compose --project-name infra -f docker-compose.infra.yml ps
for envfile in deploy/*.env; do
    [ -f "$envfile" ] || continue
    name=$(basename "$envfile" .env)
    [ "$name" = "infra" ] && continue
    echo "--- ${name} ---"
    docker compose --project-name "$name" --env-file "$envfile" -f docker-compose.agent.yml ps
done
REMOTE
echo

# ---- 8. smoke --------------------------------------------------------------
if [[ "${SKIP_SMOKE:-0}" != "1" ]]; then
    echo "==> [8/8] smoke"
    "${HERE}/smoke.sh" "${VPS_HOST}"
    echo
else
    echo "==> [8/8] smoke: SKIPPED"
fi

echo "==> deploy: ok (${BRANCH} live on ${VPS_HOST})"
