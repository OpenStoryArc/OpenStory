# OpenStory's first six weeks — a timeline

The project's history, told phase by phase. Every quoted prompt is verbatim from the OpenStory event store; every PR number resolves to the matching session via the cross-reference in [`pr-retrospective.md`](./pr-retrospective.md). The deterministic skeleton is the data; the narrative weave is the addition.

The arc, in one paragraph:

> OpenStory shipped CI and security in week 1, became reflective with the five-layer pipeline and Story tab in week 2, scaled to multi-agent and multi-machine in week 3, hardened production in week 4, and became a team in weeks 5–6. The PR shapes themselves tell the story: the giants (PR #13 at 24k LOC, PR #21 at 34k, PR #29 at 40k) appear when architectural skeletons need to ship; the small ones (#1 at 8 LOC, #15 at 30 LOC) appear at the edges, in maintenance and docs. The two engineers ship differently — Engineer A in long Socratic arcs, Engineer B in tight directive bursts — and both shapes produce the codebase.

---

## Phase 1 — Bootstrap  (March 22 → 31  ·  9 PRs  ·  ~5,500 LOC net)

OpenStory's first week was paved before any features shipped. **PR #1** fixed `just up` for cross-platform compatibility; **PR #2** set up master-branch protection. **PR #3** closed 14 dependency security vulnerabilities. **PRs #4–#5** wrote the docs and templates the project would lean on for everything after. Five small PRs in three days, none about OpenStory's mission — but every one of them about *being able to ship at all*.

Then the first big swing. **PR #6** (March 25, +3,123 LOC) shipped pi-mono integration: the CloudEvent translator gained a discriminator, the watcher learned a second directory, and `agent: "pi-mono"` started flowing through the pipeline. OpenStory went from single-agent to format-agnostic in one PR.

**PRs #7–#8** (March 25–29) shipped production deployment — the OpenClaw + Telegram + Hetzner VPS stack went live in this window. The deployment was both technical and conceptual: OpenStory wasn't a local tool anymore; it was a service watching agents on a remote machine. The session that produced PR #8 opened with *"Let's deploy this"* and ended with the OpenClaw service responding to Telegram pokes from across the internet.

**PR #9** (March 31, +907 / -1,380) closed out the bootstrap by ripping out Qdrant entirely and replacing it with SQLite FTS5. The session opened with the deletion question framed plainly. The simpler tool won.

By March 31, OpenStory had: a real CI gate, two coding agents observable through the same pipeline, a deployed production stack, and a search engine that ran in-process. Bootstrap was done.

---

## Phase 2a — First architecture wave  (April 6 → 9  ·  6 PRs  ·  ~40,000 LOC added)

This is the week OpenStory became reflective.

**PR #13** (April 6, +23,921 LOC) shipped the **five-layer pipeline + Story tab + eval-apply architecture**. The shape now load-bearing on every other PR — `CloudEvent → ViewRecord → WireRecord → Pattern → Sentence` — was sketched, implemented, and put behind a UI all in one PR. The Story tab put the agent's work in front of the user as it happened, with eval-apply pairs, sentence summaries, and live updates.

**PR #14** (April 7, +6,420) extended Story tab data surfacing and pushed NATS deeper into the architecture. **PR #15** was 30 lines of pure documentation: it put Story tab in the README hero. Tiny, but it marked the moment Story became the headline feature instead of an experimental tab.

**PR #17** (April 9, +8,391) introduced **MongoDB as a pluggable EventStore backend**. SQLite stayed default, but the project picked up the Mongo path here — and the conformance suite that runs the same 47 BDD helpers against both backends started enforcing parity. The decision to keep SQLite as default was deliberate: portable, in-process, no extra deps.

**PR #18** was bookkeeping: `token_usage` learned the new Opus 4.6 tiers and cache savings. The session opened with *"can you tell me my total spend for all my sessions?"* — a real cost-curiosity question that turned into a model-tier-aware analysis script that the team has been using ever since.

**PR #19** (April 9, +725) shipped the first **MCP server prototype**. Agents could now query OpenStory about their own sessions through the Model Context Protocol — what later became the `mcp__openstory__*` tools we used throughout this very conversation.

By April 9, OpenStory had its architectural skeleton: events flow through a five-layer pipeline, persistence is pluggable across two backends, and a UI shows the work back to the user. Reflectivity shipped here.

---

## Phase 2b — Pi-mono + audit + actor refactor  (April 10 → 19  ·  6 PRs  ·  ~100,000 LOC)

This was the hard stretch. Six PRs, three of them giants.

**PR #20** (April 10, +3,722) added Sessions API pagination + StoryView rewrite + cost analysis scripts. The pagination piece mattered for performance, but the cost analysis scripts is where `scripts/cost_report.py` and the broader introspection toolkit started to mature — the same toolkit this conversation has been building reports on top of.

**PR #21** (April 19, +34,388) shipped **pi-mono integration + architecture audit + schema registry**. The audit and registry mattered as much as the pi-mono work itself: this is where OpenStory started drift-testing itself, codifying its architectural rules into assertions that couldn't be violated silently. **PR #28** (April 17, +17,243) doubled down with **test-driven architecture audit + principle tests** — the rules became laws.

**PR #22** (April 11) added **distributed NATS streaming via Tailscale leaf nodes**. This is the moment the OpenStory observe-graph could span machines. The Hetzner VPS running OpenClaw and the dev laptop running Claude Code became *one event stream*. **PR #24** (April 12) opened up MCP access for OpenClaw to query OpenStory directly — cross-machine introspection.

Then the giant. **PR #29** (April 19, +40,094 LOC) — *"NATS required + Actor 4 decomposition (Phase 0 safety net + Phase 1 refactor)."* The largest single PR ever merged in this repo. The session that produced it walked through 42 sentences over 16 wall-clock days. The opening prompt was *"Can you resolve any conflicts between research/hermes-integration and master?"* — the merge collision was what kicked off the refactor in the first place. The midpoint prompts capture the engineer's honest confusion: *"What does it look like to completely migrate over? Why is it in the middle?"* and later *"I'm confused about where we are"*. That texture isn't in the commit messages — it's in the trail.

The result: independent consumer actors for `persist`, `patterns`, `projections`, `broadcast`, all subscribing to NATS independently. The architectural skeleton from Phase 2a now had its joints rebuilt and tested.

---

## Phase 3 — Deploy + maintenance  (April 20 → 29  ·  2 PRs)

A breath. **PR #26** (April 20, +678) split the deploy compose into per-agent stacks — Bobby and Vera, named — so multiple agents could run on the same VPS without stepping on each other. **PR #32** fixed a missing crate in `Dockerfile.prod`. Quiet week, but the production posture for multi-agent deploys settled here.

---

## Phase 4 — Engineer B joins, identity stack  (April 30 → May 3  ·  16 PRs)

The team became a team.

**April 30 — Engineer B's first PR (#35)**: *"Add boot quickstart docs and prereq-check script."* 217 lines added, 0 deleted — a pure additive contribution that helped the next person onboarding. Conventional first move, careful and useful.

**May 1 — three fixes in 24 hours.** **PR #38** (Engineer B) fixed a UI cursor bug in session pagination — *"walk before_seq cursor + stream pages on session open."* **PR #36** (Engineer A, +2,412 / -1,853) lazy-loaded session records via REST + sidebar-only WS handshake — a complementary perf win the same day. The two engineers were already shipping in tandem.

**May 2 — the identity stack landed.** Six PRs in one day:

- **PR #42** (Engineer B, +635) — *"stamp user identity on every CloudEvent."* This is the foundation everything else built on. Every event from then on carried `user`.
- **PR #45** (Engineer B, +645) — Users tab v0.1, the first per-user activity surface.
- **PR #37** (Engineer A, +471) — sidebar filters + clickable prompts + first/last-event span fix.
- **PR #40** (Engineer B) — caught a silent fallback to JsonlStore. *"legacy DB silent fallback to JsonlStore + /api/health backlog"* — a real failure mode hardened.
- **PR #41** (Engineer B) — `OPEN_STORY_HOST` propagation through compose. Small, but a real deploy paper-cut.
- **PR #48** (Engineer A, +1,681) — boot-time JSONL → EventStore reconciler. The safety net that made the JSONL backup a real first-class story rather than an opaque artifact: on every boot, JSONL is reconciled into the SQLite store, so a database loss is recoverable.

**May 3 — the day finished hot.** Five Engineer B PRs landed in stacked formation:

- **PR #47** — PersonRow: primary Person filter on the Live tab
- **PR #49** — activity sparkline + colored project chips on user cards
- **PR #50** — Phase 2: URL-driven user filter, empty-state polish
- **PR #51** — time-window filter (Last Hour / Today / This Week / All)
- **PR #53** — chore: land stacked chain (#47/#49/#50/#51) onto master

In 48 hours the Users tab evolved from "v0.1 shows users" to "fully filterable, URL-shareable, time-windowed identity surface."

In parallel Engineer A shipped **PR #52** — the **team_day pipeline**: a deterministic per-author day report (+1,461 LOC, 0 deletions). The session that produced it ran 50 sentences over 43 wall-clock hours; the plan file `ticklish-rolling-kazoo.md` recorded the design. This is the skill that produces the `captures/team_day/.../facts.md` files that turn up in the skills_used introspection report — the `/team-day` slash command was born here.

By May 3 the project had: per-event user identity, a Users tab, time-window filtering, a JSONL reconciler, a deterministic per-author day report, and a real team rhythm. **Two engineers, sixteen PRs, four days.**

---

## Phase 5 — Open / in-flight  (May 8 today  ·  9 PRs)

The next chapter is identity, deepening.

**PR #54 — PersonId + Fleet identity** (Engineer A, +2,963, currently open) is the headliner. Built directly on Engineer B's `user`-stamping foundation from PR #42, it adds `person_id` and `principal_id` to every event, plus the resolver that maps `(host, user, watch_dir)` → principal. The session is profiled in detail in the introspection reports — 23 turns, 26 hours, 12 commits, opened with *"help me understand what would be required for permissions based on user"* and pivoted 22 hours in at *"PersonID would be great. Can you plan it out?"*. This is the actor of the very conversation that produced these reports.

**PR #46 — OTel collector + cross-stack dashboard** (Engineer B, +270) is the observability pivot. OpenStory becomes a first-class OpenTelemetry source — events flow out as well as in.

**PR #44 — insight extraction design** (Engineer B, +217 docs) is the next conceptual move: another consumer in the actor architecture that extracts higher-order insights from the event stream. Six backlog entries adapt DORA-pattern signals into the OpenStory model.

**PR #34 — Sprint 1 close-out** (Engineer B, +2,692) ties off CloudEvent typed migration artifacts and a clippy fix.

**PR #33 — retire Hermes plugin research path** (Engineer A, +50 / -3,620) is a cleanup: deleting research code that didn't pan out, in keeping with the project's principle that research garden material should either graduate or get composted.

PRs **#31, #30, #25, #23** are smaller in flight — CI fixes, dependabot triage, EventStore correctness, MCP server hardening.

---

## What the timeline tells you

A few patterns visible only at this altitude:

**The architecture got harder, then easier.** The big skeleton-rebuilding PRs cluster in Phase 2a and 2b (PR #13, #21, #28, #29). After Phase 2b, the largest PR is #36 at +2,412 LOC. The skeleton was paid for once.

**Pi-mono integration paid for itself twice.** It first shipped in Phase 1 (#6, March 25) as basic format-agnostic translation. Then it was *redone* in Phase 2b (#21, April 19, +34,388 LOC) when the architecture audit added the schema registry. The first version proved the idea; the second hardened it.

**The team became a team in 4 days.** April 30 is Engineer B's first commit. By May 3 she's shipped 12 PRs and a stacked chain. There's no "ramp-up" phase visible — the contribution rate from day one matches Engineer A's. One read: the bootstrap and architecture work in Phases 1–2 made the codebase legible enough that a competent engineer could ship features on day one.

**Identity is the throughline of Phase 4 → Phase 5.** Engineer B's `user`-stamping (#42) → Engineer A's `person_id`/`principal_id` (#54) is one continuous design arc across 8 days. The fleet view, the watch_dir_pattern split, the spend-by-principal aggregation — all of these are downstream of these two PRs.

**The introspection scripts (the ones this conversation produced) are themselves the next phase.** They use the data the prior phases built — `turn.sentence` patterns from #13, the actor-decomposed event stream from #29, the user identity from #42, the team-day skill from #52, the principal_id from #54. Reflexivity all the way down.

---

*Generated narrative on top of `pr-retrospective.md`. Update the retrospective (`python3 scripts/pr_retrospective.py`) and revisit this prose when phases lengthen or new shape emerges.*
