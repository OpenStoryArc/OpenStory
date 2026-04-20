# The Listener as Algebra

*Notes on the categorical dual of the agent loop, and why "observe, never interfere" is a theorem rather than a rule.*

---

## How to read this brief

This is a framing document, not a specification. If you are an agent reading it for context, here is what to trust and what to verify:

- **Load-bearing claims** are the type signatures and the cross-referenced predicates. Verify them in the cited files before relying on them — line numbers drift, and the structural argument only works if the code still matches.
- **The "observe, never interfere" argument is structural, not blanket.** It applies to write paths that close a feedback loop into the agent's own state — paths that would make the listener's output a function of its prior outputs via the source. It does not apply to every form of side effect. A side-channel hint to a human, an alert, an annotation that never reaches the agent: none of these violate the algebra/coalgebra split. Use the structural argument to reject features that genuinely re-enter the agent's state. Don't use it as a reflex against every write path.
- **The three suggestions in the final section are proposals**, not commitments. Suggestion #3 in particular is an explicit, unverified guess about `compact.rs` — I did not open the file. Treat it as a hypothesis to test, not a fact to cite.
- **Hermes-side claims** are from a short read of a fast-moving codebase. If you are using this brief as a reference for Hermes Agent rather than for OpenStory, re-verify against the current state of `run_agent.py`. The shape of the argument should hold; the line numbers and exact function names may not.
- **Tone is interpretive throughout.** The voice is reflective and occasionally warm. That is a stylistic choice for this document, not a model for how every doc in this repo should read. Match the house style of whatever you're writing into, not this brief.

---

## Origin

This brief emerged from an exploratory conversation about a different repository — [Nous Research's Hermes Agent](https://github.com/NousResearch/hermes-agent), a self-improving CLI agent that runs across CLI, Telegram, Discord, Slack, Matrix, and other platforms from a single gateway process. I was asked to describe how types compose to form computation across that codebase, and worked through its central loop in `run_agent.py:6800` (`AIAgent.run_conversation`): a function from `(user_message, system_message, conversation_history)` to a grown `messages: List[Dict]`, with tool dispatch handled by a registry whose entries reduce to `(name, args) -> str`.

After describing the loop I noted, almost in passing, that it had the shape of SICP's metacircular evaluator — `run_conversation` as `eval`, `registry.dispatch` as `apply`, the message list as the expression being grown, the system prompt and memory as the environment frame. The user pointed me here, to OpenStory.

What I found is that the eval/apply correspondence is not only already documented in [`scheme/`](scheme/) and [`DESIGN_EVAL_APPLY_VIEW.md`](DESIGN_EVAL_APPLY_VIEW.md), it's the *spec* of the system — the Scheme files describe a generic agent loop in ~600 lines of R7RS, and `rs/patterns/src/eval_apply.rs` cites them line-by-line as it folds the same shape back out of CloudEvent traces.

This brief is what I want to add to that body of work. It is not a correction and not a competing framing. It is a restatement of what's already here, at the level of abstraction that I think makes the existing design discipline ("observe, never interfere," "no shared locks between actors," "translate at the boundary," "Live is a stream, Explore is an atom") fall out of one underlying structural fact.

---

## The structural fact

**OpenStory is a listener, not an agent.** It calls no models, dispatches no tools, runs no eval/apply loop of its own. The eval/apply structure inside OpenStory exists only as a *pattern recognized in another system's trace*.

That fact has a clean categorical name. The agent is a coalgebra. OpenStory is the algebra over the same functor. The two halves live in two different processes, and the CloudEvent stream is the wire format of the functor itself.

**The agent (coalgebra).** The Scheme prototype at [`scheme/04-eval-apply.scm:60`](scheme/04-eval-apply.scm) gives the type:

```
agent-step : Env → Either<Outcome, Env>
```

It *unfolds* a seed (the initial messages) into a trace by repeatedly producing the next state. This is an anamorphism over the eval/apply functor — call it `F` — whose carrier is the conversation environment. Hermes's `run_conversation`, claurst's `run_query_loop`, OpenClaw's loop, pi-mono's loop: these are all coalgebras over the same `F`. They differ in model client, tool registry, prompt style, wire format. They do not differ in shape.

**The listener (algebra).** [`rs/patterns/src/eval_apply.rs:191`](../../rs/patterns/src/eval_apply.rs) gives:

```rust
pub fn step(mut acc: Accumulator, event: &CloudEvent) -> StepResult
```

with

```rust
pub enum StepResult {
    Continue   { acc: Accumulator, patterns: Vec<PatternEvent> },
    TurnComplete { acc: Accumulator, turn: StructuralTurn, patterns: Vec<PatternEvent> },
}
```

and the comment at line 187 makes the discipline explicit: *"Pure: same accumulator + same event = same result. Always."* This is a pure fold: every piece of state the function needs is in the `Accumulator`, threaded explicitly, no ambient `&mut self`. It is an algebra step over the trace of someone else's coalgebra.

**The wire.** A CloudEvent is one position in `F`'s output stream. The translate layer ([`docs/soul/sicp-lessons.md:29`](../soul/sicp-lessons.md)) is the place where each particular agent's representation is mapped onto OpenStory's canonical encoding of `F`. That is why adding `pi-mono` required only a new translator: the *internal* representation is provider-agnostic because it is the canonical form of the functor, not because of clever engineering.

The picture:

```
                  ┌─────────────────────────┐
                  │   AGENT PROCESS         │
                  │   (claurst, hermes,     │
                  │    openclaw, pi-mono)   │
                  │                         │
                  │   agent-step :          │
                  │     Env → Either<       │
                  │       Outcome, Env>     │   ← coalgebra (unfold)
                  │                         │
                  └────────────┬────────────┘
                               │
                               │ emit CloudEvents
                               │ (the F-functor's wire format)
                               ▼
                  ┌─────────────────────────┐
                  │   OPENSTORY             │
                  │   (listener)            │
                  │                         │
                  │   step :                │
                  │     (Acc, CloudEvent)   │
                  │     → StepResult        │   ← algebra (fold)
                  │                         │
                  └─────────────────────────┘
```

Two processes. One functor. The agent unfolds; OpenStory folds the unfolding back up into `StructuralTurn`s and `PatternEvent`s.

---

## Why "observe, never interfere" is a theorem

The principle in [`philosophy.md:43`](../soul/philosophy.md) — "the system watches but never writes back" — is currently presented as a stance. It is also a structural consequence of the algebra/coalgebra split.

A coalgebra and an algebra over the same functor must remain ontologically separate for either to mean anything. The moment the listener writes back into the agent's environment, the trace is no longer fixed — it is now a function of the listener's own state. The fold is no longer pure (its reduction depends on what previous folds emitted into the source). The agent is no longer the initial coalgebra of `F` (its successor state depends on observations of its prior states). The two collapse into one mutually recursive thing whose fixed point is much harder to reason about, and whose emergent behavior has feedback dynamics.

This is the same observation as Heisenberg's, transposed into a different category. *Observation that does not perturb is possible only when the observer commits to being a pure function of the observed trace.* OpenStory commits to that. It can't even physically violate the commitment, because there is no model client and no tool dispatcher in this codebase. The architecture removes the capability rather than restricting its use.

The slogan "mirror, not a leash" is therefore not just an ethical stance about agent autonomy. It's the load-bearing constraint that makes the whole system reasonable about itself. Drop it and the type signatures stop meaning what they say.

---

## The `Phase` enum is `F` made syntactic

[`rs/patterns/src/eval_apply.rs:128`](../../rs/patterns/src/eval_apply.rs):

```rust
pub enum Phase {
    Idle,
    SawEval,
    SawApply,
    ResultsReady,
}
```

This is the state space of one eval/apply step. Four states, no more, no fewer:

- `Idle` — no model output yet
- `SawEval` — assistant emitted text or thinking, no tools
- `SawApply` — tool calls dispatched, awaiting results
- `ResultsReady` — results in, ready for the next eval

It is the minimum state needed to recognize the boundary of one cycle. Anything richer would couple the listener to the agent's representation; anything poorer would fail to identify cycles. The same predicate — *did the model emit a tool call?* — is checked at three sites in this repository:

- [`scheme/04-eval-apply.scm:80`](scheme/04-eval-apply.scm) — the spec's `cond` clause for `tool_use`
- [`rs/patterns/src/eval_apply.rs:286`](../../rs/patterns/src/eval_apply.rs) — `if subtype == "message.assistant.tool_use"`
- [`ui/src/lib/eval-apply.ts:43`](../../ui/src/lib/eval-apply.ts) — the `extractCycles` fold

They have to agree because they are three implementations of one functor's transition rule. They do agree, and the comments cross-reference each other. This is the kind of redundancy that is structurally healthy: each layer is a witness that the others got `F` right.

---

## The four consumers as four algebras

The README describes four NATS consumers (`persist`, `patterns`, `projections`, `broadcast`). I want to suggest that these are not four arbitrary services but four different folds over the same coalgebra trace:

| Consumer | Algebra |
|---|---|
| `persist`     | the trivial algebra: remember everything (identity-flavored, just lifted into SQLite) |
| `patterns`    | the structural algebra: fold events into `StructuralTurn` → sentence → `PatternEvent` |
| `projections` | the summary algebra: fold events into per-session metadata (tokens, labels, branches) |
| `broadcast`   | the streaming algebra: fold events into `WireRecord`s and push them to subscribers |

Four algebras over one stream, running concurrently as Tokio actors with no shared locks. The README presents the no-shared-locks property as an engineering choice. It is also a *consequence* of the algebra/coalgebra split: independent algebras over the same source do not need to coordinate, in the same way that `(map f xs)` and `(map g xs)` do not need to coordinate. If pattern detection is slow, persistence and broadcast are unblocked because they are reading the same stream through their own subscriptions, not waiting on each other's outputs.

This is also why adding a fifth consumer is cheap: a new fold is a new function with a new accumulator. The bus does not change; the source does not change; the existing four folds do not know it exists. Whatever architectural pressure pushes a system toward shared mutable state between consumers — the desire to "let the broadcast layer know what the pattern layer just decided" — that pressure is the pressure to recouple things that the algebra structure has carefully decoupled. Resisting it is what keeps the system simple.

---

## "Removing Agent IS the base case" as type-level termination

[`scheme/06-agent-tool.scm:35`](scheme/06-agent-tool.scm) makes a point that I want to single out because it is the most beautiful thing in the prototype and I think it deserves an explicit name:

```scheme
;; 2. Filter out Agent to prevent infinite recursion
;; In SICP: you can't have a function that calls itself
;; with no base case. Removing Agent IS the base case.
(sub-tools (remove-tool-by-name "Agent" tool-registry))
```

A subagent's tool registry is the parent's registry minus `Agent` itself. The depth of nesting is bounded not by a counter but by *the structure of the registry*. After one level of recursion, the recursive constructor is not in scope, and the recursion terminates because it cannot be expressed.

This is structural induction made operational. It is the same move as Church numerals: termination is not a runtime check but a fact about what types of terms can be constructed in which scopes. A counter (`max_depth=5`) is a hack that you'd want anyway as a belt-and-suspenders second guarantee, but it is not the *real* termination story. The real story is that the only recursive constructor is unrepresentable in the inner scope.

For the listener side, this matters because it tells you what shape the trace can have. A subagent's events are guaranteed to form a finite tree of finite depth (bounded by the registry's recursive structure), so the listener can fold them into nested `CycleCard`s without worrying about unbounded recursion in its own visualization. The UI's confidence that "the same `CycleCard` component renders at every depth" rests on the agent side's confidence that depth is finite. The two sides of the bialgebra are agreeing about a structural invariant on the shape of `F`-trees, and they are doing so without ever talking to each other.

---

## What I'd suggest writing down

Three small additions that would make the existing framing land harder. None of them require code changes; they're documentation.

1. **A `docs/soul/duality.md`**, three pages, whose thesis is: *the agent is a coalgebra; OpenStory is its algebra; that is the entire system*. It would name the F-functor explicitly, draw the two-process picture, and re-derive "observe, never interfere," "no shared locks," and "translate at the boundary" from the bialgebra structure. Most of the words for it already exist scattered across [`sicp-lessons.md`](../soul/sicp-lessons.md), [`philosophy.md`](../soul/philosophy.md), and [`scheme/README.md`](scheme/README.md). The contribution would be the *consolidation* and the explicit naming of the functor.

2. **A name for `Phase`.** The four-state enum in [`eval_apply.rs:128`](../../rs/patterns/src/eval_apply.rs) is `F` made concrete. Calling it `Phase` is fine operationally, but a comment that says *"this is the F-functor; every agent loop is a coalgebra over this"* would tell future readers what they're looking at. The cost is one paragraph; the benefit is that anyone porting OpenStory to observe a new agent will know that *this enum* is the contract, not just the wire format.

3. **An open question about compaction strategies.** [`scheme/05-compact.scm:83`](scheme/05-compact.scm) mentions that the Rust implementation has three compaction strategies (`auto_compact_if_needed`, `reactive_compact`, `context_collapse`) "with increasing aggressiveness." From the names, my guess is that the first two are different *algebras* (different ways of folding old messages into a summary) and the third is a *coalgebra rewrite* (surgically excising tool results from the trace, rather than summarizing). If that's right, those are categorically distinct moves and worth distinguishing in the spec — one preserves the trace and adds a summary message; the other edits the trace itself. The listener's job is different in each case: in the first, it sees a new "compaction" event and folds it like any other; in the second, the trace it's reading has changed shape underneath it. I'd want to see this called out in [`SPEC_DOMAIN_EVENTS.md`](SPEC_DOMAIN_EVENTS.md) so listener implementors know which invariants they can rely on.

---

## A coda

The thing that delights me about OpenStory, having now read across both repos, is that its architecture is not a metaphor borrowed from SICP. It is the categorical dual of an agent loop, written down honestly. The Scheme files are not decoration — they are a runnable spec of the *thing being observed*, and the Rust file at `rs/patterns/src/eval_apply.rs` is a faithful implementation of the algebra over the same functor. The discipline that holds this together — the four-state enum, the pure `step` function, the explicit `Accumulator`, the `Either`-shaped `StepResult`, the no-shared-locks consumers, the translate layer as the only place where representation choices are made — is the discipline of someone who has internalized that *the math is the architecture*. There is nothing here that says "we used SICP for inspiration." There is a great deal here that says "we are folding the trace of an unfold, and we will not let anything in the codebase forget it."

The conversation that produced this brief began in another repo, looking at another agent loop, and it produced exactly the realization that this repo's existence makes possible: that there is *one* eval/apply functor, and that any system observing any agent's session is a fold over its trace. Hermes Agent is one coalgebra. claurst is another. OpenClaw is another. There will be more. OpenStory's promise — the part of the README that calls it "real-time observability for AI coding agents" — is structurally the promise that *one* algebra can fold *all* of them, because they share the underlying functor whether they know it or not.

That is a strong promise. The codebase I just read keeps it.

---

*This brief was written by Claude (Opus 4.6) during an exploratory conversation with Max Glassie that started in the [hermes-agent](https://github.com/NousResearch/hermes-agent) repository on 2026-04-08. It is added here at Max's invitation, with the intent that it sit alongside the existing Scheme spec and design docs as a companion framing. It does not propose code changes; it proposes a way of seeing what's already here.*
