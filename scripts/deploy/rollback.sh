#!/usr/bin/env bash
#
# rollback.sh — restore volumes from a backup tarball and bring the stack
# back up on a previous branch.
#
# This script is DESTRUCTIVE: it stops the stack, extracts a tarball over
# /var/lib/docker/volumes/openstory_*, and brings the stack back up. It
# prompts for confirmation before extracting.
#
# Usage:
#   rollback.sh VPS_HOST [BACKUP_FILE] [BRANCH]
#
# If BACKUP_FILE is omitted, the most recent tarball in ~/backups/ is used.
# If BRANCH is omitted, "master" is used.
#
# Example:
#   scripts/deploy/rollback.sh deploy@<vps-host>
#   scripts/deploy/rollback.sh deploy@<vps-host> /home/deploy/backups/pre-x.tar.gz master
#
set -euo pipefail

usage() {
    cat <<'EOF'
rollback.sh — restore OpenStory volumes from a backup (DESTRUCTIVE)

USAGE:
    rollback.sh VPS_HOST [BACKUP_FILE] [BRANCH]

ARGUMENTS:
    VPS_HOST       SSH target, e.g. deploy@<vps-host>
    BACKUP_FILE    path to tarball on the VPS
                   (default: most recent ~/backups/pre-*.tar.gz)
    BRANCH         git branch to check out after restore
                   (default: master)

DESCRIPTION:
    Destructive. The script:
      1. Lists and confirms the backup file to use
      2. Prompts for y/N confirmation
      3. docker compose down
      4. sudo tar xzf over /var/lib/docker/volumes/openstory_*
      5. git fetch && git checkout BRANCH
      6. docker compose up -d

    Make sure the backup covers all three volumes (openclaw-state,
    openclaw-workspace, os-data). Anything not in the tar is NOT restored.

EXAMPLES:
    scripts/deploy/rollback.sh deploy@<vps-host>
    scripts/deploy/rollback.sh deploy@<vps-host> /home/deploy/backups/pre-openclaw-mcp-20260410-1200.tar.gz
    scripts/deploy/rollback.sh deploy@<vps-host> "" feat/previous-thing
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
BACKUP_FILE="${2:-}"
BRANCH="${3:-master}"
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

echo "==> rollback on ${VPS_HOST} (branch=${BRANCH})"

# ---- discover backup if not provided ---------------------------------------
if [[ -z "${BACKUP_FILE}" ]]; then
    echo "==> discovering latest backup in ~/backups/"
    BACKUP_FILE=$(ssh "${SSH_OPTS[@]}" "${VPS_HOST}" 'ls -1t $HOME/backups/pre-*.tar.gz 2>/dev/null | head -1' || true)
    if [[ -z "${BACKUP_FILE}" ]]; then
        echo "rollback: no backups found in ~/backups/" >&2
        exit 1
    fi
fi

echo "  backup file: ${BACKUP_FILE}"

# ---- inspect backup --------------------------------------------------------
echo "==> inspecting backup"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<REMOTE
set -euo pipefail
if [ ! -f "${BACKUP_FILE}" ]; then
    echo "rollback: backup file not found on VPS: ${BACKUP_FILE}" >&2
    exit 1
fi
size=\$(du -h "${BACKUP_FILE}" | awk '{print \$1}')
count=\$(tar -tzf "${BACKUP_FILE}" | wc -l | awk '{print \$1}')
echo "  size    : \${size}"
echo "  entries : \${count}"
echo
echo "  top-level dirs in tarball:"
tar -tzf "${BACKUP_FILE}" | awk -F/ '{print \$1"/"\$2"/"\$3"/"\$4"/"\$5}' | sort -u | head -20 | sed 's/^/    /'
REMOTE

# ---- confirm ---------------------------------------------------------------
echo
echo "THIS WILL:"
echo "  - docker compose down on ${VPS_HOST}"
echo "  - extract the tarball over /var/lib/docker/volumes/openstory_*"
echo "  - git checkout ${BRANCH}"
echo "  - docker compose up -d"
echo
printf "Proceed? [y/N] "
read -r ans
case "${ans}" in
    y|Y|yes|YES) ;;
    *) echo "rollback: aborted by user"; exit 0 ;;
esac

# ---- execute rollback ------------------------------------------------------
echo
echo "==> executing rollback"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<REMOTE
set -euo pipefail

cd "\$HOME/openstory"

echo "  docker compose down (agents + infra)"
for envfile in deploy/*.env; do
    [ -f "\$envfile" ] || continue
    name=\$(basename "\$envfile" .env)
    [ "\$name" = "infra" ] && continue
    docker compose --project-name "\$name" --env-file "\$envfile" -f docker-compose.agent.yml down 2>/dev/null || true
done
docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml down 2>/dev/null || true

echo "  extracting ${BACKUP_FILE}"
# The tarballs created by backup.sh use absolute paths starting with
# /var/lib/docker/volumes. Extract from / so the paths match.
sudo tar xzf "${BACKUP_FILE}" -C /

echo "  git fetch && git checkout ${BRANCH}"
git fetch origin "${BRANCH}"
git checkout "${BRANCH}"
git reset --hard "origin/${BRANCH}"
git log -1 --format='  head: %h %s'
REMOTE

# After the checkout, the on-disk nats-hub.conf no longer has the live
# token (git reset blew it away). Re-substitute from .env before bringing
# the stack up — otherwise nats will crash on the placeholder.
echo "==> re-substituting NATS token after rollback"
HERE="$(cd "$(dirname "$0")" && pwd)"
"${HERE}/set-nats-env.sh" "${VPS_HOST}"

echo "==> bringing stack back up"
ssh "${SSH_OPTS[@]}" "${VPS_HOST}" bash -s <<REMOTE
set -euo pipefail
cd "\$HOME/openstory"
docker network create openstory 2>/dev/null || true
docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml up -d
for envfile in deploy/*.env; do
    [ -f "\$envfile" ] || continue
    name=\$(basename "\$envfile" .env)
    [ "\$name" = "infra" ] && continue
    docker compose --project-name "\$name" --env-file "\$envfile" -f docker-compose.agent.yml up -d
done
sleep 3
docker compose --project-name infra -f docker-compose.infra.yml ps
for envfile in deploy/*.env; do
    [ -f "\$envfile" ] || continue
    name=\$(basename "\$envfile" .env)
    [ "\$name" = "infra" ] && continue
    docker compose --project-name "\$name" --env-file "\$envfile" -f docker-compose.agent.yml ps
done
REMOTE

echo
echo "==> rollback: done"
echo "    run scripts/deploy/smoke.sh ${VPS_HOST} to verify"
