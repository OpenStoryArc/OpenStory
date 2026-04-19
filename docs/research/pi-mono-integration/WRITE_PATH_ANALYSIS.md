# Pi-Mono Write Path Analysis

## How three agents write — a visual comparison

### Claude Code: one content block per write

```
User types: "Read the config and explain it"

Anthropic SSE stream begins...

  ┌─ thinking block arrives ─────────────────────────────┐
  │  appendFileSync(session.jsonl, {thinking...})         │  ← JSONL line 1
  └───────────────────────────────────────────────────────┘

  ┌─ text block arrives ─────────────────────────────────┐
  │  appendFileSync(session.jsonl, {text...})             │  ← JSONL line 2
  └───────────────────────────────────────────────────────┘

  ┌─ tool_use block arrives ─────────────────────────────┐
  │  appendFileSync(session.jsonl, {tool_use...})         │  ← JSONL line 3
  └───────────────────────────────────────────────────────┘

File watcher sees: 3 writes, 3 events, 3 subtypes
  → message.assistant.thinking
  → message.assistant.text
  → message.assistant.tool_use
```

### Hermes: whole-file snapshot rewrite per turn

```
User types: "Read the config and explain it"

Anthropic SSE stream begins...
  thinking, text, tool_calls all accumulate in memory...
  ...stream ends, message_end fires

  ┌─ ENTIRE session file rewritten ──────────────────────┐
  │  writeFileSync(session.json, [                        │
  │    {role: system, ...},                               │
  │    {role: user, content: "Read the config..."},       │
  │    {role: assistant, reasoning: "...",                 │  ← ALL blocks
  │     content: "I'll read...",                          │     in ONE
  │     tool_calls: [{name: "read", args: {...}}]},       │     message
  │  ])                                                   │
  └───────────────────────────────────────────────────────┘

File watcher sees: 1 write, diffs against previous snapshot
  snapshot_watcher.rs extracts the NEW message
  → decomposes into 3 CloudEvents (thinking, text, tool_use)
```

### Pi-Mono: buffer-then-flush, one line per API response

```
User types: "Read the config and explain it"

Anthropic SSE stream begins...

  ┌─ IN MEMORY (nothing on disk yet) ───────────────────┐
  │                                                       │
  │  content_block_start  → thinking block                │
  │  content_block_delta  → "The config file is..."       │
  │  content_block_stop                                   │
  │                                                       │
  │  content_block_start  → text block                    │
  │  content_block_delta  → "This is a TOML..."           │
  │  content_block_stop                                   │
  │                                                       │
  │  content_block_start  → toolCall block                │
  │  content_block_delta  → {"path": "/config.toml"}      │
  │  content_block_stop                                   │
  │                                                       │
  │  message_end fires                                    │
  └───────────────────────────────────────────────────────┘
           │
           ▼
  ┌─ SINGLE appendFileSync ─────────────────────────────┐
  │  {                                                    │
  │    type: "message",                                   │
  │    message: {                                         │
  │      role: "assistant",                               │
  │      content: [                                       │
  │        {type: "thinking", thinking: "The config..."},│  ← ALL blocks
  │        {type: "text", text: "This is a TOML..."},    │     bundled
  │        {type: "toolCall", name: "read", args: {...}} │     into ONE
  │      ],                                               │     JSONL line
  │      stopReason: "toolUse",                           │
  │      usage: {input: 150, output: 75, ...}             │
  │    }                                                  │
  │  }                                                    │
  └───────────────────────────────────────────────────────┘

File watcher sees: 1 write, 1 line, but 3 content blocks inside
  translate_pi.rs picks ONE subtype (tool_use wins)
  → message.assistant.tool_use         ← text is INVISIBLE
```

## The buffer-then-flush detail

Pi-mono has an additional wrinkle: it doesn't write **anything** until the first assistant response completes.

```
Session starts...

  User prompt arrives:
    fileEntries.push({type: "message", role: "user", ...})
    → NO DISK WRITE (flushed = false)

  Session header:
    fileEntries.push({type: "session", ...})
    → NO DISK WRITE

  First assistant response completes (message_end):
    fileEntries.push({type: "message", role: "assistant", ...})

    if (!flushed) {
      // BULK WRITE: all accumulated entries at once
      for (entry of fileEntries) {
        appendFileSync(sessionFile, JSON.stringify(entry) + "\n")
      }
      flushed = true
    }

  Subsequent messages:
    // One appendFileSync per entry, immediately
    appendFileSync(sessionFile, JSON.stringify(entry) + "\n")
```

File watcher timeline:
```
t=0s  Session starts          → nothing on disk
t=1s  User types prompt       → nothing on disk
t=3s  SSE stream begins       → nothing on disk
t=8s  SSE stream ends         → BURST: 3+ lines appear at once
t=9s  Tool result comes back  → 1 line appended
t=12s Next assistant response → 1 line appended (still bundled)
```

## Where observation can happen

```
                    Anthropic API
                         │
                    SSE stream
                         │
              ┌──────────┴──────────┐
              │   anthropic.ts       │
              │   (event loop)       │
              │                      │
              │  content_block_start ─── subscribe() → message_update events
              │  content_block_delta ─── (per-delta, per-block granularity)
              │  content_block_stop  ─── ◄── CAN OBSERVE HERE (hooks/extension)
              │  message_end         │
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │  session-manager.ts  │
              │  _persist()          │
              │  appendFileSync()    │  ◄── WRITTEN TO DISK (bundled)
              └──────────┬──────────┘
                         │
              ┌──────────┴──────────┐
              │  Open Story          │
              │  file watcher        │  ◄── CAN OBSERVE HERE (file changes)
              │  → translate_pi.rs   │      but only sees bundled lines
              └─────────────────────┘
```

**Two observation points:**
1. **File watcher** (current) — sees complete, bundled JSONL lines after `message_end`
2. **Extension/subscribe API** — sees per-content-block events during streaming

The file watcher path requires the translator to **decompose** bundled lines.
The extension path could write **pre-decomposed** events — but that means modifying how pi-mono persists, or adding a parallel write stream.

## The eval/apply loop

```
PI-MONO TURN (one user prompt → final response):

  ┌─────────────────────────────────────────────────┐
  │ 1. User message                                  │
  │    → persist: {role: "user", content: [...]}     │  1 JSONL line
  └───────────────────┬─────────────────────────────┘
                      │
  ┌───────────────────▼─────────────────────────────┐
  │ 2. EVAL: streamAssistantResponse()               │
  │    → API call to Anthropic                        │
  │    → accumulate: thinking + text + toolCalls      │
  │    → message_end fires                            │
  │    → persist: {role: "assistant", content: [...]} │  1 JSONL line
  │                                                   │  (ALL blocks)
  │    stopReason: "toolUse" → continue to APPLY      │
  │    stopReason: "stop"    → exit loop              │
  └───────────────────┬─────────────────────────────┘
                      │ (if toolUse)
  ┌───────────────────▼─────────────────────────────┐
  │ 3. APPLY: executeToolCalls()                     │
  │    → run each tool                                │
  │    → persist per tool: {role: "toolResult", ...}  │  1 JSONL line
  │                                                   │  per tool
  └───────────────────┬─────────────────────────────┘
                      │
                      ▼ loop back to EVAL (step 2)
```

Each EVAL step produces exactly ONE JSONL line containing ALL content blocks.
This is the fundamental unit the translator must decompose.
