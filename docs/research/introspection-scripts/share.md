# Five OpenStory reports — one per question

Each report below answers one of the questions an early adopter asked. The format is reader-first: **who's asking and why → what you see in your data → how it works → a concrete example with the arc of what happened → what you do with this.** Every number and every quote is reproducible by running the scripts in `scripts/`.

The deterministic floor is the data; the narrative arc in each example is the thin model-supplied layer on top — the 90/10 split, made visible.

---

## 1. "Why are these changes needed in this PR?"

**Who asks this, and why.** A tech lead reviewing a teammate's PR. An onboarding engineer trying to understand existing work. A manager auditing scope on a multi-day feature. They have a diff in front of them and the commit subjects aren't enough — they want to know *what was being figured out* and *why this approach won*. Today this lives in Slack scrollback or "ask the author" — out of band, lossy, and gone the moment people switch contexts.

**What you see in your data.** For every PR-shipping session, the full prompt trail: opening question, each pivot, the plan(s) the agent wrote, the commits that landed, in time order. The PR body becomes self-explaining instead of "trust me on context."

**How it works.** Four signals already in the event stream — none of them new collection:

1. `gh pr create` tool_calls (the deterministic anchor — it's a `Bash` command matching `gh\s+pr\s+create`).
2. `turn.sentence` patterns — one per turn, with verb + human prompt + agent eval.
3. Writes to `**/plans/**` (the spec the PR fulfilled).
4. `git commit -m "..."` regex matches, including the HEREDOC form.

Joined to the session list for `user / project / branch`.

**Concrete example — a research-driven feature PR.**

```
Title:    feat: PersonId + Fleet identity foundation
Duration: 26.4h     Sentences: 23     Plans: 1     Commits: 12
```

The arc: the engineer opens at 00:31 with a small question — *"help me understand what would be required for permissions based on user."* The agent answers concretely from an existing NATS spike, but the engineer leans in instead: *"is an agent a person?"*. That question opens a four-hour philosophical exploration through agency, identity, AD/Keycloak compatibility, and how to keep the model pluggable. The engineer asks for a diagram; the agent produces an HTML visualization. By 1:47 AM a testcontainer spike is running to validate the conceptual model. Sleep.

Comes back the next afternoon, validates the spike (*"so, what did we learn?"*), and at hour 22 asks the pivot: ***"PersonID would be great. Can you plan it out?"*** A single plan file is written. The engineer green-lights: *"both 1 and 2 in the same PR. Go ahead and push :)"*. Over the next 4 hours, 12 commits land in dependency order — docs → tests → core → server → store → API → UI. A vite proxy bug surfaces near the end and is debugged live. The PR ships 26 hours after the first prompt, with its full philosophical justification attached.

**What you do with this.** Review the PR with the design decisions visible, not buried. The 4 hours of *"is an agent a person?"* is the actual work — challenging it intelligently is now possible. Onboarding engineers reading the trail learn how the team thinks, not just what shipped.

---

## 2. "How did the engineer come up with this answer? What was the trail of prompting?"

**Who asks this, and why.** A senior engineer trying to mentor a peer without standing over their shoulder. A tech lead calibrating team patterns ("are we exploring enough? shipping enough?"). An engineer learning a new area of the codebase by watching how someone else figured it out. They want process visibility — not as surveillance, but as a way to share thinking.

**What you see in your data.** Every session reduces to a verb-classified timeline (explained / wrote / edited / checked / committed / ran) and lands in one of five archetypes:

- **Socratic** — research-driven, conversation-heavy
- **Directive** — do-X-then-ship, low exploration
- **Recovery** — debug-shaped, high check + interleaved explain
- **Plumbing** — silent refactor, write-heavy with no ship
- **Mixed** — pivots mid-session, the most common shape

**How it works.** `scripts/prompt_trail.py` reads `turn.sentence` patterns, tallies a verb histogram, and applies a small ordered rule set:

```
explain ≥ 0.45                    → Socratic
write+ship ≥ 0.55, explain < 0.20 → directive (if ship ≥ 0.10) else plumbing
check ≥ 0.30, explain ≥ 0.20      → recovery
write ≥ 0.40, explain ≤ 0.30      → directive
otherwise                         → mixed
```

Not ML, ten lines. Anyone on the team can read it, argue with it, or tune it.

**Concrete example — two sessions, two shapes.**

A 10-sentence Socratic session: 60% explained, 20% wrote, 20% checked. The engineer wasn't shipping — they were thinking. They invoked `/office-hours` and `/plan-ceo-review` on the same conversation. The arc was a long brainstorm before any code was written, the kind of session that produces a design doc and a clearer head, not a commit. Compare to a 12-sentence directive hotfix: 0% explained, 8 commits, 1 test run, 17 commits across 4 PRs by the end of the day — pure execution, the engineer knew exactly what to do.

Both were productive sessions. They had different shapes because they were doing different work.

The interesting case is in between: a 23-sentence session that came in at 43% explain — *just under* the 45% Socratic threshold, so it landed as "mixed." Re-reading the arc, that's correct: it started Socratic and pivoted to ship at hour 22. Forcing it to "Socratic" would have been a label-clinging classifier mistake. "Mixed" honestly captures the pivot.

**What you do with this.** Coach engineers on shape, not output. "Your last three PRs were directive — try a Socratic session before the next one" is a real conversation you can have when the data is visible. Or: "this kind of feature usually needs more recovery; want to pair?". The team patterns become measurable without anyone naming names.

---

## 3. "Why did the coding agent create this state?"

**Who asks this, and why.** A reviewer hitting an unfamiliar change in a diff and wondering "where did this come from?". An engineer doing forensics on a chunk of code that nobody seems to remember writing. A trust auditor asking whether a sensitive file was changed for a documented reason. They want a clear answer to *why these bytes exist*, traceable back to a human prompt.

**What you see in your data.** Pick any file path. Get every session, every turn, every prompt that wrote it — sorted by time, attributed to user and project. Reverse-provenance from bytes to intent.

**How it works.** `scripts/state_provenance.py`:

1. FTS-search for the file basename.
2. For each candidate session, find every `Write`/`Edit`/`MultiEdit` with that exact `file_path`.
3. For each write timestamp, find the latest `turn.sentence` whose `started_at ≤ ts` — that turn owns the write.
4. Return `(session, user, project, turns, prompts, write_count)`.

**Concrete example — a hot file vs a cold one.**

The hot file is `ui/src/components/Sidebar.tsx`: 18 writes across 3 sessions, all by the same engineer but in three completely different intents. Session A wrote 7 chunks across two turns, owned by *"your instinct on making sure our test suite tests the correct thing is a good one :)…"* — a test-focused refactor. Session B wrote 3 more, owned by an agent self-edit (system-reminder). Session C, the PersonId+Fleet PR, wrote 8 more, all owned by *"just build the ui work on this same pr"* — a one-shot UI implementation. Three intents, three engineers' moods, three reasons. The file's history isn't a single story — it's a fan.

The cold file is `rs/server/src/principal_resolver.rs`. 4 writes, 1 session, 1 turn, owned by ***"PersonID would be great. Can you plan it out?"***. Born once, never touched again. A path, not a fan.

A third example that hints at what's possible: `docs/BACKLOG.md` shows 14 writes across 5 sessions, with prompts like *"Add to backlog :) I need to go to bed. what an amazing day"* — the file is functioning as a cross-session journal of "future work I want to remember." That's a *real fact* about how the team uses the doc, recoverable from the data without anyone documenting the convention.

**What you do with this.** Code review with "why" attached to "what." If you're reviewing a PR that touches `Sidebar.tsx`, knowing that 8 of the 18 writes happened in *this* session, all rooted in *"just build the ui work on this same pr"*, tells you what's in scope. The other 10 writes were different intents. Reviewer doesn't have to guess. Same shape solves "I don't remember why this code exists" — pull up provenance, read the prompts, you remember.

---

## 4. "What knowledgebase did the agent use? What skills?"

**Who asks this, and why.** A tech lead deciding whether to invest in more skills, more docs, more specialized prompts. A manager auditing whether the knowledgebase the team built is actually being used. An engineer wanting to discover what skills exist by seeing what their teammates run. The question is investment-shaped: "what's load-bearing? what's dead weight?".

**What you see in your data.** Three orthogonal signals per session, three different intents:

- **invokes** — slash-commands run during the session (e.g. `/team-day`, `/sessionstory`)
- **skill reads** — the agent opened a `SKILL.md` mid-flow without invoking it
- **knowledge** — `Read` of any `*.md` outside `node_modules` / `target` (`docs/`, `README.md`, `CLAUDE.md`)

**How it works.** `scripts/skills_used.py` runs three regex/path detectors:

```
INVOKE   user_message contains "Base directory for this skill: …/skills/<name>"
READ     tool_call Read whose file_path matches **/SKILL.md
KNOW     tool_call Read of *.md, excluding node_modules/target/dist/build
```

No new event types. The signal is already there.

**Concrete example — a skill that fed itself.**

In one session, an engineer invoked `/team-day` once. The skill is a two-phase script that produces facts files for a given day. After the invoke, the same session shows two `Read` calls on the artifacts the skill just emitted: `captures/team_day/2026-05-02/facts.md` and `captures/team_day/2026-05-03/facts.md`. The engineer then used those facts to write a stack-landing PR description.

So the chain is visible end-to-end: **skill invoke → artifact emitted → artifact consulted → PR shipped.** The skill was load-bearing for that PR, not decorative.

The team-level finding is more interesting: across the whole corpus, `/team-day` shows 6 invocations split 3/3 between two engineers. Most other skills (`/office-hours`, `/plan-ceo-review`, `/sessionstory`) have a single invoker. `/team-day` is the only skill with strong evidence of co-use — that's a real signal about which investments paid off and which sit alone.

**What you do with this.** Decide what to invest in based on what's actually used, not what was hyped. Promote skills with evidence of co-use; deprecate skills that no one's invoked in a month. Audit the `docs/` folder by looking at which markdown files actually show up in knowledge-consultation events — if `ARCHITECTURE.md` is read 30 times and `OLD_DESIGN.md` is read zero times, the data is telling you something. Skill discovery for new hires gets easier too: the dashboard shows what their teammates are actually invoking.

---

## 5. "How is the team consuming this? And personal vs work for the same engineer?"

**Who asks this, and why.** An engineering manager watching the agent-subscription bill climb. A finance partner who wants to attribute spend to projects. An engineer who runs the agent on three machines for both day-job and side projects, and doesn't want their `mempalace` weekend hacking to look like billable work.

**What you see in your data.** Every session in `/api/sessions` already carries `host`, `user`, `project_name`, `principal_id`, plus per-session `total_input_tokens` and `total_output_tokens`. The "fleet" view groups by principal — one human, one or more identities (work / personal / second machine), one filter. Personal-vs-work for the same engineer is a `watch_dir_pattern` on a principal: same person, two principals, automatic split by which directory the session ran in.

**How it works.** OpenStory's PR #54 (the same session profiled in report 1) added `person_id` and `principal_id` to every newly ingested CloudEvent, plus a resolver that maps `(host, user, watch_dir)` → principal at ingest time. `scripts/openstory_tier_usage.py` cross-cuts sessions by user × project × tier against this attribution.

**Concrete example — the live store, today.**

```
488 sessions in store
Principal coverage: 270/488 = 55%   (218 unattributed, mostly pre-resolver)

Per-(host, user) breakdown:
  Maxs-Air         · engineer-a    270 sess   3.6M output tokens   ✓ enrolled
  Engineer Bs-Mac-mini  · engineer-b          60 sess   732k output tokens   ✗ not enrolled
  Maxs-MacBook-Air · unknown        12 sess    28k output tokens   ✗ second rig
  unknown          · unknown       143 sess   753k output tokens   ✗ pre-resolver

Per-engineer project mix:
  Engineer A:  18 distinct projects on the same host/user
               OpenStory 73%, claurst 7%, mempalace 5%, hermes-agent, arc-listener,
               gstack, resume, little-learner, …
  Engineer B:   7 distinct projects
               OpenStory 52%, openstory-ui-prototype 29%, raptor-agentic-team 10%, …
```

The arc is the *coverage gap*, not a single session. Today, half the corpus is attributed; the other half is sitting in an "unknown" bucket because those sessions ran before the resolver shipped. New sessions auto-tag from now on, so the number trends up by itself. Engineer A spans 18 projects on one host/user — clear evidence that the personal/work split is needed *because the data shows it happening already*. The fix isn't building a feature; it's enrolling two more principals and adding a `watch_dir_pattern` to the existing one. Same model, two new TOML entries, instant split.

**What you do with this.** Spend visibility by principal, not by raw user. The fleet sidebar already exists; once enrollment catches up, every existing dashboard (token usage, project mix, skill use) becomes principal-filterable in one stroke. The same model handles "I run this on three machines" without invasive instrumentation. And for finance: tokens-by-principal-by-project is one query, attributable to the right cost center automatically.

---

*All five reports are reproducible from a clean checkout. Methodology in [`README.md`](./README.md), live data appendix in [`cases.md`](./cases.md), generator at `scripts/build_cases_report.py`.*
