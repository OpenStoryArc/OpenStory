#!/usr/bin/env bash
#
# backup.sh — snapshot the OpenStory data volumes on the VPS.
#
# Tars up the three durable named volumes:
#   openstory_openclaw-state      (agent state + device.json)
#   openstory_openclaw-workspace  (IDENTITY.md, memory, skills, repos)
#   openstory_os-data             (SQLite + JSONL + plans)
#
# Writes to ~/backups/pre-<TAG>-<timestamp>.tar.gz on the VPS. Uses sudo for
# the tar (volume paths under /var/lib/docker) then chowns the result back to
# the deploy user for easy scp.
#
# This script is destructive in the sense that it spends disk on the VPS, but
# it never touches the volumes themselves — tar reads, it does not write.
#
# Usage:
#   backup.sh VPS_HOST [TAG]
#
# Example:
#   scripts/deploy/backup.sh deploy@<vps-host> openclaw-mcp
#
set -euo pipefail

usage() {
    cat <<'EOF'
backup.sh — snapshot OpenStory volumes on the VPS

USAGE:
    backup.sh VPS_HOST [TAG]

ARGUMENTS:
    VPS_HOST    SSH target, e.g. deploy@<vps-host>
    TAG         optional label, default "manual"
                output: ~/backups/pre-<TAG>-<YYYYMMDD-HHMM>.tar.gz

DESCRIPTION:
    Uses sudo tar on the VPS to snapshot the three named volumes:
      openstory_openclaw-state
      openstory_openclaw-workspace
      openstory_os-data

    Prints the backup path, byte size, and archive entry count so the caller
    can sanity-check the snapshot is non-trivial.

EXAMPLES:
    scripts/deploy/backup.sh deploy@<vps-host>
    scripts/deploy/backup.sh deploy@<vps-host> openclaw-mcp
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
TAG="${2:-manual}"
SSH_OPTS=(-o BatchMode=yes -o ConnectTimeout=10)

# Sanitize tag — strip anything that isn't [A-Za-z0-9_.-]
SAFE_TAG=$(printf '%s' "${TAG}" | tr -c 'A-Za-z0-9_.-' '-')

echo "==> Creating backup on ${VPS_HOST} (tag=${SAFE_TAG})"

REMOTE_SCRIPT=$(cat <<REMOTE
set -euo pipefail

mkdir -p "\$HOME/backups"
ts=\$(date +%Y%m%d-%H%M)
out="\$HOME/backups/pre-${SAFE_TAG}-\${ts}.tar.gz"

vols=(
  /var/lib/docker/volumes/openstory_openclaw-state
  /var/lib/docker/volumes/openstory_openclaw-workspace
  /var/lib/docker/volumes/openstory_os-data
)

for v in "\${vols[@]}"; do
    if [ ! -d "\$v" ]; then
        echo "backup: missing volume \$v" >&2
        exit 1
    fi
done

echo "  writing \${out}"
sudo tar czf "\${out}" "\${vols[@]}"
sudo chown "\$(id -u):\$(id -g)" "\${out}"

size=\$(du -h "\${out}" | awk '{print \$1}')
count=\$(tar -tzf "\${out}" | wc -l | awk '{print \$1}')
echo "  size     : \${size}"
echo "  entries  : \${count}"
echo "  path     : \${out}"
echo "\${out}"
REMOTE
)

# Run over SSH and echo the last line (the backup path) for callers.
if ! ssh "${SSH_OPTS[@]}" "${VPS_HOST}" "bash -s" <<< "${REMOTE_SCRIPT}"; then
    rc=$?
    echo "backup: failed (rc=${rc})" >&2
    exit "${rc}"
fi

echo
echo "==> backup: ok"
