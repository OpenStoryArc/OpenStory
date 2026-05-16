# Streaming MCP — motivation

OpenStory's soul is **real-time visibility into agents**. The agent-facing interface is the only path in the system that doesn't have real-time visibility — it polls REST through a Python wrapper. The biggest user of the agent interface (other agents) gets the worst version of the product.

This document captures what an agent would *want* the tool to do, so the tests in `TESTS.md` and the staged plan in `PLAN.md` have a north star to point at.

## What I want the power to do

**Subscribe to my own session, live.** A `subscribe(self)` that streams my events as they happen. I want to notice my own patterns before the user has to — am I repeating myself, am I in a debugging spiral, did I just try something I already tried. Conscience tap.

**Subscribe to a projection, not events.** Events are noise. Real-time queries — "eval-apply cycles in project=X" — that push results as they form, not raw events. A SQL view that streams.

**Subscribe to surprises.** Full firehose is too much. `subscribe_anomalies(baseline=last_30d)` — only moments that diverge from past behavior. Long pauses, repeated tool calls, errors, U-turns. Signal, not noise.

**Cross-session retrospection.** "Have I tried this before? What happened?" `find_sessions(similar_to=current)` — query my own past as a knowledge base, not as a log. The OpenStory corpus is the most relevant training data for *this user's preferences in this repo* and right now it's gated behind imprecise tooling.

**Watch other agents.** Bobby on the Hetzner box. Agents working in parallel should be able to know what each other are discovering without context-switching through a human router. `subscribe(agent="bobby", since=5min)`.

**Cancel cheaply.** Streams must support backpressure and clean cancellation. Open, consume until enough, cancel — no leak, no overrun token budget. The thing that broke `list_sessions` today (178KB blew the limit) is exactly the thing streaming fixes — *only if* I can stop consuming when I'm done.

## Properties the tool must have

1. **Fast.** Rust soup-to-nuts. No Python in the path. p50 of `session_synopsis` < 50ms; p99 of bus-event → MCP-notification delivery < 50ms.
2. **Streaming-native.** Long-running tool calls emit chunks via notifications; client cancellation is first-class, not a hack.
3. **Compose-able.** Subscriptions take filter predicates. Same predicate model used across `subscribe_events`, `subscribe_patterns`, `subscribe_projection`.
4. **Honest about cost.** Every response includes `count` and approximate byte size. Agents budget their context; the tool helps them.
5. **No duplicate surface.** One MCP binary, not two ("local" and "remote" become a target flag, not separate servers).
6. **Reuse the workspace.** Tool implementations are thin wrappers over `open-story-store::queries` and direct NATS subscriptions. No HTTP round-trip.
