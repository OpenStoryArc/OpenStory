# Philosophy

## Mission

**Read your agent history.**

Coding agents already write everything down. Open Story makes that history legible. The point isn't surveillance. It's understanding and owning the story of the work.

Who reads that history is not a design axis. Dashboard, API, MCP, scripts — same mission. The load-bearing line is not audience; it is **read, don't rewrite**.

That ownership is **personal sovereignty**: observe, understand, decide. The data is yours. Open formats (CloudEvents, JSONL, Markdown), portable, unencumbered. Own your data, own your story.

## Across the whens

The mission is not only archaeology. **Tense matters.** The same history can be read in every aspect of the work:

| When | What you read | Conjugation |
|------|----------------|-------------|
| **Now** | the session as it is writing | *is writing* — watch this session live |
| **So far** | the open session's trail | *has written* — reflect mid-flight without closing the work |
| **After** | finished sessions, fleet, search | *wrote* — story, cost, the command that fixed it |

Self-reflection is just the mission conjugated: a session may read **what it is doing**, **what it has done so far**, and **what it did**. The transcript already is that history; Open Story makes it readable while the work is still open, and after it is closed. It is not memory injection. Looking at history does not rewrite the actor.

**How a session reads itself:** the MCP (`open-story-mcp`) is the in-session surface for the same three tenses — `subscribe_session` / `subscribe_tokens` for *is writing*, session/query tools for *has written*, search and list tools for *wrote*. Dashboard and REST are the same history; MCP is how the work in flight holds the mirror without leaving the loop. Still read-only. Still not the agent runtime.

**Attention layer (agent-in-UI), not a second mission:** `ui_control` / `where_is_user` / `subscribe_ui_state` let a session *steer the mirror* — open a view, focus an event, present a finding, follow where attention already is. That is aligned with the mission when it **shows or navigates history**. It is not co-equal with reading history; chrome for chrome's sake fails mission fit. Partition: everything authored lands on `ui.*` only, never `events.*`. Doctrine: **the agent may steer the mirror; it may not rewrite history.**

## Constraint

**Observe, never interfere.** This is a mirror, not a leash.

The system watches but never interferes. It never writes back to the agent, never modifies transcripts, never blocks execution. It does not become the agent runtime, does not inject memory or policy, and does not stand between you and the tools you already use. It translates what happens into a form you can see, search, and reason about.

This principle prevents scope creep. Features that would require mutating the source, inserting into the agent's execution path, or blocking agent behavior do not belong here. The load-bearing rule is **don't change what the agent wrote** — not a blanket ban on store lifecycle. Append-only ingest, session delete, retention sweeps: those operate on *your* copy of history. They never rewrite the agent's transcripts. Views that display history are read-only over the observed source. The value of observation comes from its purity: if the observer affects the observed, the observation is compromised.

## Own your data source

The interface has two fundamentally different relationships with the same history:

**Live** is a stream — an immutable, lazy sequence you listen to. Events arrive, you see them flow by. When you refresh, it starts fresh. This is push, real-time, ephemeral. The stream is the truth for "what is being written now."

**Explore** is an atom — persisted state that is constantly refreshed but always consistent and queryable. Sessions can be viewed, searched, interpreted, sliced by facet. This is pull, on-demand, authoritative. The atom is the truth for "what was written."

Each view owns exactly one data source. Merging them creates a view that's "sort of live and sort of complete but actually neither." Keep views honest about what they know and where their data comes from.

## Turns, not trees

Agent transcripts have a `parent_uuid` field on each event, which might suggest a tree structure. In practice, this creates a sequential chain — each event simply points to the previous one. A session with 300 events can have a chain 177 levels deep, with almost no branching.

The useful structures are:
- **Turns** — one user prompt, the agent's work in response, and the final answer. This matches how sessions are actually experienced.
- **Inverted indexes** — "which events touched this file?" or "show me all bash commands." These let you slice the data any way you need.

We built a tree view, looked at real data, and deleted it. The data model should match the data, not our assumptions about it.

## Faceted navigation over hierarchy

A tree forces you to pick one hierarchy: by turn? by file? by tool? Faceted navigation lets you slice any way. Click a turn AND a file to see "what happened to `config.rs` in turn 3." The implementation is simple — inverted indexes built in one pass over the flat record array. No graph database. Pure functions. Instant.

## Prototype first

Every major feature starts as a script querying real data. Validate the data model before building UI. The prototype catches wrong assumptions before you invest in components.

This prevented us from building a tree UI for data that isn't a tree. It revealed that 89% of sessions are agent subagents (informing the sidebar hierarchy). It showed that payload truncation saves less than 1MB across all sessions (informing the decision to effectively disable it).

The prototype is the spec. If it works on real data, the production implementation has a clear target.

## Minimal, honest code

No abstractions without justification. Three clear lines beat a clever helper. Don't build for hypothetical futures. Solve the problem in front of you.

If you're adding complexity, ask: does this help **read agent history**, without compromising the observe-only constraint? If you can't articulate that sovereignty benefit, it doesn't belong here.
