# Philosophy

## Mission

**Read your agent history.**

Your coding agents already write everything down. Open Story makes that history legible — live as work unfolds, and later when you need the story, the cost, or the exact command that fixed it. Humans and AI agents are co-creators; agents read, think, edit, test, and make decisions alongside you. The point isn't surveillance. It's understanding and owning the story of the work.

That ownership is **personal sovereignty**: you can observe, understand, and decide. The data is yours. Open formats (CloudEvents, JSONL, Markdown), portable, unencumbered. Own your data, own your story.

## Constraint

**Observe, never interfere.** This is a mirror, not a leash.

The system watches but never interferes. It never writes back to the agent, never modifies transcripts, never blocks execution. It does not become the agent runtime, does not inject memory or policy, and does not stand between you and the tools you already use. It translates what happens into a form you can see, search, and reason about.

This principle prevents scope creep. Features that would require mutating the source, inserting into the agent's execution path, or blocking agent behavior do not belong here. Session data is immutable — no CRUD on events. The dashboard is read-only. The value of observation comes from its purity: if the observer affects the observed, the observation is compromised.

## Own your data source

The interface has two fundamentally different relationships with the same history:

**Live** is a stream — an immutable, lazy sequence you listen to. Events arrive, you see them flow by. When you refresh, it starts fresh. This is push, real-time, ephemeral. The stream is the truth for "what is being written now."

**Explore** is an atom — persisted state that is constantly refreshed but always consistent and queryable. Sessions can be viewed, searched, interpreted, sliced by facet. This is pull, on-demand, authoritative. The atom is the truth for "what was written."

Each view owns exactly one data source. Merging them creates a view that's "sort of live and sort of complete but actually neither." Keep views honest about what they know and where their data comes from.

## Turns, not trees

Agent transcripts have a `parent_uuid` field on each event, which might suggest a tree structure. In practice, this creates a sequential chain — each event simply points to the previous one. A session with 300 events can have a chain 177 levels deep, with almost no branching.

The useful structures are:
- **Turns** — one human prompt, the agent's work in response, and the final answer. This matches how humans think about what happened.
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

If you're adding complexity, ask: does this help someone **read their agent history**, without compromising the observe-only constraint? If you can't articulate that sovereignty benefit, it doesn't belong here.
