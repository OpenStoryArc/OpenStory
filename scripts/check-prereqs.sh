#!/usr/bin/env bash
# scripts/check-prereqs.sh — probe (and optionally install) OpenStory boot prereqs
#
# Modes:
#   all (default) — check every boot mode's tools
#   native        — cargo, npm, nats-server, just
#   docker        — docker (installed + daemon running)
#
# Usage:
#   bash scripts/check-prereqs.sh                            # report only
#   bash scripts/check-prereqs.sh --mode native
#   bash scripts/check-prereqs.sh --mode docker
#   bash scripts/check-prereqs.sh --install                  # brew-install missing
#   bash scripts/check-prereqs.sh --mode native --install
#
# Exit codes:
#   0 = all prereqs satisfied
#   1 = something missing (or daemon not running)
#   2 = bad arguments

set -euo pipefail

MODE="all"
INSTALL=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)    MODE="${2:-}"; shift 2 ;;
    --install) INSTALL=1; shift ;;
    -h|--help) sed -n '2,18p' "$0"; exit 0 ;;
    *)         echo "unknown arg: $1" >&2; exit 2 ;;
  esac
done

case "$MODE" in
  all|native|docker) ;;
  *) echo "unknown mode: $MODE (expected: all | native | docker)" >&2; exit 2 ;;
esac

# entries: tool|install_key|description
# install_key: a brew package name, OR "rustup"/"docker-desktop" for manual installs
NATIVE_TOOLS=(
  "cargo|rustup|Rust toolchain"
  "npm|node|Node.js"
  "nats-server|nats-server|NATS JetStream"
  "just|just|command runner"
)
DOCKER_TOOLS=(
  "docker|docker-desktop|Docker daemon"
)

check_native=0
check_docker=0
[[ "$MODE" == "all" || "$MODE" == "native" ]] && check_native=1
[[ "$MODE" == "all" || "$MODE" == "docker"  ]] && check_docker=1

missing_brew=()
missing_manual=()  # entries: tool|hint

probe() {
  local entry="$1"
  local tool="${entry%%|*}"
  local rest="${entry#*|}"
  local key="${rest%%|*}"
  local desc="${rest#*|}"

  if command -v "$tool" >/dev/null 2>&1; then
    if [[ "$tool" == "docker" ]]; then
      if docker info >/dev/null 2>&1; then
        printf "  \xe2\x9c\x93 %s — %s (daemon running)\n" "$tool" "$desc"
      else
        printf "  \xe2\x9c\x97 %s — installed but daemon not running\n" "$tool"
        missing_manual+=("docker|start Docker Desktop")
      fi
    else
      printf "  \xe2\x9c\x93 %s — %s\n" "$tool" "$desc"
    fi
    return
  fi

  printf "  \xe2\x9c\x97 %s — %s (not installed)\n" "$tool" "$desc"
  case "$key" in
    rustup)
      missing_manual+=("cargo|curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh") ;;
    docker-desktop)
      missing_manual+=("docker|install Docker Desktop from https://www.docker.com/products/docker-desktop/") ;;
    *)
      missing_brew+=("$key") ;;
  esac
}

if [[ $check_native -eq 1 ]]; then
  echo "Native mode prereqs:"
  for t in "${NATIVE_TOOLS[@]}"; do probe "$t"; done
fi

if [[ $check_docker -eq 1 ]]; then
  [[ $check_native -eq 1 ]] && echo
  echo "Docker mode prereqs:"
  for t in "${DOCKER_TOOLS[@]}"; do probe "$t"; done
fi

echo

if [[ ${#missing_brew[@]} -eq 0 && ${#missing_manual[@]} -eq 0 ]]; then
  echo "All checked prereqs are installed. ✓"
  exit 0
fi

if [[ $INSTALL -eq 0 ]]; then
  echo "Missing prereqs:"
  if [[ ${#missing_brew[@]} -gt 0 ]]; then
    echo "  Brew-installable: ${missing_brew[*]}"
    echo "    → re-run with --install to install these in one shot"
  fi
  if [[ ${#missing_manual[@]} -gt 0 ]]; then
    echo "  Manual install required:"
    for entry in "${missing_manual[@]}"; do
      tool="${entry%%|*}"
      hint="${entry#*|}"
      echo "    $tool: $hint"
    done
  fi
  exit 1
fi

# --install path
if [[ ${#missing_brew[@]} -gt 0 ]]; then
  if ! command -v brew >/dev/null 2>&1; then
    echo "ERROR: brew not installed. Install from https://brew.sh first." >&2
    exit 1
  fi
  echo "Running: brew install ${missing_brew[*]}"
  brew install "${missing_brew[@]}"
  echo
fi

if [[ ${#missing_manual[@]} -gt 0 ]]; then
  echo "Manual install still required (the script can't auto-run these):"
  for entry in "${missing_manual[@]}"; do
    tool="${entry%%|*}"
    hint="${entry#*|}"
    echo "  $tool: $hint"
  done
  exit 1
fi

echo "Done. ✓"
