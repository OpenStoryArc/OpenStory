# Open Story — command runner
# Install: brew install just (mac), cargo install just (any platform)
# Usage: just <recipe>

# ONNX Runtime library name (platform-dependent)
ort_lib := if os() == "windows" { "onnxruntime.dll" } else if os() == "macos" { "libonnxruntime.dylib" } else { "libonnxruntime.so" }

# Default recipe: list available commands
default:
    @just --list

# Start the Rust server
serve *ARGS:
    cargo run --manifest-path rs/cli/Cargo.toml -- serve {{ARGS}}

# Start the Vite dev server for the UI
dev:
    cd ui && npm run dev

# Run all tests (Rust + UI + Clippy — mirrors CI)
test:
    cargo test --manifest-path rs/Cargo.toml --workspace --exclude open-story-cli -- --skip compose --skip container
    cargo clippy --manifest-path rs/Cargo.toml --workspace --exclude open-story-cli -- -D warnings
    cd ui && npm test -- --run

# Run Rust tests only
test-rs:
    cargo test --manifest-path rs/Cargo.toml

# Run UI tests only
test-ui:
    cd ui && npm test -- --run

# Open Vitest UI — interactive test browser for UI specs
test-ui-dev:
    cd ui && npx vitest --ui

# Open Playwright UI mode — visual E2E test runner
e2e-dev:
    cd e2e && npx playwright test --ui

# Run Playwright E2E tests
e2e:
    cd e2e && npx playwright test

# Kill any process on a given port (Windows/Git Bash compatible)
[private]
kill-port port:
    #!/usr/bin/env bash
    if command -v netstat &>/dev/null; then
      pid=$(netstat -ano 2>/dev/null | grep ":{{port}} " | grep LISTENING | awk '{print $5}' | head -1)
      if [ -n "$pid" ] && [ "$pid" != "0" ]; then
        echo "Killing process $pid on port {{port}}"
        taskkill //F //PID "$pid" 2>/dev/null || kill "$pid" 2>/dev/null || true
      fi
    elif command -v lsof &>/dev/null; then
      pid=$(lsof -ti :{{port}} 2>/dev/null | head -1)
      if [ -n "$pid" ]; then
        echo "Killing process $pid on port {{port}}"
        kill "$pid" 2>/dev/null || true
      fi
    fi

# Build and start both server + UI (Ctrl+C to stop)
up:
    #!/usr/bin/env bash
    set -e
    # Kill any lingering open-story processes
    if [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]]; then
      taskkill //F //IM open-story.exe 2>/dev/null || true
    else
      pkill -f 'open-story.*serve' 2>/dev/null || true
    fi
    just kill-port 3002
    just kill-port 5173
    trap 'kill $(jobs -p) 2>/dev/null' EXIT
    cargo build --manifest-path rs/cli/Cargo.toml
    cargo run --manifest-path rs/cli/Cargo.toml -- serve &
    sleep 2
    cd ui && npm run dev &
    wait

# Launch Jupyter notebook for event data exploration
explore:
    cd scripts && uv run --with pandas --with plotly --with jupyter jupyter lab explore.ipynb

# Live event viewer — pretty-print the unified event log (--follow for live stream)
events *ARGS:
    cd scripts && PYTHONIOENCODING=utf-8 uv run python event_viewer.py {{ARGS}}

# Container runtime: prefer podman, fall back to docker
container_cmd := if `which podman 2>/dev/null || true` != "" { "podman" } else { "docker" }

# Start Qdrant vector database for semantic search
qdrant:
    #!/usr/bin/env bash
    cmd={{container_cmd}}
    if $cmd ps --format '{{"{{"}}.Names{{"}}"}}' | grep -q '^qdrant$'; then
      echo "Qdrant already running"
    elif $cmd ps -a --format '{{"{{"}}.Names{{"}}"}}' | grep -q '^qdrant$'; then
      $cmd start qdrant
    else
      $cmd run -d --name qdrant -p 6333:6333 -p 6334:6334 docker.io/qdrant/qdrant:v1.13.2
    fi

# Stop Qdrant
qdrant-stop:
    {{container_cmd}} stop qdrant && {{container_cmd}} rm qdrant

# Download the embedding model (all-MiniLM-L6-v2, ~86MB)
download-model:
    python scripts/download_model.py --output data/models

# Token usage summary (--by-session, --by-day, --days N, --model opus)
token-usage *ARGS:
    PYTHONIOENCODING=utf-8 uv run python scripts/token_usage.py {{ARGS}}

# Backfill semantic embeddings for all existing events
backfill:
    ORT_DYLIB_PATH=data/models/{{ort_lib}} cargo run --manifest-path rs/cli/Cargo.toml -- backfill --data-dir ./data

# Start NATS (JetStream) via Docker for local dev with the bus
nats:
    docker run --rm -p 4222:4222 -p 8222:8222 nats:2-alpine --jetstream

# Start everything with Docker Compose (NATS + server + UI)
compose:
    docker compose up --build

# Build the Docker test image (required before container/compose tests)
docker-build:
    cd rs && docker build -t open-story:test .

# Run container tests (local mode, no NATS)
test-container: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_container

# Run compose tests (full NATS bus path)
test-compose: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_compose

# Run search config tests (server + Qdrant)
test-search: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_config_search -- --ignored --nocapture --test-threads=1

# Run full config tests (server + NATS + Qdrant)
test-full: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_config_full -- --ignored --nocapture --test-threads=1

# Run graceful degradation tests
test-degrade: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_config_degrade -- --ignored --nocapture --test-threads=1

# Run all configuration tests (search + full + degrade)
test-configs: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_config_search --test test_config_full --test test_config_degrade -- --ignored --nocapture --test-threads=1

# Build the OpenClaw test image (required before openclaw integration tests)
docker-build-openclaw:
    cd ~/projects/openclaw && docker build -t openclaw:test .

# Run OpenClaw integration tests (requires API key + both Docker images)
test-openclaw: docker-build docker-build-openclaw
    ANTHROPIC_API_KEY=$$(cat .anthropic_api_key 2>/dev/null || echo $$ANTHROPIC_API_KEY) cargo test --manifest-path rs/Cargo.toml -p open-story --test test_openclaw_integration -- --ignored --nocapture --test-threads=1

# Run all Docker-based tests (container + compose + configs)
test-docker: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_container --test test_compose

# Run performance tests — break the bus at small/medium/large container sizes
test-perf: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_compose_perf -- --ignored --nocapture

# Run a specific perf test (e.g., just test-perf-one perf_backfill_small)
test-perf-one NAME: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_compose_perf {{NAME}} -- --ignored --nocapture

# Run the full NFR report (slowest — runs all tiers)
test-perf-nfr: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_compose_perf perf_nfr_report -- --ignored --nocapture

# Run Claude integration test via Rust test harness (requires ANTHROPIC_API_KEY)
test-claude: docker-build
    docker build -t claude-runner:test -f rs/tests/Dockerfile.claude-runner .
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_claude_integration -- --ignored --nocapture

# Run split deployment tests (publisher + NATS + consumer)
test-split: docker-build
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_split_deployment -- --ignored --nocapture

# Run K8s integration tests (requires Docker on Linux/WSL + K3s image)
test-k8s:
    cargo test --manifest-path rs/Cargo.toml -p open-story --test test_k8s -- --ignored --nocapture

# Run Claude integration test via compose (requires ANTHROPIC_API_KEY)
test-claude-compose: docker-build
    docker build -t claude-runner:test -f rs/tests/Dockerfile.claude-runner .
    docker compose -f rs/tests/docker-compose.integration.yml up --build --abort-on-container-exit

# Deploy Open Story + Claude runner (API key via .anthropic_api_key file)
deploy-claude: docker-build
    docker build -t claude-runner:test -f rs/tests/Dockerfile.claude-runner .
    docker compose -f docker-compose.integration.yml up -d

# Deploy split stack: listener + NATS + consumer (no Claude runner)
deploy-split: docker-build
    docker compose -f docker-compose.split.yml up -d

# Deploy OpenClaw: Claude + listener + NATS + consumer
deploy-openclaw: docker-build
    docker build -t claude-runner:test -f rs/tests/Dockerfile.claude-runner .
    docker compose -f docker-compose.openclaw.yml up -d

# Start full stack + observability (Prometheus, Grafana, NATS exporter)
observe:
    docker compose -f docker-compose.yml -f docker-compose.observe.yml up --build

# Start observability overlay only (assumes main stack is running)
observe-only:
    docker compose -f docker-compose.yml -f docker-compose.observe.yml up --build prometheus grafana nats-exporter

# Stop observability stack
observe-down:
    docker compose -f docker-compose.yml -f docker-compose.observe.yml down

# Generate Rust code coverage report (requires cargo-llvm-cov + llvm-tools-preview)
coverage:
    cd rs && cargo llvm-cov --workspace --html

# Generate UI code coverage report
coverage-ui:
    cd ui && npx vitest --coverage --run

# Build everything
build:
    cargo build --manifest-path rs/cli/Cargo.toml --release
    cd ui && npm run build

# Build the production Docker image (server + UI)
prod-build:
    docker build -f Dockerfile.prod -t open-story:prod .

# Start the production stack (OpenClaw + Open Story + Telegram bot)
prod-up: prod-build
    docker compose -f docker-compose.prod.yml up -d

# Stop the production stack
prod-down:
    docker compose -f docker-compose.prod.yml down
