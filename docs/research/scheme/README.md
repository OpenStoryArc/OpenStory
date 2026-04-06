*The open letter lives at the [top of this repository's README](../README.md).*

---

# The Agent Loop as Metacircular Evaluator

A runnable Scheme implementation of an AI agent's core architecture,
in the educational tradition of
[Structure and Interpretation of Computer Programs](https://mitp-content-server.mit.edu/books/content/sectbyfn/books_pres_0/6515/sicp.zip/index.html).

## What This Is

This is the agent loop from [claurst](../README.md) — a Rust reimplementation
of Claude Code — distilled to its essence in ~600 lines of Scheme.
No API calls, no HTTP, no async runtime. Just the **structure**.

The insight: an AI coding agent's architecture maps precisely to
SICP's metacircular evaluator (Chapter 4.1).

```
SICP                          Agent Loop
────                          ──────────
eval                          model API call
apply                         tool dispatch
expression                    ContentBlock (text, tool-use, tool-result, thinking)
environment                   conversation history (list of messages)
primitive procedure           built-in tool (Bash, Read, Grep)
compound procedure            Agent tool (spawns nested eval-apply loop)
garbage collection            context compaction (summarize old history)
```

And from category theory:

```
Inner loop (streaming)        algebra     (fold: Stream<Delta> → Message)
Outer loop (agent turns)      coalgebra   (unfold: State → step → ... → Outcome)
Compaction                    algebra over the coalgebra's trace
The fixed point               ContentBlock → world → ContentBlock
```

## Running It

```sh
brew install chibi-scheme   # R7RS reference implementation
cd scheme/
./run-tests.sh              # 45 tests across 6 layers
chibi-scheme 07-simulation.scm  # full simulated conversation
```

Also works on: MIT/GNU Scheme 12+, Guile 3.0+ (with `--r7rs`).

## Reading Order

Each file builds on the previous. Start at the top.

| File | Layer | SICP Parallel | Rust Source |
|------|-------|---------------|-------------|
| `00-prelude.scm` | Test framework | — | — |
| `01-types.scm` | ContentBlock, Message, Environment | §2.4 Tagged Data | `core/src/lib.rs:142-280` |
| `02-stream.scm` | Inner fold: deltas → Message | §2.2.3 Fold as universal | `api/src/lib.rs:186-248` |
| `03-tools.scm` | Tool registry + dispatch | §4.1.1 `apply` | `tools/src/lib.rs` |
| `04-eval-apply.scm` | The agent loop | §4.1.1 The evaluator | `query/src/lib.rs:406-1051` |
| `05-compact.scm` | Context compaction | §5.3 Garbage collection | `query/src/compact.rs` |
| `06-agent-tool.scm` | Nested eval-apply | §4.1.3 Compound procedures | `query/src/agent_tool.rs` |
| `07-simulation.scm` | Full demonstration | — | — |

## The Architecture in One Paragraph

The user sends a message. The **model** examines the conversation and
produces ContentBlocks — either text (done) or tool-use (keep going).
If tool-use, we **dispatch** the tool, get a result, append it to the
conversation, and call the model again. This is `eval` and `apply`
calling each other in a loop. The loop is a **coalgebra** (unfold) —
it produces the conversation coinductively, one turn at a time, until
a termination condition fires. Inside each turn, the streaming response
is a **fold** (algebra) — SSE deltas accumulate into a complete Message.
When the conversation gets too long, **compaction** summarizes it —
an algebra applied to the coalgebra's trace, using the same model
to think about its own history. The **Agent tool** spawns a nested
loop with fresh scope — a compound procedure. The evaluator evaluating
itself.

## Key Ideas to Notice

**ContentBlock is the universal type.** Everything the model says or
does is a ContentBlock variant. ToolUse maps to itself through the
world (execute → ToolResult → API → ToolUse). This fixed point is
the entire architecture.

**The mock model is just a function.** `(environment, tools) → events`.
Making `eval` an ordinary Scheme procedure strips away the mysticism.
The API call is a function. That's it.

**Tagged lists are sum types.** Scheme has no `enum` keyword, but
`(list 'tool-use id name input)` with `(eq? (car x) 'tool-use)` is
the same thing Rust does with `#[serde(tag = "type")]`. SICP Section
2.4 calls this "tagged data."

**Tail recursion is the coalgebra.** The agent loop is a tail-recursive
function that either returns (Left/terminal) or recurses (Right/continue).
There's no `while` loop, no mutation. The recursion IS the unfold.

**Compaction is self-referential.** The system uses its own model to
summarize its own conversation. This is what makes the metacircular
evaluator *metacircular* — it's written in the language it evaluates.

## References

- Abelson & Sussman, *Structure and Interpretation of Computer Programs*,
  Chapters 2.4, 4.1, 5.3
- The Rust implementation: `../src-rust/crates/{core,api,query,tools}/`
- Behavioral specifications: `../spec/01_core_entry_query.md`
