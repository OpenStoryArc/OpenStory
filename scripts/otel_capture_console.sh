#!/usr/bin/env bash
# Phase 0 of the OTel ingest prototype.
#
# Launches Claude Code with the OpenTelemetry console exporter on. Every event
# the agent emits prints to stderr as it happens — the cheapest way to see what
# Claude Code's OTel schema actually looks like before we build any receivers.
#
# Usage:
#   scripts/otel_capture_console.sh           # launch a fresh Claude Code session
#   scripts/otel_capture_console.sh 2> otel-console.log  # redirect just OTel output
#
# Notes:
#   - Logs are dumped to stderr by the OTel SDK; agent UI uses stdout. Redirect
#     stderr to a file if you want to keep the records around.
#   - OTEL_LOG_USER_PROMPTS / OTEL_LOG_TOOL_DETAILS turn on opt-in payload fields
#     so we can see prompt text and tool params, not just lengths. Disable them
#     if you don't want sensitive payloads in the local log file.
#   - Export interval is set to 1 second so events appear during a short session
#     instead of being buffered for the default 5 seconds.

set -euo pipefail

export CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_LOGS_EXPORTER=console
export OTEL_METRICS_EXPORTER=console

export OTEL_LOG_USER_PROMPTS="${OTEL_LOG_USER_PROMPTS:-1}"
export OTEL_LOG_TOOL_DETAILS="${OTEL_LOG_TOOL_DETAILS:-1}"

export OTEL_LOGS_EXPORT_INTERVAL="${OTEL_LOGS_EXPORT_INTERVAL:-1000}"
export OTEL_METRIC_EXPORT_INTERVAL="${OTEL_METRIC_EXPORT_INTERVAL:-10000}"

echo "[otel-capture-console] launching claude with telemetry → console" >&2
echo "[otel-capture-console] event records will appear on stderr" >&2
exec claude "$@"
