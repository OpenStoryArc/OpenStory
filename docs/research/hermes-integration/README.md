# Hermes Integration Prototype

This directory contains a prototype for letting OpenStory observe Hermes Agent sessions, and letting Hermes agents query OpenStory for structural views of their own past work. It is the actionable companion to [`../HERMES_INTEGRATION.md`](../HERMES_INTEGRATION.md), which describes the architectural framing and the design decisions.

## What's here

| File | Status | What it is |
|---|---|---|
| [`translate_hermes.py`](translate_hermes.py) | runnable sketch | Python prototype of the Hermes-native → CloudEvent translator. The Rust port lives in OpenStory at `rs/core/src/translate_hermes.rs` (not yet written) — this is the executable spec. |
| [`example_hermes_events.jsonl`](example_hermes_events.jsonl) | synthetic data | What the Hermes plugin would write to its output directory: one Hermes-native event per line, in the shape the in-memory `messages: List[Dict]` already uses, plus a small envelope (`session_id`, `timestamp`, `source`). |
| [`test_translate.py`](test_translate.py) | runnable test | Loads the example, runs the translator, asserts on the resulting CloudEvent shape. Run with `python test_translate.py` — no dependencies. |
| [`plugin_sketch.py`](plugin_sketch.py) | sketch + `# VERIFY:` markers | The Hermes-side plugin: a `register(ctx)` function that hooks `post_llm_call`, `post_tool_call`, and `on_session_finalize`, plus the recall tool. |
| [`recall_tool_sketch.py`](recall_tool_sketch.py) | sketch | The OpenStory-querying tool, separated for clarity. Imported by `plugin_sketch.py`. |
| [`pyproject.toml.example`](pyproject.toml.example) | example | Entry-point declaration showing how the standalone `hermes-openstory` package would self-register with Hermes via `[project.entry-points."hermes_agent.plugins"]`. |
| [`DISTRIBUTION_PLAN.md`](DISTRIBUTION_PLAN.md) | plan | What it takes to ship the integration as a standalone pip-installable package. |

## How to read it

Start with [`translate_hermes.py`](translate_hermes.py) and [`example_hermes_events.jsonl`](example_hermes_events.jsonl). Together they show the *contract*: the format the Hermes plugin will write, and the CloudEvent shape OpenStory expects to receive after translation. Run the test to see them in action:

```bash
cd docs/research/hermes-integration
python test_translate.py
```

Then read [`plugin_sketch.py`](plugin_sketch.py) to see how the Hermes plugin produces those input lines from inside a running Hermes session, and [`recall_tool_sketch.py`](recall_tool_sketch.py) to see how a Hermes agent can query OpenStory for structural views of its own past work.

[`DISTRIBUTION_PLAN.md`](DISTRIBUTION_PLAN.md) describes the path to shipping. Read it last — it presumes you've seen the rest.

## What's NOT here

- **A real OpenStory translator in Rust.** The `translate_hermes.py` sketch is the executable spec; the Rust port is a follow-up task tracked in [`../../BACKLOG.md`](../../BACKLOG.md).
- **A working PyPI package.** `pyproject.toml.example` shows the right shape but is not yet a real package; building one is the work tracked in [`DISTRIBUTION_PLAN.md`](DISTRIBUTION_PLAN.md).
- **Verified Hermes message shape.** This is the load-bearing prerequisite. The `# VERIFY:` markers in the Python files mark every assumption. See "Verification gap" below.

## Verification gap (the load-bearing prerequisite)

The translator and plugin are written against an *idealized* Hermes message shape. The actual shape needs to be confirmed against a finalized Hermes session log before the integration can ship. Specifically:

- **Assistant tool calls.** Hermes is provider-polymorphic. Does an assistant message use OpenAI's `tool_calls: [{id, function: {name, arguments}}]` shape (when the active provider is OpenRouter, Nous Portal, etc.) or Anthropic's `content: [{type: "tool_use", id, name, input}]` blocks (when the active provider is Anthropic direct)? It may be both, depending on which provider produced the turn.
- **Tool result messages.** What keys does a `role: "tool"` message carry? `tool_call_id`? `tool_name`? `name`? Both?
- **Reasoning blocks.** Where does Anthropic-style thinking show up — as a separate `reasoning` field on assistant messages, or as `content` blocks of type `thinking`?
- **System messages.** Hermes injects various synthetic system messages (compression summaries, todo snapshots). How are these tagged in the message dict?

Resolving these is a small task: boot a Hermes session, run a few turns including a tool call, finalize, and read `~/.hermes/logs/session_{id}.json`. Each `# VERIFY:` marker in the Python files names exactly which line in the resulting log file would resolve it.

**Recommended next session:** spin up a containerized Hermes (Max has offered the option), run a 5-turn task that exercises a tool call and a thinking block, capture the finalized log, and update the translator + plugin to match. After that the prototype becomes runnable end-to-end against a real Hermes process.

## What success looks like

End-to-end, in order:

1. User installs the standalone package: `pip install hermes-openstory`
2. Hermes auto-discovers it via the `hermes_agent.plugins` entry-point group on next startup
3. Plugin's `register(ctx)` runs, registers the `recall` tool and the lifecycle hooks
4. User starts a normal Hermes session — anywhere (`hermes`, Telegram, Discord, etc.)
5. As the session runs, the plugin's `post_llm_call` and `post_tool_call` hooks fire and append Hermes-native events to `~/.hermes/openstory-events/{session_id}.jsonl`
6. OpenStory's file watcher (already running locally) detects the new file, recognizes the Hermes path pattern, and routes the lines through `translate_hermes.rs`
7. CloudEvents flow through OpenStory's existing pipeline (persist, patterns, projections, broadcast) just like Claude Code or pi-mono events would
8. The Hermes session appears live in the OpenStory dashboard with sentence diagrams, eval/apply phases, domain facts, etc.
9. The Hermes agent can call its new `recall` tool mid-session — e.g., *"check what I did with this file in past sessions"* — and receive a structured answer from OpenStory's API
10. **The agent reads the algebra of its own coalgebra.** No feedback loop into the source. The bialgebra stays honest.

That's the v1 success case. The five integrations described in the brief — translator+sink, recall tool, skill signals, training data refinery, cross-provider comparison — are the five things that become possible once steps 1–10 are working.
