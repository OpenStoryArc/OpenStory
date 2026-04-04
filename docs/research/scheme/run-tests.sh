#!/bin/sh
# Run all tests for the agent loop educational module.
# Usage: cd scheme && ./run-tests.sh

set -e
cd "$(dirname "$0")"

echo "════════════════════════════════════════"
echo "  Agent Loop — Test Suite"
echo "════════════════════════════════════════"

PASS=0
FAIL=0

for f in 01-types-test.scm 02-stream-test.scm 03-tools-test.scm \
         04-eval-apply-test.scm 05-compact-test.scm 06-agent-tool-test.scm; do
  echo ""
  echo "━━━ $f ━━━"
  if chibi-scheme "$f"; then
    PASS=$((PASS + 1))
  else
    FAIL=$((FAIL + 1))
  fi
done

echo ""
echo "════════════════════════════════════════"
if [ "$FAIL" -gt 0 ]; then
  echo "  $FAIL test file(s) FAILED"
  exit 1
else
  echo "  All $PASS test files passed."
fi
echo "════════════════════════════════════════"
