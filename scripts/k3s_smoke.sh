#!/usr/bin/env bash
# k3s_smoke.sh — stand one OpenStory node up on a k3s cluster, prove it
# serves, and tear it down (REQUIREMENTS K-01, G-08).
#
# Applies deploy/k8s/overlays/a1 into an `os-loop-` namespace, waits for the
# Deployment to be available, port-forwards the Service, runs
# scripts/node_health_probe.py --json against it, prints the verdict, and
# deletes the namespace. Never the hub, never anyone's live services.
#
#   scripts/k3s_smoke.sh                         # kubectl's current context
#   scripts/k3s_smoke.sh --context a1 --namespace os-loop-smoke-$USER
#   scripts/k3s_smoke.sh --keep                  # leave the namespace up
#   scripts/k3s_smoke.sh --dry-run               # print every command, run none
#   scripts/k3s_smoke.sh --test                  # self-tests of the pure helpers
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OVERLAY="$REPO/deploy/k8s/overlays/a1"
CONTEXT=""
NAMESPACE=""   # default: the overlay's own `namespace:`
TIMEOUT="20m"
LOCAL_PORT=3106
KEEP=0
DRY=0
TEST=0

usage() { sed -n '2,16p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; }

while [ $# -gt 0 ]; do
  case "$1" in
    --overlay) OVERLAY="$2"; shift 2 ;;
    --context) CONTEXT="$2"; shift 2 ;;
    --namespace) NAMESPACE="$2"; shift 2 ;;
    --timeout) TIMEOUT="$2"; shift 2 ;;
    --port) LOCAL_PORT="$2"; shift 2 ;;
    --keep) KEEP=1; shift ;;
    --dry-run) DRY=1; shift ;;
    --test) TEST=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) echo "unknown argument: $1" >&2; usage >&2; exit 2 ;;
  esac
done

# ── pure helpers (tested) ────────────────────────────────────────────────────

# The namespace an overlay pins (`namespace: …` in its kustomization.yaml).
overlay_namespace() {
  sed -n 's/^namespace:[[:space:]]*//p' "$1/kustomization.yaml" 2>/dev/null | head -1
}

# G-08: the loop only ever touches os-loop- namespaces.
guard_namespace() {
  case "$1" in
    os-loop-*) return 0 ;;
    *) echo "refusing namespace '$1': experiments live under os-loop-*" >&2; return 1 ;;
  esac
}

# The kubectl prefix for a context, or none for the current one.
kctl() {
  if [ -n "$CONTEXT" ]; then printf 'kubectl --context %s' "$CONTEXT"; else printf 'kubectl'; fi
}

run() {
  if [ "$DRY" = 1 ]; then echo "would run: $*"; return 0; fi
  echo "+ $*" >&2
  "$@"
}

self_test() {
  local fails=0
  check() { if eval "$1"; then echo "  ok   $2"; else echo "  FAIL $2"; fails=$((fails+1)); fi; }
  check 'guard_namespace os-loop-smoke' "when_namespace_is_os_loop_it_allows"
  check '! guard_namespace default 2>/dev/null' "when_namespace_is_default_it_refuses"
  check '! guard_namespace kube-system 2>/dev/null' "when_namespace_is_kube_system_it_refuses"
  check '[ "$(CONTEXT= kctl)" = kubectl ]' "when_no_context_it_uses_the_current_one"
  check '[ "$(CONTEXT=a1 kctl)" = "kubectl --context a1" ]' "when_a_context_is_given_it_is_passed"
  local d; d="$(mktemp -d)"; printf 'namespace: os-loop-a1\nresources: [x]\n' > "$d/kustomization.yaml"
  check '[ "$(overlay_namespace "$d")" = os-loop-a1 ]' "when_the_overlay_pins_a_namespace_it_is_read"
  check '[ -z "$(overlay_namespace /nonexistent)" ]' "when_there_is_no_overlay_the_namespace_is_empty"
  rm -rf "$d"
  local out; out="$(DRY=1 run kubectl apply -k x)"
  check '[ "$out" = "would run: kubectl apply -k x" ]' "when_dry_run_it_prints_the_command"
  if [ "$fails" = 0 ]; then echo "ok: 8 checks"; else echo "$fails check(s) failed" >&2; return 1; fi
}

# ── the smoke ────────────────────────────────────────────────────────────────

smoke() {
  local pinned; pinned="$(overlay_namespace "$OVERLAY")"
  if [ -z "$NAMESPACE" ]; then NAMESPACE="$pinned"; fi
  if [ -n "$pinned" ] && [ "$pinned" != "$NAMESPACE" ]; then
    echo "the overlay pins namespace '$pinned'; pass --namespace $pinned or none" >&2
    return 2
  fi
  guard_namespace "$NAMESPACE"
  local k; k="$(kctl)"
  # shellcheck disable=SC2086
  if [ "$DRY" = 1 ] || ! $k get namespace "$NAMESPACE" >/dev/null 2>&1; then
    run $k create namespace "$NAMESPACE"
  fi
  # shellcheck disable=SC2086
  run $k apply -k "$OVERLAY"
  # shellcheck disable=SC2086
  run $k rollout status deployment/openstory -n "$NAMESPACE" --timeout="$TIMEOUT"

  local pf_pid=""
  if [ "$DRY" = 0 ]; then
    $k port-forward -n "$NAMESPACE" service/openstory "$LOCAL_PORT:3002" >/dev/null 2>&1 &
    pf_pid=$!
    sleep 2
  else
    echo "would run: $k port-forward -n $NAMESPACE service/openstory $LOCAL_PORT:3002"
  fi
  run python3 "$REPO/scripts/node_health_probe.py" --json --api "http://127.0.0.1:$LOCAL_PORT" || true
  # K-04: one JSON object per log line.
  # shellcheck disable=SC2086
  run $k logs -n "$NAMESPACE" deployment/openstory -c server --tail=3
  [ -n "$pf_pid" ] && kill "$pf_pid" 2>/dev/null || true

  if [ "$KEEP" = 1 ]; then
    echo "kept namespace $NAMESPACE; tear down with: $k delete namespace $NAMESPACE"
  else
    # shellcheck disable=SC2086
    run $k delete namespace "$NAMESPACE" --wait=false
  fi
}

if [ "$TEST" = 1 ]; then self_test; exit $?; fi
smoke
