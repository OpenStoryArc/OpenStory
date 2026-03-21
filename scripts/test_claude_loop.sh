#!/bin/bash
# End-to-end test: Open Story captures events from a Claude Code headless session.
#
# Usage:
#   # From the repo root (inside WSL):
#   bash scripts/test_claude_loop.sh
#
#   # With custom ports/paths:
#   OS_PORT=3099 bash scripts/test_claude_loop.sh
#
# Prerequisites:
#   - ANTHROPIC_API_KEY set in environment
#   - Open Story built (cargo build -p open-story-cli)
#   - Claude Code installed (npm install -g @anthropic-ai/claude-code)
#   - Claude Code hooks configured (~/.claude/settings.json)
#
# What this does:
#   1. Starts Open Story server in background
#   2. Runs Claude Code headless with a simple task
#   3. Verifies events were captured via /api/sessions
#   4. Reports pass/fail

set -euo pipefail

# --- Configuration ---
OS_PORT="${OS_PORT:-3002}"
DATA_DIR="${DATA_DIR:-/tmp/os-loop-test-$$}"
WORK_DIR="${WORK_DIR:-/tmp/os-loop-work-$$}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SERVER_PID=""

# --- Colors ---
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}PASS: $1${NC}"; }
fail() { echo -e "${RED}FAIL: $1${NC}"; }
step() { echo -e "${YELLOW}==> $1${NC}"; }

# --- Cleanup ---
cleanup() {
    if [ -n "$SERVER_PID" ] && kill -0 "$SERVER_PID" 2>/dev/null; then
        step "Stopping Open Story server (PID $SERVER_PID)..."
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
    fi
    rm -rf "$DATA_DIR" "$WORK_DIR"
}
trap cleanup EXIT

# --- Preconditions ---
if [ -z "${ANTHROPIC_API_KEY:-}" ]; then
    fail "ANTHROPIC_API_KEY not set"
    exit 1
fi

if ! command -v claude &>/dev/null; then
    fail "Claude Code not found. Install with: npm install -g @anthropic-ai/claude-code"
    exit 1
fi

# --- Setup ---
mkdir -p "$DATA_DIR" "$WORK_DIR"

# Create a small workspace for Claude to operate in
cat > "$WORK_DIR/README.md" << 'EOF'
# Test Project
A tiny project for integration testing.
EOF

cat > "$WORK_DIR/hello.py" << 'EOF'
def greet(name: str) -> str:
    return f"Hello, {name}!"

if __name__ == "__main__":
    print(greet("world"))
EOF

# --- 1. Start Open Story ---
step "Starting Open Story server on port $OS_PORT..."
cargo run --manifest-path "$REPO_ROOT/rs/cli/Cargo.toml" -- serve \
    --host 127.0.0.1 \
    --port "$OS_PORT" \
    --data-dir "$DATA_DIR" \
    --watch-dir "$WORK_DIR" &
SERVER_PID=$!

# Wait for server readiness
for i in $(seq 1 30); do
    if curl -sf "http://localhost:$OS_PORT/api/sessions" >/dev/null 2>&1; then
        break
    fi
    if ! kill -0 "$SERVER_PID" 2>/dev/null; then
        fail "Server exited unexpectedly"
        exit 1
    fi
    sleep 1
done

if ! curl -sf "http://localhost:$OS_PORT/api/sessions" >/dev/null 2>&1; then
    fail "Server did not become ready within 30s"
    exit 1
fi
step "Server is ready."

# --- 2. Run Claude Code headless ---
step "Running Claude Code headless task..."
CLAUDE_OUTPUT=$(claude -p "Read README.md and hello.py. Then create a file called test_output.txt containing the text 'integration test passed'." \
    --dangerously-skip-permissions \
    --output-format json \
    --cwd "$WORK_DIR" \
    2>&1) || true

echo "Claude output (first 500 chars): ${CLAUDE_OUTPUT:0:500}"

# Give hooks a moment to deliver
sleep 3

# --- 3. Verify events captured ---
step "Checking for captured sessions..."
SESSIONS=$(curl -sf "http://localhost:$OS_PORT/api/sessions" || echo "[]")
SESSION_COUNT=$(echo "$SESSIONS" | python3 -c "import sys,json; data=json.load(sys.stdin); print(len(data))" 2>/dev/null || echo "0")

echo "Sessions found: $SESSION_COUNT"

RESULTS=0
FAILURES=0

# Test 1: At least one session captured
if [ "$SESSION_COUNT" -gt 0 ]; then
    pass "Captured $SESSION_COUNT session(s)"
    RESULTS=$((RESULTS + 1))
else
    fail "No sessions captured"
    FAILURES=$((FAILURES + 1))
fi

# Test 2: Session has events (if we have sessions)
if [ "$SESSION_COUNT" -gt 0 ]; then
    FIRST_SESSION_ID=$(echo "$SESSIONS" | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['session_id'])" 2>/dev/null || echo "")
    if [ -n "$FIRST_SESSION_ID" ]; then
        EVENTS=$(curl -sf "http://localhost:$OS_PORT/api/sessions/$FIRST_SESSION_ID/events" || echo "[]")
        EVENT_COUNT=$(echo "$EVENTS" | python3 -c "import sys,json; print(len(json.load(sys.stdin)))" 2>/dev/null || echo "0")

        if [ "$EVENT_COUNT" -gt 0 ]; then
            pass "Session $FIRST_SESSION_ID has $EVENT_COUNT events"
            RESULTS=$((RESULTS + 1))
        else
            fail "Session $FIRST_SESSION_ID has no events"
            FAILURES=$((FAILURES + 1))
        fi

        # Test 3: Events have expected subtypes
        SUBTYPES=$(echo "$EVENTS" | python3 -c "
import sys, json
events = json.load(sys.stdin)
subtypes = set()
for e in events:
    st = e.get('subtype', '')
    if st:
        subtypes.add(st.split('.')[0] + '.' + st.split('.')[1] if '.' in st else st)
print(','.join(sorted(subtypes)))
" 2>/dev/null || echo "")

        if echo "$SUBTYPES" | grep -q "message.assistant"; then
            pass "Found assistant message events (subtypes: $SUBTYPES)"
            RESULTS=$((RESULTS + 1))
        else
            fail "No assistant message events found (subtypes: $SUBTYPES)"
            FAILURES=$((FAILURES + 1))
        fi
    fi
fi

# Test 4: Claude created the output file
if [ -f "$WORK_DIR/test_output.txt" ]; then
    pass "Claude created test_output.txt"
    RESULTS=$((RESULTS + 1))
else
    fail "Claude did not create test_output.txt"
    FAILURES=$((FAILURES + 1))
fi

# --- Summary ---
echo ""
echo "================================"
echo "Results: $RESULTS passed, $FAILURES failed"
echo "================================"

[ "$FAILURES" -eq 0 ] && exit 0 || exit 1
