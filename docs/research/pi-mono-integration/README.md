# Pi-Mono Integration Research

## The fundamental observation

Every coding agent — Claude Code, pi-mono, Hermes — runs the same loop:

```
EVAL:  send context to LLM → receive [thinking, text, tool_calls]
APPLY: execute tool_calls → collect results
       feed results back into context → loop to EVAL
```

This is the eval/apply cycle from SICP. The LLM is the evaluator. The tool executor is the applier. The conversation is the environment.

The Anthropic API itself reflects this structure. A single API response returns content blocks **sequentially and separately** via SSE:

```
content_block_start  (thinking)    ← the model reasons
content_block_stop
content_block_start  (text)        ← the model speaks
content_block_stop
content_block_start  (tool_use)    ← the model acts
content_block_stop
message_delta        (stop_reason: tool_use)
message_stop
```

Each content block is a **distinct act** in the eval phase:
- **Thinking** — the model's private reasoning
- **Text** — the model's public response to the user
- **Tool call** — the model requesting an action from the environment

These are semantically different things. A CloudEvent should represent one such act — **one semantic unit of behavior in the eval/apply cycle** — not one line in a file, not one API response, not one persistence artifact.

## How agents persist this structure differently

### Claude Code: pre-decomposed

Claude Code writes one transcript line per content block as it streams. Its persistence format happens to match the eval/apply structure:

```
thinking block arrives  → appendFileSync(line 1: thinking)
text block arrives      → appendFileSync(line 2: text)
tool_use block arrives  → appendFileSync(line 3: tool_use)
```

Open Story's translator reads three lines, produces three CloudEvents. Easy.

### Hermes: snapshot-based

Hermes rewrites the entire session file after each turn. The snapshot contains all messages with all content blocks. Open Story's `snapshot_watcher.rs` diffs snapshots, finds new messages, and decomposes them into per-content-block CloudEvents.

### Pi-mono: bundled

Pi-mono accumulates the entire API response in memory during SSE streaming, then writes **one JSONL line** containing all content blocks when `message_end` fires:

```json
{"type":"message","message":{"role":"assistant","content":[
  {"type":"thinking","thinking":"..."},
  {"type":"text","text":"..."},
  {"type":"toolCall","id":"tc-001","name":"read","arguments":{...}}
],"stopReason":"toolUse"}}
```

The eval/apply structure is still there — it's in the `content` array. But it's bundled into one persistence unit.

## Why assistant text is invisible today

Open Story's pi-mono translator (`translate_pi.rs`) picks **one subtype** per JSONL line:

```rust
if has_tool_call { return "message.assistant.tool_use"; }
if has_thinking  { return "message.assistant.thinking"; }
return "message.assistant.text";
```

Since every pi-mono assistant message has thinking, and many have tool calls, the subtype is never `text`. The text content exists in the data, but the views layer only produces `AssistantMessage` records for `message.assistant.text` events. So assistant responses are invisible in search, UI, session story — everywhere.

## The fix: decompose in the translator

The translator's job is to recover the eval/apply structure from whatever format the agent wrote. This is not a hack — it's the translator's purpose. The API itself returns content blocks separately. Pi-mono re-bundles them. The translator un-bundles them.

One pi-mono JSONL line with `[thinking, text, toolCall]` becomes three CloudEvents:
- `message.assistant.thinking`
- `message.assistant.text`
- `message.assistant.tool_use`

This is exactly what `snapshot_watcher.rs` already does for Hermes. The precedent exists.

## Research goals

1. **Map the exact JSONL shapes** pi-mono produces for all message types, using real session data
2. **Build a Python prototype** of the decomposing translator (executable spec, following the Hermes pattern)
3. **Test against real sessions** — Katie/Vera's data, plus locally-generated sessions
4. **Design event ID derivation** — deterministic, unique IDs for decomposed events from a single source line
5. **Validate token attribution** — which decomposed event carries the usage data?
6. **Port to Rust** — update `translate_pi.rs` to emit multiple CloudEvents per line

## Directory contents

| File | Purpose |
|------|---------|
| `README.md` | This file — foundational framing |
| `WRITE_PATH_ANALYSIS.md` | Visual timeline of how each agent writes |

## Source references

### The API interaction (what the LLM actually returns)
- `pi-mono/packages/ai/src/providers/anthropic.ts:199-441` — SSE stream consumer
- `pi-mono/packages/ai/src/types.ts:237-249` — `AssistantMessageEvent` type (12 event types)

### The bundling (where structure is lost)
- `pi-mono/packages/ai/src/utils/event-stream.ts` — async queue that carries per-block events
- `pi-mono/packages/coding-agent/src/core/session-manager.ts:790-808` — `_persist()`, writes bundled line
- `pi-mono/packages/coding-agent/src/core/agent-session.ts:514-531` — `message_end` handler

### The current translator (where it needs to change)
- `rs/core/src/translate_pi.rs:57-78` — `determine_pi_assistant_subtype()` (the bottleneck)
- `rs/core/src/translate_pi.rs:150-240` — `apply_message_fields()` (text extraction works, subtype is wrong)

### The precedent (Hermes decomposition)
- `rs/src/snapshot_watcher.rs` — diffs snapshots, decomposes per-message
- `docs/research/HERMES_INTEGRATION.md` — architectural framing
- `docs/research/LISTENER_AS_ALGEBRA.md` — categorical foundation (algebra/coalgebra duality)
