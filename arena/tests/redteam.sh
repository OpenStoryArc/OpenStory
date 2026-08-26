#!/usr/bin/env bash
# Arena standing red-team seal probes.
#
# Run after arena/tests/e2e.sh (or any two `/launch`es) so two real sandbox
# containers exist. Every probe below MUST FAIL (non-zero exit) from
# *inside* the sandbox for the seal to hold — a probe that *succeeds* is a
# breach, not a passing test. See docker_driver.rs's module doc for the
# mechanism: each sandbox gets its own `internal: true` Docker network with
# only the edge proxy and the LiteLLM gateway attached, so cross-sandbox
# reach and raw internet egress fail at the network layer.
#
# gVisor (ARENA_DOCKER_RUNTIME=runsc) narrows the syscall surface further,
# but none of these five probes attempt a syscall-level escape — they're
# all network- or env-level checks — so they hold identically with or
# without gVisor. Locally (no gVisor on most dev machines, incl. this one:
# arm64 macOS/Docker Desktop 28.3.3) this still exercises the seal's actual
# mechanism (the internal networks), just without the extra kernel-isolation
# layer a real deploy host adds on top. Run it against a runsc-backed
# sandbox on a deploy host for the full picture.
set -uo pipefail

SB=${1:-sandbox-alice-e2e}
SB2=${2:-sandbox-mallory-e2e}
fail=0

probe() { # name, shell command that must FAIL (non-zero) for the seal to hold
  if docker exec "$SB" sh -c "$2" >/dev/null 2>&1; then
    echo "BREACH: $1"
    fail=1
  else
    echo "held:   $1"
  fi
}

probe "(a) docker socket reachable" \
  'test -S /var/run/docker.sock'

probe "(b) another sandbox reachable by container DNS" \
  "timeout 3 sh -c 'curl -sf http://${SB2}:7681 || wget -qO- http://${SB2}:7681'"

probe "(c) direct internet egress (api.anthropic.com)" \
  'timeout 5 curl -sf https://api.anthropic.com'

probe "(c2) direct internet egress (arbitrary host)" \
  'timeout 5 curl -sf https://example.com'

# The sandbox only ever gets a LiteLLM *virtual* key (docker_driver.rs sets
# ANTHROPIC_API_KEY to spec.api_key, which is what LiteLlmMinter::mint
# returned — never the real key, which lives only in the litellm
# container's own env). Real Anthropic keys are shaped "sk-ant-...";
# LiteLLM virtual keys are not. grep -v on ANTHROPIC_BASE_URL is
# belt-and-suspenders so the litellm URL's own env-var name can never
# accidentally satisfy the pattern.
probe "(d) real ANTHROPIC key recoverable from env" \
  'env | grep -v ANTHROPIC_BASE_URL | grep -qE "sk-ant-"'

probe "(e) litellm admin (/key/generate) reachable without master key" \
  'curl -sf -X POST http://arena-litellm:4000/key/generate -H "Content-Type: application/json" -d "{}"'

if [ "$fail" -eq 0 ]; then
  echo "SEAL HOLDS"
else
  echo "SEAL BROKEN"
  exit 1
fi
