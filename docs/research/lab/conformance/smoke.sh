#!/usr/bin/env bash
#
# smoke.sh — post-deploy smoke test against a real lab URL.
#
# Run this after `tofu apply` + `nixos-anywhere`, against the host's
# public URL, to verify the deployment is healthy. Manual in V0;
# wired into a `lab-deploy.yml` GitHub Actions workflow in V1.
#
# Usage:
#   LAB_URL=https://lab.example.com LAB_TOKEN=... ./conformance/smoke.sh

set -euo pipefail

LAB_URL="${LAB_URL:?LAB_URL is required}"
LAB_TOKEN="${LAB_TOKEN:?LAB_TOKEN is required}"

curl_lab() {
  curl --fail --show-error --silent \
    --max-time 10 \
    -H "Authorization: Bearer $LAB_TOKEN" \
    "$@"
}

echo "[smoke] checking $LAB_URL/api/health"
curl_lab -o /dev/null "$LAB_URL/api/health"

echo "[smoke] checking $LAB_URL/api/sessions"
curl_lab -o /tmp/sessions.json "$LAB_URL/api/sessions"
echo "[smoke]   $(jq 'length' /tmp/sessions.json) sessions"

echo "[smoke] POSTing synthetic hook"
SESSION_ID="smoke-$(date +%s)"
curl_lab \
  -H "Content-Type: application/json" \
  -d "{
    \"hook_event_name\": \"Stop\",
    \"session_id\": \"$SESSION_ID\",
    \"transcript_path\": \"/tmp/$SESSION_ID.jsonl\",
    \"cwd\": \"/tmp/smoke\"
  }" \
  "$LAB_URL/hooks"
echo

echo "[smoke] polling for session $SESSION_ID"
for i in $(seq 1 20); do
  if curl_lab "$LAB_URL/api/sessions" | jq -e ".[] | select(.session_id == \"$SESSION_ID\")" >/dev/null 2>&1; then
    echo "[smoke] session visible after ${i} attempts"
    break
  fi
  sleep 1
  if [[ $i -eq 20 ]]; then
    echo "[smoke] FAIL: session $SESSION_ID not visible after 20s" >&2
    exit 1
  fi
done

echo "[smoke] PASS"
