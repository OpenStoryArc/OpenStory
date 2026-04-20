# Pi-Mono API Interaction Map

Captured 2026-04-13 from real pi-mono sessions via `test-pimono-shapes.mjs`.
API trace captured via `api-proxy.mjs` on localhost:9090.

---

## Scenario 01: Text Only (no bug)

```
┌─────────────────────────────────────────────────────────────────────┐
│ USER: "Say hello in exactly one sentence."                          │
└────────────────────────────┬────────────────────────────────────────┘
                             │
                    ┌────────▼────────┐
                    │  API CALL 1     │
                    │  POST /v1/msgs  │
                    │  1 user msg     │
                    │  4 tools        │
                    └────────┬────────┘
                             │
              ┌──────────────▼──────────────────────────────┐
              │  SSE RESPONSE                                │
              │                                              │
              │  content_block_start  index=0  type=text     │
              │  content_block_delta  "Hello, welcome —"     │
              │  content_block_stop                          │
              │  message_delta  stop_reason=end_turn         │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  content: [{type:"text", text:"Hello..."}]   │
              │  stopReason: "stop"                          │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  TRANSLATOR TODAY                            │
              │                                              │
              │  picks: message.assistant.text  ✓ CORRECT    │
              │  visible: [text]                             │
              │  invisible: []                               │
              └──────────────────────────────────────────────┘
```

---

## Scenario 04: Thinking + Text (BUG — text invisible)

```
┌─────────────────────────────────────────────────────────────────────┐
│ USER: "Think carefully about why 1+1=2, then give me an answer."    │
└────────────────────────────┬────────────────────────────────────────┘
                             │
                    ┌────────▼────────┐
                    │  API CALL 1     │
                    │  POST /v1/msgs  │
                    └────────┬────────┘
                             │
              ┌──────────────▼──────────────────────────────┐
              │  SSE RESPONSE                                │
              │                                              │
              │  content_block_start  index=0  type=thinking │
              │  content_block_delta  ~20 chars reasoning     │
              │  content_block_stop                          │
              │                                              │
              │  content_block_start  index=1  type=text     │
              │  content_block_delta  ×3  (147 chars total)  │
              │  content_block_stop                          │
              │                                              │
              │  message_delta  stop_reason=end_turn         │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  content: [                                  │
              │    {type:"thinking", thinking:"..."},        │
              │    {type:"text", text:"1+1=2 because..."}   │
              │  ]                                           │
              │  stopReason: "stop"                          │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  TRANSLATOR TODAY                            │
              │                                              │
              │  has_thinking=true → picks: .thinking        │
              │                                              │
              │  visible:   [thinking]                       │
              │  INVISIBLE: [text] ← "1+1=2 because..."     │
              │                                              │
              │  ⚠ The user's actual answer is LOST          │
              └──────────────────────────────────────────────┘
```

---

## Scenario 06: Thinking + Text + Tool (BUG — worst case)

```
┌─────────────────────────────────────────────────────────────────────┐
│ USER: "Think about what test-broken.py does wrong, explain, read."  │
└────────────────────────────┬────────────────────────────────────────┘
                             │
                    ┌────────▼────────┐
                    │  API CALL 1     │
                    │  POST /v1/msgs  │
                    │  1 user msg     │
                    └────────┬────────┘
                             │
              ┌──────────────▼──────────────────────────────────────┐
              │  SSE RESPONSE                                        │
              │                                                      │
              │  content_block_start  index=0  type=thinking         │
              │  content_block_delta  "The user wants me to read..." │
              │  content_block_stop                                  │
              │                                                      │
              │  content_block_start  index=1  type=text             │
              │  content_block_delta  "Let me read the file first"   │
              │  content_block_stop                                  │
              │                                                      │
              │  content_block_start  index=2  type=tool_use         │
              │     name="read"  id="toolu_013B..."                  │
              │  content_block_delta  {"path":"/tmp/test-broken.py"} │
              │  content_block_stop                                  │
              │                                                      │
              │  message_delta  stop_reason=tool_use                 │
              └──────────────────────────┬───────────────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  content: [                                  │
              │    {type:"thinking", thinking:"The user.."}, │
              │    {type:"text", text:"Let me read..."},     │
              │    {type:"toolCall", name:"read", args:{..}} │
              │  ]                                           │
              │  stopReason: "toolUse"                       │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  TRANSLATOR TODAY                            │
              │                                              │
              │  has_tool_call=true → picks: .tool_use       │
              │                                              │
              │  visible:   [toolCall]                       │
              │  INVISIBLE: [thinking, text]                 │
              │                ↑            ↑                │
              │         reasoning    "Let me read the        │
              │                       file first..."         │
              │                                              │
              │  ⚠ TWO content blocks LOST from one line    │
              └──────────────────────────┬───────────────────┘
                                         │
      ┌──────────────────────────────────▼──────────────────┐
      │  TOOL EXECUTION (pi-mono runs the tool)             │
      │  read("/tmp/test-broken.py") → file content         │
      └──────────────────────────────────┬──────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  role: "toolResult"                          │
              │  toolCallId: "toolu_013B..."                 │
              │  content: [{text: "def fib(n):..."}]         │
              │  isError: false                              │
              └──────────────────────────┬───────────────────┘
                                         │
                    ┌────────────────────▼────────┐
                    │  API CALL 2                  │
                    │  POST /v1/msgs               │
                    │  3 msgs: user+assistant+tool  │
                    └────────────────────┬─────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  SSE RESPONSE                                │
              │                                              │
              │  content_block_start  index=0  type=text     │
              │  content_block_delta  ×10  (650 chars)       │
              │  content_block_stop                          │
              │                                              │
              │  message_delta  stop_reason=end_turn         │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  content: [{type:"text", text:"The bug..."}] │
              │  stopReason: "stop"                          │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  TRANSLATOR TODAY                            │
              │                                              │
              │  picks: message.assistant.text  ✓ CORRECT    │
              │  (this call had only one block, so no loss)  │
              └──────────────────────────────────────────────┘
```

---

## Scenario 07: Multi-Tool (partial loss)

```
┌─────────────────────────────────────────────────────────────────────┐
│ USER: "Read both config.toml and test-broken.py at the same time."  │
└────────────────────────────┬────────────────────────────────────────┘
                             │
                    ┌────────▼────────┐
                    │  API CALL 1     │
                    └────────┬────────┘
                             │
              ┌──────────────▼──────────────────────────────┐
              │  SSE RESPONSE                                │
              │                                              │
              │  content_block_start  index=0  type=tool_use │
              │     name="read"  path="/tmp/test-config..."  │
              │  content_block_stop                          │
              │                                              │
              │  content_block_start  index=1  type=tool_use │
              │     name="read"  path="/tmp/test-broken..."  │
              │  content_block_stop                          │
              │                                              │
              │  message_delta  stop_reason=tool_use         │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  PI-MONO PERSISTS (1 JSONL line)             │
              │                                              │
              │  content: [                                  │
              │    {type:"toolCall", name:"read", args:{..}},│
              │    {type:"toolCall", name:"read", args:{..}} │
              │  ]                                           │
              │  stopReason: "toolUse"                       │
              └──────────────────────────┬───────────────────┘
                                         │
              ┌──────────────────────────▼───────────────────┐
              │  TRANSLATOR TODAY                            │
              │                                              │
              │  picks: message.assistant.tool_use           │
              │  captures FIRST tool only                    │
              │                                              │
              │  visible:   [toolCall #1]                    │
              │  INVISIBLE: [toolCall #2]                    │
              │                                              │
              │  ⚠ Second parallel tool call LOST            │
              └──────────────────────────────────────────────┘
```

---

## The Pattern

```
                    What the API returns        What pi-mono writes      What the translator sees
                    (per content block)         (bundled per response)    (picks ONE subtype)

Scenario 01:        [text]                  →   [text]               →   text           ✓
Scenario 02:        [toolCall]              →   [toolCall]           →   tool_use       ✓
Scenario 03:        [toolCall] then [text]  →   [toolCall], [text]   →   tool_use, text ✓
Scenario 04:        [thinking, text]        →   [thinking, text]     →   thinking       ⚠ text lost
Scenario 05:        [thinking, text, tool]  →   [thinking,text,tool] →   tool_use       ⚠ thinking+text lost
Scenario 06:        [thinking, text, tool]  →   [thinking,text,tool] →   tool_use       ⚠ thinking+text lost
Scenario 07:        [toolCall, toolCall]    →   [toolCall, toolCall] →   tool_use       ⚠ 2nd tool lost
Scenario 08:        [toolCall] then [text]  →   [toolCall], [text]   →   tool_use, text ✓

Data loss occurs whenever a single API response contains >1 content block type.
The API gives them to us separately. Pi-mono bundles them. The translator picks one.
```

---

## The Fix (what a decomposing translator would produce)

```
Scenario 06, API call 1:

  JSONL line (today):
    1 line → 1 CloudEvent (tool_use) → thinking + text INVISIBLE

  JSONL line (with decomposition):
    1 line → 3 CloudEvents:
      message.assistant.thinking   ← "The user wants me to read..."
      message.assistant.text       ← "Let me read the file first..."
      message.assistant.tool_use   ← read("/tmp/test-broken.py")

  All content preserved. Same structure the API returned.
```
