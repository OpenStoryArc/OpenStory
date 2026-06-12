# OTel vs. transcript JSONL — empirical comparison

A side-by-side capture of Claude Code's two observation channels — the
append-only JSONL transcript OpenStory already watches, and the
OpenTelemetry stream introduced in Claude Code 2.x — to answer one
concrete question for the BACKLOG entry *"Cowork OTel exporter as second
observation channel"*: **is OTel a replacement for the transcript, a
strict superset, or a complementary sidecar?**

**Answer (verified 2026-06-12 on a real session):** with the full content
gates enabled (`OTEL_LOG_TOOL_CONTENT=1` +
`OTEL_LOG_RAW_API_BODIES=file:...`), OTel is a *content superset* of the
JSONL — it carries everything the transcript does *and* additional
structural signal (timing, cost, permission decisions, span hierarchy,
tool schemas, system prompt, model-side sampling params) that the
transcript fundamentally can't reconstruct. With the default-redacted
flags, OTel is a structural sidecar only. The JSONL remains the
sovereignty escape hatch (local-first, append-only, agent's
on-disk view), so the two should be ingested together, not as
alternatives.

The comparison script: [`scripts/otel_vs_jsonl.py`](../../scripts/otel_vs_jsonl.py).

## The four content gates (most important reading)

The default OTel export carries cost/timing/decision telemetry but
**redacts every content surface** — prompts, tool inputs, tool outputs,
API bodies. Content layers are enabled per type so an organization can
take cost/timing without auto-exporting prompts (or vice versa):

| Env var | What it adds |
|---|---|
| `OTEL_LOG_USER_PROMPTS=1` | Prompt text on `user_prompt` events + `claude_code.interaction` spans |
| `OTEL_LOG_TOOL_DETAILS=1` | Tool inputs/parameters on events; `full_command`/`file_path`/`skill_name` span attrs (4 KB truncation) |
| `OTEL_LOG_TOOL_CONTENT=1` | A `tool.output` span event with input + output bodies, 60 KB cap per attribute |
| `OTEL_LOG_RAW_API_BODIES=file:<dir>` | Full Anthropic Messages API request/response JSON in a sidecar dir, with `body_ref` pointer events (or inline+60 KB cap without `file:`) |

A claim like *"OTel doesn't carry distributed-tracing content"* is wrong
*by default* and wrong with three of four flags on — but it's exactly
right with one of them off. Worth keeping in mind when reading vendor
docs.

## Local lab recipe

### Collector

Stand up a single OTel Collector container that fans out to `debug`,
`prometheus`, and `file` exporters:

```yaml
# captures/otel-lab/otel-collector.yaml
receivers:
  otlp:
    protocols:
      grpc: { endpoint: 0.0.0.0:4317 }
      http: { endpoint: 0.0.0.0:4318 }

processors:
  batch:

exporters:
  debug:
    verbosity: basic
  file:
    path: /otel-data/otel-capture.jsonl
  prometheus:
    endpoint: 0.0.0.0:8889

service:
  pipelines:
    metrics: { receivers: [otlp], processors: [batch], exporters: [file, debug, prometheus] }
    logs:    { receivers: [otlp], processors: [batch], exporters: [file, debug] }
    traces:  { receivers: [otlp], processors: [batch], exporters: [file, debug] }
```

```bash
mkdir -p captures/otel-lab/data && chmod 777 captures/otel-lab/data
docker run -d --name otel-lab \
  -p 127.0.0.1:4317:4317 -p 127.0.0.1:4318:4318 -p 127.0.0.1:8889:8889 \
  -v "$PWD/captures/otel-lab/otel-collector.yaml:/etc/otelcol-contrib/config.yaml:ro" \
  -v "$PWD/captures/otel-lab/data:/otel-data" \
  otel/opentelemetry-collector-contrib:latest
```

### Claude Code

In a fresh terminal (env is read at process startup — re-exporting after
`claude` is running will not take effect):

```bash
export CLAUDE_CODE_ENABLE_TELEMETRY=1
export OTEL_METRICS_EXPORTER=otlp
export OTEL_LOGS_EXPORTER=otlp
export OTEL_TRACES_EXPORTER=otlp
export CLAUDE_CODE_ENHANCED_TELEMETRY_BETA=1     # traces are beta-gated
export OTEL_EXPORTER_OTLP_PROTOCOL=grpc
export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
export OTEL_METRIC_EXPORT_INTERVAL=10000
export OTEL_LOGS_EXPORT_INTERVAL=5000
# Full content (omit any of these to leave that surface redacted):
export OTEL_LOG_USER_PROMPTS=1
export OTEL_LOG_TOOL_DETAILS=1
export OTEL_LOG_TOOL_CONTENT=1
export OTEL_LOG_RAW_API_BODIES=file:$PWD/captures/otel-lab/data/api-bodies

claude
```

Run a few prompts that exercise different tools (a `Read`, a `Bash`, an
`Edit`) and quit.

### Compare

```bash
python3 scripts/otel_vs_jsonl.py --list                   # sessions captured
python3 scripts/otel_vs_jsonl.py <SESSION_ID>             # rendered diff
python3 scripts/otel_vs_jsonl.py <SESSION_ID> --json      # machine-readable
python3 scripts/otel_vs_jsonl.py --test                   # synthetic-fixture smoke test
```

The script auto-resolves transcripts under both `~/.claude/projects/`
(CLI sessions) and
`~/Library/Application Support/Claude/local-agent-mode-sessions/`
(Cowork sessions, once enabled).

## Findings on a real session

One ~30 minute session with the full content gates enabled — Bash,
Read, Edit, AskUserQuestion, ToolSearch tools exercised.

| Channel | Volume | What's exclusively here |
|---|---|---|
| JSONL transcript | 490 KB, 208 records | `mode`/`permission-mode`/`ai-title`/`last-prompt` lifecycle records; agent's append-only local log |
| OTel events (log records) | 141 | `duration_ms`, `decision_source`, `success`, sizes, `prompt.id` correlation, `event.sequence`, structured `bash_command`/`full_command` split |
| OTel spans (traces) | 148 | `interaction` → `llm_request` / `tool` tree; per-tool **permission-wait vs execution** split; `gen_ai.*` semantic conventions; `traceparent` propagated to subprocesses |
| OTel api-bodies sidecar | 2.4 MB on disk | Literal Anthropic API requests + responses — full conversation, 14 tool schemas, 8 263-char system prompt, sampling params |

**The join key is `tool_use_id`.** Same value on the JSONL tool_use
block, on the OTel `tool_result` event, on the parent `claude_code.tool`
span, and on the `tool.output` span event. This means an OTel ingest
pipeline can enrich existing OpenStory records keyed on
`(session_id, tool_use_id)` rather than running as a parallel store —
the right shape for a future implementation.

## Implications for OpenStory

A few concrete things the data established for future planning:

- **OTel is additive, not duplicative.** The JSONL stays canonical for
  content (and for sovereignty: agent-perspective, local-first,
  append-only). OTel adds timing, cost, permission decisions, and the
  span tree the transcript can't carry.
- **`body_ref` is the same pattern OpenStory already uses.** Tiny event
  + pointer to a sidecar dir; full body on demand. Mirrors the
  truncate-and-`/content` flow in `state.store.full_payloads`.
- **OTel knows things the transcript doesn't.** Tool schemas, system
  prompt content, `max_tokens`/`thinking`/`context_management` params,
  `gen_ai.usage.input_tokens` per request — none reach disk on the
  watcher side.
- **The redaction default is the right default.** Most orgs won't want
  content auto-exported to Datadog/Honeycomb. The per-type gates let
  OpenStory ingest *cost-and-timing only* from the OTel stream as a
  conservative default, and turn on content layers per deployment.
- **Cowork applicability is mechanical, not architectural.** Cowork is
  Claude Code in a VM (PR #71), so the same env vars and OTel surface
  apply once the desktop app exposes the toggle (it advertises OTel
  support per the April 2026 release notes). The lab in this doc
  doubles as the verification harness when that lands.
