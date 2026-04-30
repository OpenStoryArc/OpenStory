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

# Check (and optionally install) boot prerequisites — flags: --mode native|docker|all, --install
check *ARGS:
    bash scripts/check-prereqs.sh {{ARGS}}

# Build and start NATS + Mongo + server (mongo backend) + UI (Ctrl+C to stop)
up:
    #!/usr/bin/env bash
    set -e
    # Kill any lingering processes
    if [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]]; then
      taskkill //F //IM open-story.exe 2>/dev/null || true
    else
      pkill -f 'open-story.*serve' 2>/dev/null || true
    fi
    just kill-port 3002
    just kill-port 5173

    # Start NATS JetStream (hard dependency)
    if ! command -v nats-server &>/dev/null; then
      echo "ERROR: nats-server not found. Install: brew install nats-server"
      exit 1
    fi
    if ! lsof -i :4222 &>/dev/null 2>&1; then
      echo "Starting NATS JetStream (leaf node → Hetzner hub)..."
      if [ -f .env ]; then set -a; source .env; set +a; fi
      : "${NATS_LEAF_URL:?NATS_LEAF_URL missing — see deploy/nats-leaf.conf header}"
      nats-server -c deploy/nats-leaf.conf &disown 2>/dev/null
      sleep 1
    else
      echo "NATS already running on :4222"
    fi

    # Start MongoDB (default backend)
    just mongo

    trap 'kill $(jobs -p) 2>/dev/null' EXIT
    cargo build --manifest-path rs/cli/Cargo.toml --features mongo
    OPEN_STORY_DATA_BACKEND=mongo \
    OPEN_STORY_MONGO_URI=mongodb://localhost:27017 \
    OPEN_STORY_MONGO_DB=openstory \
      cargo run --manifest-path rs/cli/Cargo.toml --features mongo -- serve &
    sleep 2
    cd ui && npm run dev &
    wait

# Same as `just up` but uses SQLite (no Docker / Mongo container required)
up-no-mongo:
    #!/usr/bin/env bash
    set -e
    if [[ "$OSTYPE" == msys* || "$OSTYPE" == cygwin* ]]; then
      taskkill //F //IM open-story.exe 2>/dev/null || true
    else
      pkill -f 'open-story.*serve' 2>/dev/null || true
    fi
    just kill-port 3002
    just kill-port 5173

    if ! command -v nats-server &>/dev/null; then
      echo "ERROR: nats-server not found. Install: brew install nats-server"
      exit 1
    fi
    if ! lsof -i :4222 &>/dev/null 2>&1; then
      echo "Starting NATS JetStream (leaf node → Hetzner hub)..."
      if [ -f .env ]; then set -a; source .env; set +a; fi
      : "${NATS_LEAF_URL:?NATS_LEAF_URL missing — see deploy/nats-leaf.conf header}"
      nats-server -c deploy/nats-leaf.conf &disown 2>/dev/null
      sleep 1
    else
      echo "NATS already running on :4222"
    fi

    trap 'kill $(jobs -p) 2>/dev/null' EXIT
    cargo build --manifest-path rs/cli/Cargo.toml
    cargo run --manifest-path rs/cli/Cargo.toml -- serve &
    sleep 2
    cd ui && npm run dev &
    wait

# Start MongoDB (mongo:7) as a Docker container (idempotent)
mongo:
    #!/usr/bin/env bash
    cmd={{container_cmd}}
    name=openstory-mongo
    if $cmd ps --format '{{"{{"}}.Names{{"}}"}}' | grep -q "^${name}$"; then
      echo "MongoDB already running"
    elif $cmd ps -a --format '{{"{{"}}.Names{{"}}"}}' | grep -q "^${name}$"; then
      echo "Restarting existing MongoDB container..."
      $cmd start ${name} >/dev/null
    else
      echo "Starting MongoDB (mongo:7)..."
      $cmd run -d --name ${name} -p 27017:27017 \
        -v openstory-mongo-data:/data/db \
        docker.io/library/mongo:7 >/dev/null
    fi
    # Wait for the server to accept connections (≤10s)
    for i in $(seq 1 20); do
      if $cmd exec ${name} mongosh --quiet --eval 'db.adminCommand({ping: 1})' >/dev/null 2>&1; then
        echo "MongoDB ready on :27017"
        exit 0
      fi
      sleep 0.5
    done
    echo "WARNING: mongo did not become ready in 10s — check '$cmd logs ${name}'"

# Stop and remove the openstory-mongo container (data volume preserved)
mongo-stop:
    #!/usr/bin/env bash
    cmd={{container_cmd}}
    name=openstory-mongo
    if $cmd ps -a --format '{{"{{"}}.Names{{"}}"}}' | grep -q "^${name}$"; then
      $cmd stop ${name} >/dev/null
      $cmd rm ${name} >/dev/null
      echo "Stopped MongoDB"
    else
      echo "MongoDB not running"
    fi

# Drop the openstory-mongo data volume (DESTRUCTIVE — clean slate for Mongo)
mongo-reset: mongo-stop
    #!/usr/bin/env bash
    cmd={{container_cmd}}
    if $cmd volume ls --format '{{"{{"}}.Name{{"}}"}}' | grep -q '^openstory-mongo-data$'; then
      $cmd volume rm openstory-mongo-data >/dev/null
      echo "Removed openstory-mongo-data volume"
    else
      echo "No openstory-mongo-data volume to remove"
    fi

# Start NATS JetStream standalone (leaf node → Hetzner hub)
nats:
    #!/usr/bin/env bash
    set -e
    if [ -f .env ]; then set -a; source .env; set +a; fi
    : "${NATS_LEAF_URL:?NATS_LEAF_URL missing — see deploy/nats-leaf.conf header}"
    nats-server -c deploy/nats-leaf.conf

# Stop NATS
nats-stop:
    pkill nats-server || true

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

# Start NATS (JetStream) via Docker (alternative to native nats-server)
nats-docker:
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

# Build the openclaw-mcp image (requires openclaw:latest already built)
agent-build:
    docker build -f Dockerfile.openclaw -t openclaw-mcp:latest .

# Start shared infrastructure (NATS + Open Story)
infra-up: prod-build
    docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml up -d

# Stop shared infrastructure
infra-down:
    docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml down

# Start a specific agent (usage: just agent-up bobby)
agent-up name: agent-build
    docker compose --project-name {{name}} --env-file deploy/{{name}}.env -f docker-compose.agent.yml up -d

# Stop a specific agent
agent-down name:
    docker compose --project-name {{name}} --env-file deploy/{{name}}.env -f docker-compose.agent.yml down

# Follow logs for a specific agent
agent-logs name:
    docker compose --project-name {{name}} --env-file deploy/{{name}}.env -f docker-compose.agent.yml logs -f

# Start everything: infra + all agents
prod-up: prod-build agent-build
    docker network create openstory 2>/dev/null || true
    docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml up -d
    @for envfile in deploy/*.env; do \
        [ -f "$$envfile" ] || continue; \
        name=$$(basename "$$envfile" .env); \
        [ "$$name" = "infra" ] && continue; \
        echo "starting agent: $$name"; \
        docker compose --project-name "$$name" --env-file "$$envfile" -f docker-compose.agent.yml up -d; \
    done

# Stop everything: all agents + infra
prod-down:
    @for envfile in deploy/*.env; do \
        [ -f "$$envfile" ] || continue; \
        name=$$(basename "$$envfile" .env); \
        [ "$$name" = "infra" ] && continue; \
        docker compose --project-name "$$name" --env-file "$$envfile" -f docker-compose.agent.yml down 2>/dev/null || true; \
    done
    docker compose --project-name infra --env-file deploy/infra.env -f docker-compose.infra.yml down

# Check status of all services
prod-check:
    docker compose --project-name infra -f docker-compose.infra.yml ps
    @for envfile in deploy/*.env; do \
        [ -f "$$envfile" ] || continue; \
        name=$$(basename "$$envfile" .env); \
        [ "$$name" = "infra" ] && continue; \
        echo "--- $$name ---"; \
        docker compose --project-name "$$name" --env-file "$$envfile" -f docker-compose.agent.yml ps; \
    done

# Run production deployment tests (bot unit tests + bot image build)
prod-test:
    cd telegram-bot && python bot.py --test
    docker build -t telegram-bot:test ./telegram-bot
