# Design: Eval-Apply Structural View for OpenStory

*A proposal for showing the computational structure of agent sessions.*

## The Idea

OpenStory currently shows **what** happened in a session: messages, tool calls, results, in a timeline. This proposal adds a view that shows **why** — the computational structure underneath.

Every agent session is an eval-apply loop. OpenStory already has the data to make that visible. This design describes a new **pattern detector** (the eval-apply detector) and a **UI mode** (structural view) that together let a student watch the metacircular evaluator unfold in real time.

## What the Student Sees

A toggle on the session view: "Show structure." The same events, annotated:

- **EVAL** phases highlighted: model examining the conversation, producing a response
- **APPLY** phases highlighted: tool being dispatched, world changing, result returning
- **FOLD** markers: streaming deltas accumulating into a complete message (the inner algebra)
- **TURN** boundaries: one step of the coalgebra completing
- **SCOPE** nesting: when an Agent tool creates a sub-loop, the nested turns are indented
- **GC** events: compaction summarizing old messages, environment shrinking
- **TERMINATE/CONTINUE** annotations: at each turn boundary, why the loop continued or stopped

The environment (conversation history) grows visibly with each turn. A counter shows message count and estimated token usage. When compaction fires, the counter drops — visible garbage collection.

## Architecture: The Eval-Apply Detector

A new pattern detector implementing the existing `Detector` trait.

### State Machine

```
                    ┌──────────────────────┐
                    │                      │
                    ▼                      │
    ┌──────────┐  assistant_message  ┌────────────┐
    │          │ ──────────────────▶ │            │
    │  IDLE    │                     │  SAW_EVAL  │
    │          │ ◀────────────────── │            │
    └──────────┘    end_turn         └────────────┘
         ▲               │                │
         │               │            tool_call
         │               ▼                │
         │        ┌──────────┐           ▼
         │        │ TURN_END │    ┌──────────────┐
         │        └──────────┘    │              │
         │                        │  SAW_APPLY   │
         │                        │              │
         │                        └──────────────┘
         │                              │
         │                         tool_result
         │                              │
         │                              ▼
         │                     ┌────────────────┐
         │                     │                │
         └──────────────────── │ RESULTS_READY  │
              (next eval)      │                │
                               └────────────────┘
```

### What It Emits

On each state transition, the detector emits a `PatternEvent` with:

```rust
struct EvalApplyEvent {
    phase: Phase,           // Eval, Apply, Fold, GC, Scope
    turn_number: u32,       // which coalgebra step
    scope_depth: u32,       // 0 = root, 1+ = nested agent
    tool_name: Option<String>,
    stop_reason: Option<String>,  // end_turn, tool_use, max_tokens
    env_size: u32,          // messages in conversation so far
    event_ids: Vec<String>, // the events that comprise this phase
}

enum Phase {
    EvalStart,      // model call begins
    FoldDelta,      // streaming delta received (inner algebra step)
    FoldComplete,   // message fully accumulated
    ApplyStart,     // tool dispatch begins
    ApplyEnd,       // tool result received
    TurnEnd,        // coalgebra step complete
    ScopeOpen,      // Agent tool → nested eval-apply begins
    ScopeClose,     // nested loop returns
    Compact,        // GC fires
}
```

### Detection Logic

```rust
fn feed(&mut self, ctx: &FeedContext) -> Vec<PatternEvent> {
    let record = &ctx.record;
    let mut events = Vec::new();

    match &record.body {
        // Model produced a response → eval phase
        RecordBody::AssistantMessage { .. } => {
            self.turn_number += 1;
            events.push(self.emit(Phase::EvalStart));
            events.push(self.emit(Phase::FoldComplete));

            // Check for tool_use blocks → will continue
            if self.has_tool_use(record) {
                self.state = State::SawEval;
            } else {
                events.push(self.emit_turn_end("end_turn"));
            }
        }

        // Tool dispatched → apply phase
        RecordBody::ToolCall { name, .. } => {
            if name == "Agent" {
                self.scope_depth += 1;
                events.push(self.emit(Phase::ScopeOpen));
            }
            events.push(self.emit_apply_start(name));
            self.state = State::SawApply;
        }

        // Tool result → apply complete
        RecordBody::ToolResult { .. } => {
            events.push(self.emit(Phase::ApplyEnd));
            self.state = State::ResultsReady;
        }

        // Turn boundary
        RecordBody::TurnEnd { .. } => {
            events.push(self.emit_turn_end("tool_use"));
            self.state = State::Idle;
        }

        // Compaction
        RecordBody::SystemEvent { subtype, .. }
            if subtype == "system.compact" =>
        {
            events.push(self.emit(Phase::Compact));
        }

        _ => {}
    }

    // Track environment size
    if matches!(&record.body,
        RecordBody::UserMessage { .. } | RecordBody::AssistantMessage { .. }
    ) {
        self.env_size += 1;
    }

    events
}
```

### Integration

The detector plugs into OpenStory's existing pattern pipeline. No new infrastructure. `PatternEvent`s are persisted to SQLite alongside existing patterns (test cycles, error recovery, etc.) and broadcast to the UI via the existing WebSocket enriched messages.

## Architecture: The Structural View (UI)

### Data Flow

```
PatternEvent (eval-apply) ──┐
                            ├──▶ StructuralViewModel ──▶ React component
ViewRecord (existing)  ─────┘
```

The UI receives both the raw ViewRecords (for content) and the EvalApply PatternEvents (for structure). The view model combines them:

```typescript
interface StructuralTurn {
  turnNumber: number;
  scopeDepth: number;
  evalPhase: {
    messageId: string;
    content: string;       // from ViewRecord
    foldDeltaCount: number; // how many streaming chunks
    stopReason: string;
  };
  applyPhases: Array<{
    toolName: string;
    input: string;         // from ToolCall ViewRecord
    output: string;        // from ToolResult ViewRecord
    durationMs: number;
    nestedScope?: StructuralTurn[];  // if Agent tool
  }>;
  envSize: number;         // messages after this turn
  isTerminal: boolean;     // did the loop end here?
}
```

### Rendering

Each turn renders as a card with:
- Turn number and scope depth (indentation for nesting)
- EVAL section: the model's message, with stop reason badge
- APPLY section(s): each tool call with input/output, collapsible
- Environment counter: "Environment: 5 messages (~2.1k tokens)"
- Continue/Terminate indicator

Compaction events render as a special "GC" card showing before/after environment size.

Agent tool calls expand inline to show the nested scope's turns — recursion made visible.

### Annotations (Educational)

Optional tooltip annotations that explain what's happening:

- On EVAL: "The model examines the conversation (environment) and produces content blocks. This is `eval` in the metacircular evaluator."
- On APPLY: "The tool is dispatched with its input. This is `apply` — the operator is the tool name, the operand is the input."
- On CONTINUE: "stop_reason is `tool_use`, so the loop continues. The coalgebra takes another step."
- On TERMINATE: "stop_reason is `end_turn`. The coalgebra terminates. This is the Left branch of Either<Outcome, State>."
- On nested scope: "The Agent tool spawns a new eval-apply loop with a fresh environment. This is a compound procedure — SICP Section 4.1.3."
- On compaction: "The environment exceeded the context window. Old messages are summarized. This is garbage collection — SICP Section 5.3."

These annotations can be toggled on/off. Default: on for first-time visitors, off for returning users.

## Metrics Derived from the Detector

Once the eval-apply detector runs, you get quantitative metrics for free:

- **Turns to resolution**: how many coalgebra steps before end_turn?
- **Tool chain depth**: longest sequence of apply phases in a single turn?
- **Scope nesting depth**: deepest Agent tool recursion?
- **Compaction frequency**: how often does GC fire?
- **Eval/Apply ratio**: time spent in model calls vs tool execution?
- **Environment growth rate**: messages per turn?
- **Fold size**: average streaming deltas per message?

These are the "Agent DORA metrics" that pi-mono proposed — but grounded in the actual computational structure rather than ad-hoc heuristics.

## Implementation Sequence

1. **Eval-Apply Detector** (Rust, `rs/patterns/src/eval_apply.rs`) — implement the state machine, emit PatternEvents. Write tests using the existing detector test infrastructure.

2. **Persist & Broadcast** — PatternEvents already persist and broadcast. No new infrastructure needed. Just register the new detector in the pipeline.

3. **StructuralViewModel** (TypeScript, `ui/src/lib/`) — pure function that combines ViewRecords + EvalApply PatternEvents into the StructuralTurn tree.

4. **StructuralView component** (React, `ui/src/components/`) — renders the turn cards with nesting, annotations, and env counters.

5. **Toggle** — add "Show structure" toggle to session view header. Persists in localStorage.

6. **Annotations** — educational tooltips with SICP references. Link to the Scheme code and LINEAGE.md for students who want to go deeper.

## Dependencies

- The monadic EventData refactor (in progress on `feat/typed-event-data`) makes this easier by giving typed access to tool names, stop reasons, and content blocks. But the detector can work with the current ViewRecord types too — it only needs `record_type` and a few payload fields.

- The existing pattern detection infrastructure handles persistence, broadcast, and UI delivery. This is just a new detector plugged into the same pipeline.

## What This Enables

A student opens OpenStory, watches an agent session, and sees the metacircular evaluator running in real time. They can trace eval calling apply calling eval. They can see the environment grow. They can watch compaction fire and the environment shrink. They can see a compound procedure open a nested scope and return a value.

Then they open the Scheme code in this repo and see the same structure in 600 lines they can run in a REPL.

Then they read SICP Chapter 4 and see it again, in the language it was first expressed in.

Three mirrors, same structure, forty years apart.
