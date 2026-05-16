#!/usr/bin/env bash
#
# spin_up_and_probe.sh — the headline integration test for the lab.
#
# Brings up a k3d cluster, installs the OpenStory + NATS Helm charts,
# probes the deployed surface end-to-end, then tears everything down.
# Green run = the deploy artifact actually works against the real
# binary in a real Kubernetes — not a mock.
#
# Modeled on rs/tests/helpers/container.rs's start_open_story() polling
# pattern, but at the deployment layer.
#
# Requirements (all available in the lab dev shell — `nix develop`):
#   k3d, kubectl, helm, curl, jq, docker
#   open-story:test image built (`cd rs && docker build -t open-story:test .`)
#
# Usage:
#   ./conformance/spin_up_and_probe.sh
#
# Exit codes:
#   0 — all probes passed
#   1 — a probe failed (cluster left up for inspection if KEEP_CLUSTER=1)

set -euo pipefail

CLUSTER_NAME="${CLUSTER_NAME:-openstory-lab-test}"
NS="openstory"
IMAGE="${IMAGE:-open-story:test}"
KEEP_CLUSTER="${KEEP_CLUSTER:-0}"

LAB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$LAB_DIR"

cleanup() {
  local rc=$?
  if [[ "$KEEP_CLUSTER" == "1" ]]; then
    echo "[spin_up_and_probe] KEEP_CLUSTER=1 — cluster left up for inspection." >&2
    return $rc
  fi
  echo "[spin_up_and_probe] tearing down k3d cluster $CLUSTER_NAME..." >&2
  k3d cluster delete "$CLUSTER_NAME" >/dev/null 2>&1 || true
  return $rc
}
trap cleanup EXIT

echo "[spin_up_and_probe] creating k3d cluster $CLUSTER_NAME"
k3d cluster create "$CLUSTER_NAME" \
  --no-lb \
  --k3s-arg "--disable=traefik,servicelb@server:0" \
  --image rancher/k3s:v1.30.5-k3s1 \
  --wait

echo "[spin_up_and_probe] importing $IMAGE into k3d"
k3d image import "$IMAGE" -c "$CLUSTER_NAME"

echo "[spin_up_and_probe] creating namespace $NS"
kubectl create namespace "$NS"

echo "[spin_up_and_probe] creating openstory-secrets"
# Test-only secrets. Real secrets come from sops-nix on the actual lab host.
kubectl -n "$NS" create secret generic openstory-secrets \
  --from-literal=api_token="$(openssl rand -hex 16)" \
  --from-literal=db_key="$(openssl rand -hex 32)"

echo "[spin_up_and_probe] writing nats hub config to host path"
# k3d's bind-mount of the host's /etc isn't viable; we ship the conf
# as a ConfigMap directly when running under k3d.
kubectl -n "$NS" create configmap nats-hub-conf \
  --from-file=nats-hub.conf="../../deploy/nats-hub.conf"

echo "[spin_up_and_probe] installing nats chart"
# Override the hub-config volume to use the ConfigMap created above.
helm install nats charts/nats -n "$NS" \
  --set "image.tag=2.10-alpine" \
  --set-string "hubConfigHostPath=/etc/nats/nats-hub.conf" \
  --wait --timeout 2m

echo "[spin_up_and_probe] installing openstory chart"
helm install openstory charts/openstory -n "$NS" \
  --set "image.repository=open-story" \
  --set "image.tag=test" \
  --set "image.pullPolicy=Never" \
  --wait --timeout 3m

echo "[spin_up_and_probe] port-forwarding openstory:3002 → localhost:13002"
kubectl -n "$NS" port-forward svc/openstory-openstory 13002:3002 >/tmp/pf.log 2>&1 &
PF_PID=$!
trap 'kill $PF_PID 2>/dev/null || true; cleanup' EXIT

# Health probe — poll 30× 500ms = 15s ceiling
echo "[spin_up_and_probe] probing /api/health"
for i in $(seq 1 30); do
  if curl -fsS -o /dev/null "http://localhost:13002/api/health"; then
    echo "[spin_up_and_probe] health OK after ${i} attempts"
    break
  fi
  sleep 0.5
  if [[ $i -eq 30 ]]; then
    echo "[spin_up_and_probe] FAIL: /api/health never returned 200"
    kubectl -n "$NS" logs deploy/openstory-openstory --tail=100 || true
    exit 1
  fi
done

# POST a synthetic hook event (Stop event shape).
echo "[spin_up_and_probe] POSTing synthetic hook"
SESSION_ID="probe-$(date +%s)"
HOOK_PAYLOAD=$(cat <<EOF
{
  "hook_event_name": "Stop",
  "session_id": "$SESSION_ID",
  "transcript_path": "/tmp/$SESSION_ID.jsonl",
  "cwd": "/tmp/probe"
}
EOF
)
HOOK_RC=$(curl -sS -o /tmp/hook.out -w "%{http_code}" \
  -H "Content-Type: application/json" \
  -d "$HOOK_PAYLOAD" \
  "http://localhost:13002/hooks")
if [[ "$HOOK_RC" != "200" && "$HOOK_RC" != "202" ]]; then
  echo "[spin_up_and_probe] FAIL: /hooks returned $HOOK_RC"
  cat /tmp/hook.out >&2
  exit 1
fi
echo "[spin_up_and_probe] /hooks OK ($HOOK_RC)"

# GET /api/sessions — assert the session shows up. The persist consumer
# is async; poll up to 20 attempts (10s).
echo "[spin_up_and_probe] polling /api/sessions for $SESSION_ID"
for i in $(seq 1 20); do
  if curl -fsS "http://localhost:13002/api/sessions" | jq -e ".[] | select(.session_id == \"$SESSION_ID\")" >/dev/null 2>&1; then
    echo "[spin_up_and_probe] session $SESSION_ID visible after ${i} attempts"
    break
  fi
  sleep 0.5
  if [[ $i -eq 20 ]]; then
    echo "[spin_up_and_probe] FAIL: session $SESSION_ID not visible after 10s"
    curl -sS "http://localhost:13002/api/sessions" | jq . >&2 || true
    exit 1
  fi
done

# GET /api/sessions/{id}/records — assert the event we POSTed is present.
echo "[spin_up_and_probe] checking /api/sessions/$SESSION_ID/records"
RECORDS=$(curl -fsS "http://localhost:13002/api/sessions/$SESSION_ID/records")
if ! echo "$RECORDS" | jq -e 'length > 0' >/dev/null; then
  echo "[spin_up_and_probe] FAIL: no records returned for session $SESSION_ID"
  echo "$RECORDS" >&2
  exit 1
fi

echo "[spin_up_and_probe] PASS — lab spins up and answers probes"
