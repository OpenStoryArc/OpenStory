# Research: Pi-Mono Message Visibility in Open Story

## Status: Findings complete, implementation needs deeper design

## Problem

Pi-mono (OpenClaw) assistant responses are invisible in Open Story's analysis tools, search, session story, and UI. User messages partially visible. Bobby and Vera (Katie's agent) both affected.

The raw JSONL has all the data — every user message, every assistant response, every tool call. Open Story drops it during the CloudEvent transformation pipeline.

## How Pi-Mono Writes Session JSONL

Pi-mono persists **one JSONL line per API call/response**. The session file is a log of Anthropic API interactions.

### The write path

```
Anthropic SSE stream
  → accumulates into single AssistantMessage object
  → message_end event fires
  → agent-session.ts: appendMessage(message)
  → session-manager.ts: _persist(entry) → appendFileSync(sessionFile, JSON.stringify(entry) + "\n")
```

Key files in pi-mono source (`/Users/maxglassie/projects/pi-mono/`):
- `packages/ai/src/providers/anthropic.ts:203-441` — streaming accumulator
- `packages/agent/src/agent-loop.ts:155-232` — eval/apply loop
- `packages/coding-agent/src/core/agent-session.ts:514-531` — event processing + persist trigger
- `packages/coding-agent/src/core/session-manager.ts:796-814` — JSONL file writer

### The eval/apply loop

```
INNER LOOP (one turn):
  1. User message → persist 1 line (role=user)
  2. EVAL: streamAssistantResponse() → LLM API call
     → persist 1 line (role=assistant, ALL content blocks from API response)
  3. Check stopReason:
     - "stop" → exit loop
     - "toolUse" → continue to APPLY
  4. APPLY: executeToolCalls()
     → persist 1 line per tool result (role=toolResult)
  5. Loop back to step 2 (next LLM call with tool results in context)
```

### Critical structural difference from Claude Code

**Claude Code** breaks the API response into separate transcript events before writing:
- Thinking → one transcript line
- Text → one transcript line  
- Tool use → one transcript line per tool

**Pi-mono** writes the raw API response as-is:
- One JSONL line = one `AssistantMessage` with `content: [thinking, text, toolCall, toolCall, ...]`

Pi-mono's approach is honest — it's the API response verbatim. Claude Code pre-decomposes.

### What a pi-mono session looks like

From Vera's (Katie's agent) session `07af8c9e`:

```
LINE 0:  type=session                          — header
LINE 1:  type=model_change                     — anthropic/claude-sonnet-4-5
LINE 2:  type=thinking_level_change            — metadata
LINE 3:  type=custom (model-snapshot)          — bookkeeping

LINE 4:  type=message role=user                — content: [{type:"text", text:"hello hello!"}]
LINE 5:  type=message role=assistant           — content: [{type:"thinking",...}, {type:"text", text:"Hey! Good to see you!"}, {type:"toolCall",...}, {type:"toolCall",...}, {type:"toolCall",...}]
         stopReason: "toolUse"
LINE 6:  type=message role=toolResult          — content: [{type:"text", text:"# IDENTITY.md..."}]
LINE 7:  type=message role=toolResult          — content: [{type:"text", text:"# USER.md..."}]
LINE 8:  type=message role=toolResult          — content: [{type:"text", text:"ENOENT..."}]
LINE 9:  type=message role=assistant           — content: [{type:"thinking",...}, {type:"text", text:"Oh! Fresh start..."}]
         stopReason: "stop"
LINE 10: type=custom (model-snapshot)          — bookkeeping
```

**Every assistant message has `thinking` as first content block.** Two shapes:
1. Conversation: `[thinking, text]` — stopReason: "stop"
2. Tool use: `[thinking, text, toolCall, ...]` — stopReason: "toolUse"

There are **zero** assistant messages with only `[text]`. Pi-mono always includes thinking.

`custom` entries with `customType: "model-snapshot"` appear after every assistant turn — bookkeeping from an extension.

## Where Open Story Drops the Data

### The translator (`rs/core/src/translate_pi.rs`)

`determine_pi_assistant_subtype()` picks ONE subtype:
```rust
if has_tool_call { return "message.assistant.tool_use"; }
if has_thinking  { return "message.assistant.thinking"; }
return "message.assistant.text";
```

Since every pi-mono assistant message has `thinking`, the subtype is either `tool_use` or `thinking`. It is **never** `text`.

The translator does extract `payload.text` from the first text block — but the subtype determines how the views layer processes it.

### The views layer (`rs/views/src/from_cloud_event.rs`)

- `message.assistant.thinking` → produces `Reasoning` records only (text block ignored)
- `message.assistant.tool_use` → produces `ToolCall` records (text block ignored)
- `message.assistant.text` → produces `AssistantMessage` (never reached for pi-mono)

**Result:** No `AssistantMessage` ViewRecords for pi-mono. The visible response text exists in `payload.text` but is never surfaced.

### Downstream effects

Because no `AssistantMessage` records exist:
- **FTS search** can't find assistant responses
- **Session story** reports `opening_prompt: null`, `prompt_timeline: []` (user messages are partially affected too — early ones don't appear)
- **Session label** may be wrong (projection extracts from first `UserMessage`)
- **Sentence detector** can't extract from assistant text
- **UI narrative view** shows only reasoning + tool calls, not the conversation

## Design Question: Where to Fix

### Option A: Translator (`translate_pi.rs`)

Emit multiple CloudEvents from a single JSONL line. A pi-mono assistant message with `[thinking, text, toolCall]` becomes 3 CloudEvents:
- `message.assistant.thinking` (the thinking block)
- `message.assistant.text` (the text block)
- `message.assistant.tool_use` (the tool calls)

**Pro:** Matches Claude Code's event model exactly. Entire downstream pipeline works unchanged.
**Con:** Breaks 1:1 relationship between JSONL lines and CloudEvents. Dedup logic may need adjustment (multiple events from same source line need unique IDs). The translator's job is to translate, not to decompose — this pushes structural transformation into the wrong layer.

### Option B: Views layer (`from_cloud_event.rs`)

Produce multiple ViewRecords from the `thinking` and `tool_use` branches when text blocks are present.

**Pro:** Doesn't change the CloudEvent model. Simple to implement.
**Con:** Papering over the structural mismatch. Every new consumer/view has to handle mixed content. The fix is invisible to anything that reads CloudEvents directly.

### Option C: New decomposition layer

Add an explicit step between translation and views that normalizes pi-mono's per-API-call events into per-content-block events. Could be a "canonicalizer" that runs after translation.

**Pro:** Clean separation of concerns. Translation stays honest (1:1 with source). Canonicalization is explicit and testable.
**Con:** New layer, new complexity.

### Recommendation

Needs more thought. The user's instinct ("we're not going to fix this in the view") is right — the views layer shouldn't be the place that understands pi-mono's message structure. But the translator decomposition approach needs careful design around dedup, ordering, and parent-child relationships.

## What's Working

- Translation of user messages (subtype `message.user.prompt`, text extracted) ✓
- Translation of tool results ✓
- FTS indexing of user message text ✓
- Session header translation ✓
- Model change translation ✓
- Skipping of `custom`, `thinking_level_change`, etc. ✓

## What's Broken

- Assistant response text invisible (no `AssistantMessage` records)
- Early user messages may not appear in records endpoint (needs investigation — search finds them but records API doesn't always return them)
- Token usage not surfacing in session synopsis (0/0)
- Sentence detector can't run on invisible messages

## Source Code References

### Pi-mono (how it writes)
- `packages/coding-agent/src/core/session-manager.ts:796-814` — `_persist()`, `appendFileSync`
- `packages/coding-agent/src/core/session-manager.ts:829-839` — `appendMessage()` creates entry
- `packages/coding-agent/src/core/agent-session.ts:514-531` — `message_end` handler
- `packages/agent/src/agent-loop.ts:155-232` — eval/apply loop
- `packages/ai/src/providers/anthropic.ts:203-441` — streaming accumulator
- `packages/ai/src/types.ts` — `AssistantMessage`, `ToolCall`, `TextContent`, `ThinkingContent` types

### Open Story (how it reads)
- `rs/core/src/translate_pi.rs:57-77` — `determine_pi_assistant_subtype()` (the bottleneck)
- `rs/core/src/translate_pi.rs:149-189` — `apply_message_fields()` (text extraction works)
- `rs/views/src/from_cloud_event.rs:100-112` — `message.user.prompt` → `UserMessage` (works)
- `rs/views/src/from_cloud_event.rs:250` — `message.assistant.thinking` → `Reasoning` only (broken)
- `rs/store/src/projection.rs:271-292` — label extraction from `UserMessage` (works if records exist)
- `rs/store/src/extract.rs:13-28` — FTS text extraction (works for all record types)
- `scripts/sessionstory.py:136-164` — story summarization (works if records are correct)

## Test Data

Katie/Vera's session on the VPS: `07af8c9e-94ef-4048-aabf-c50ce4806686`
- 53 JSONL lines, rich conversation with tool use
- Available at: `/pi-watch/katie/agents/main/sessions/07af8c9e-94ef-4048-aabf-c50ce4806686.jsonl` inside the `infra-open-story-1` container
- Also available via NATS (events flowing to local Open Story)
