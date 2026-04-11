#!/usr/bin/env bash
#
# set-nats-env.sh — provision NATS leaf-node auth on the VPS.
#
# Ensures the VPS has:
#   - NATS_LEAF_TOKEN in ~/openstory/.env (generated once, reused on rerun)
#   - TAILSCALE_IP   in ~/openstory/.env (from `tailscale ip -4`)
#   - deploy/nats-hub.conf with the real token substituted in place of the
#     CHANGE_ME placeholder
#
# Idempotent: if NATS_LEAF_TOKEN already exists in .env, it is reused — we
# never rotate it silently. To rotate, remove the line from .env first.
#
# The token value is never echoed. Only success/failure and presence flags
# are printed.
#
# Usage:
#   set-nats-env.sh VPS_HOST
#
# Example:
#   scripts/deploy/set-nats-env.sh deploy@<vps-host>
#
set -euo pipefail

usage() {
    cat <<'EOF'
set-nats-env.sh — provision NATS leaf-node auth on the VPS

USAGE:
    set-nats-env.sh VPS_HOST

ARGUMENTS:
    VPS_HOST    SSH target, e.g. deploy@<vps-host>

DESCRIPTION:
    On the VPS, ensures the following in ~/openstory/.env:
      NATS_LEAF_TOKEN=<openssl rand -hex 24>   (generated once, reused)
      TAILSCALE_IP=<tailscale ip -4>

    Also substitutes the token into deploy/nats-hub.conf, replacing the
    CHANGE_ME_generate_with_openssl_rand_hex_24 placeholder.

    Idempotent. If NATS_LEAF_TOKEN already exists in .env, it is reused.
    To rotate, remove the line from .env first, then rerun.

    The token value is never printed.

EXAMPLES:
    scripts/deploy/set-nats-env.sh deploy@<vps-host>
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

echo "==> Provisioning NATS env on ${VPS_HOST}"

REMOTE_SCRIPT=$(cat <<'REMOTE'
set -euo pipefail

cd "$HOME/openstory" || { echo "set-nats-env: ~/openstory missing" >&2; exit 1; }

ENV_FILE=".env"
HUB_CONF="deploy/nats-hub.conf"
PLACEHOLDER="CHANGE_ME_generate_with_openssl_rand_hex_24"

touch "${ENV_FILE}"
chmod 600 "${ENV_FILE}"

# --- NATS_LEAF_TOKEN (generate or reuse) -------------------------------
if grep -q '^NATS_LEAF_TOKEN=' "${ENV_FILE}"; then
    echo "  NATS_LEAF_TOKEN: present (reusing)"
    token=$(grep '^NATS_LEAF_TOKEN=' "${ENV_FILE}" | head -1 | cut -d= -f2-)
else
    if ! command -v openssl >/dev/null 2>&1; then
        echo "set-nats-env: openssl not found" >&2
        exit 1
    fi
    token=$(openssl rand -hex 24)
    printf 'NATS_LEAF_TOKEN=%s\n' "${token}" >> "${ENV_FILE}"
    echo "  NATS_LEAF_TOKEN: generated and appended"
fi

if [ -z "${token}" ]; then
    echo "set-nats-env: empty NATS_LEAF_TOKEN after provisioning" >&2
    exit 1
fi

# --- TAILSCALE_IP -------------------------------------------------------
if command -v tailscale >/dev/null 2>&1; then
    ts_ip=$(tailscale ip -4 2>/dev/null | head -1 || true)
else
    ts_ip=""
fi

if [ -n "${ts_ip}" ]; then
    if grep -q '^TAILSCALE_IP=' "${ENV_FILE}"; then
        # Replace existing line (portable: rewrite file)
        tmp=$(mktemp)
        grep -v '^TAILSCALE_IP=' "${ENV_FILE}" > "${tmp}"
        printf 'TAILSCALE_IP=%s\n' "${ts_ip}" >> "${tmp}"
        mv "${tmp}" "${ENV_FILE}"
        chmod 600 "${ENV_FILE}"
        echo "  TAILSCALE_IP   : updated (${ts_ip})"
    else
        printf 'TAILSCALE_IP=%s\n' "${ts_ip}" >> "${ENV_FILE}"
        echo "  TAILSCALE_IP   : appended (${ts_ip})"
    fi
else
    echo "  TAILSCALE_IP   : WARN tailscale not available — leaving existing value (if any)"
fi

chmod 600 "${ENV_FILE}"

# --- substitute token into nats-hub.conf --------------------------------
if [ ! -f "${HUB_CONF}" ]; then
    echo "set-nats-env: ${HUB_CONF} missing" >&2
    exit 1
fi

if grep -q "${PLACEHOLDER}" "${HUB_CONF}"; then
    # Escape any characters dangerous to sed. Tokens from openssl rand -hex
    # are [0-9a-f] so this is belt-and-braces.
    escaped=$(printf '%s' "${token}" | sed -e 's/[\/&]/\\&/g')
    sed -i "s/${PLACEHOLDER}/${escaped}/g" "${HUB_CONF}"
    echo "  ${HUB_CONF}: placeholder replaced"
else
    # Validate: the current file should contain the live token. If it
    # contains neither the placeholder nor the token, warn loudly.
    if grep -qF "${token}" "${HUB_CONF}"; then
        echo "  ${HUB_CONF}: already holds current token (ok)"
    else
        echo "  ${HUB_CONF}: WARN neither placeholder nor current token found — manual review advised"
    fi
fi

echo "set-nats-env: ok"
REMOTE
)

if ! ssh "${SSH_OPTS[@]}" "${VPS_HOST}" "bash -s" <<< "${REMOTE_SCRIPT}"; then
    rc=$?
    echo "set-nats-env: failed (rc=${rc})" >&2
    exit "${rc}"
fi

echo
echo "==> set-nats-env: ok"
