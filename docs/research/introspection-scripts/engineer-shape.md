# Engineer-shaped — how Engineer A and Engineer B ship, side by side

A comparison built from the OpenStory event store and `gh pr list`. The narrative is the 10% layer; the tables that follow are the 90% — every number reproducible by re-running `scripts/engineer_shape.py`.

This is not a ranking. It's a *shape* report — two engineers producing the same codebase via different rhythms, both productive, both legible from the data.

---

## The shape in one paragraph

Engineer A and Engineer B are working on the same project but in different *phases* of their relationship to it: Engineer A has 6 weeks in, has authored every architectural skeleton, and ships at a 27-PR / 155k-line cadence with a heavy tail of small ops PRs. Engineer B has 13 days in, has shipped 12 PRs / 12k lines focused tightly on the identity-and-users feature stack, and her sessions read more conversational than action-oriented in our classifier. The codebase reflects both — the bones are Engineer A's, the user-facing surfaces of the last two weeks are mostly Engineer B's, and the `/team-day` skill is the strongest team-shared signal in the entire corpus (invoked 3 times by each).

---

## At a glance

| | `engineer-a` | `engineer-b` (gh: `engineer-b`) |
|---|---|---|
| First PR | 2026-03-22 | 2026-04-26 |
| Most recent PR | 2026-05-08 | 2026-05-03 |
| Merged PRs | **27** | **12** |
| Open PRs | 6 | 3 |
| Lines added (across PRs) | 155,464 | 11,845 |
| Lines deleted (across PRs) | 16,817 | 265 |
| Sessions in store | 270 | 60 |
| Classifiable sessions (have `turn.sentence`) | 89 | 17 |
| Output tokens (cumulative) | 3,776,035 | 732,972 |

A few read-points:

- **Time-on-project differs by 5 weeks.** Engineer A started 2026-03-22, Engineer B 2026-04-26. So per-day shipping rate is more comparable than the totals suggest: ~0.6 PRs/day for Engineer A over 48 days, ~0.9 PRs/day for Engineer B over 13 days.
- **Engineer B's add/delete ratio is 45×.** 11,845 / 265. She is shipping almost-pure new code. Engineer A's ratio is 9× — he deletes too, partly because he wrote the code being deleted.
- **Classifiable session coverage is uneven.** 33% of Engineer A's sessions have `turn.sentence` patterns (89/270); 28% of Engineer B's (17/60). Comparable, but the absolute sample sizes differ — keep that in mind when reading shape distributions.

---

## How they ship — archetype distribution

| archetype | `engineer-a` | `engineer-b` |
|---|---|---|
| mixed | 46 (52%) | 8 (47%) |
| socratic | 9 (10%) | 5 (29%) |
| directive | 18 (20%) | 2 (12%) |
| recovery | 8 (9%) | 2 (12%) |
| **plumbing** | 8 (9%) | **0 (0%)** |

Two genuine signals:

1. **Engineer B's Socratic share is nearly 3× Engineer A's** (29% vs 10%). She's running more conversation-shaped sessions — not surprising for someone in a new codebase asking *why is it like this?* alongside *what should I add?*. Many of her early sessions invoke `/team-day` and `/office-hours` patterns of inquiry rather than shipping bursts.
2. **Engineer B has no plumbing sessions.** Plumbing in our taxonomy is the silent refactor — high write, zero explain, no commits. Engineer A has 8 (9%). That tracks: silent refactors are usually about reshaping load-bearing internal code, which Engineer A has more standing to do because he wrote most of it.

The shared 50/50 finding: both engineers' modal session is **mixed** — pivots from explore to ship mid-flow. That's the most common shape in the entire corpus (49% of all classifiable sessions). Whatever else is true about the two engineers, they share *the* dominant working pattern in this codebase.

---

## What they do in sessions — verb mix (top 9)

| verb | `engineer-a` | `engineer-b` |
|---|---|---|
| checked | 248 | 77 |
| wrote | 198 | 16 |
| explained | 177 | 61 |
| **committed** | **138** | **5** |
| edited | 121 | 31 |
| ran tests | 49 | 20 |
| created | 48 | 1 |
| delegated | 43 | 6 |
| read | 22 | 6 |

The eye-catcher: **`committed` × 138 vs × 5**. That's a 28× gap. It almost certainly does *not* mean Engineer B commits less — she has 12 merged PRs of work landed. What it means is that **Engineer B's shipping happens outside the 17 sessions our `turn.sentence` detector classified.** Her stacked-PR landing pattern (#47/#49/#50/#51 → #53) was one tightly-coupled multi-PR session that the classifier didn't generate sentences for, possibly because the turns were too short or the work was too recent for the patterns consumer to have caught up.

Two takeaways:

- **The classifier is sampling unevenly between the two engineers.** Caveat applies to every per-session shape comparison in this report.
- **The 90/10 principle saved this from being a wrong claim.** Without the verb table making the 28× gap visible and the *classifiable session* count visible right next to it, the synthesis would have been "Engineer B ships less" — which is false. The deterministic floor catches the synthesis when it gets cute.

The other striking gap is `created × 48` vs `× 1` — Engineer A creates new files often (greenfield is his mode); Engineer B edits existing ones. That fits with what's actually in the codebase: Engineer A wrote the architectural skeletons in Phase 2 (PRs #13, #21, #28, #29) which are heavy on new files; Engineer B has mostly added to existing UI components and built her additions inside established patterns.

---

## Where they ship — top projects

### `engineer-a` (270 sessions across 18 projects)

| project | sessions |
|---|---|
| OpenStory | 191 |
| claurst | 24 |
| arc-listener | 17 |
| gstack | 5 |
| openstory-ui-prototype | 5 |
| restaurant-metrics | 5 |
| mempalace | 4 |
| yc-app | 3 |
| hermes-agent | 3 |
| _12 more, ≤2 sessions each_ | _~12_ |

### `engineer-b` (60 sessions across 7 projects)

| project | sessions |
|---|---|
| openstory-ui-prototype | **25** |
| raptor-agentic-team | 15 |
| OpenStory | 11 |
| dora-metrics | 4 |
| telegram-int-local | 2 |
| ycombinator-app | 2 |
| workspace | 1 |

Two real findings here:

1. **Engineer B's heaviest project isn't OpenStory — it's `openstory-ui-prototype` (25 sessions).** She's been prototyping UI ideas in a separate workspace and bringing the validated patterns back into OpenStory proper (11 sessions). That's a *prototype-then-port* workflow visible in the data, not a workflow she'd ever describe verbally — but the session distribution makes it legible.
2. **Engineer A spans 18 projects on the same machine.** This is the data behind the "personal-vs-work principal split" use case in earlier reports. `claurst`, `mempalace`, `yc-app`, `restaurant-metrics`, `little-learner` are on the same host as OpenStory but represent different scopes. The Fleet identity work in PR #54 will let those split cleanly.

---

## Skills they invoke

| skill | `engineer-a` | `engineer-b` |
|---|---|---|
| /team-day | **3** | **3** |
| /sessionstory | 4 | 0 |
| /office-hours | 2 | 0 |
| /frontend-design | 1 | 0 |
| /plan-ceo-review | 1 | 0 |

`/team-day` is the only co-used skill in the entire corpus, and it lands at exactly **3 invocations from each engineer**. That's the strongest team-shared-tool signal we have. Both engineers use it for the same purpose: produce the daily facts file, then write a stand-up or PR description from it.

Engineer A is the introspection-skill power user — he's the one running `/sessionstory` (4×), `/office-hours` (2×), `/plan-ceo-review` (1×), `/frontend-design` (1×). These are mostly "thinking-about-thinking" skills he uses solo. Engineer B hasn't touched them yet, which could mean (a) she hasn't discovered them, (b) she doesn't need them yet for her current work, or (c) she has different workflow preferences. The observation alone tells you where to look — no claim from the data about which is true.

---

## How they introspect OpenStory itself (tier events)

| tier | `engineer-a` | `engineer-b` |
|---|---|---|
| rawdog (grep .jsonl) | 58 | 7 |
| **rest (`/api/*`)** | **620** | **99** |
| script (`scripts/sessionstory.py` etc.) | 91 | 1 |
| skill (`/sessionstory`, `/team-day`) | 7 | 3 |
| mcp (`mcp__openstory__*`) | 291 | 38 |

Both engineers introspect OpenStory; both use **REST + MCP** as the dominant tiers. Engineer A uses scripts heavily (91 vs 1) — he wrote most of them, so they're his native interface. Engineer B hits the API directly via curl and the MCP server.

The rawdog-vs-skill ratio is also telling. Both engineers occasionally fall back to grep'ing JSONL transcripts directly (Engineer A 58, Engineer B 7) despite CLAUDE.md flagging that as anti-pattern. The honest read: when an answer is needed *right now* and the script wouldn't reach it, the shortest path wins. The path forward is to make the scripts cover more questions, not to scold the rawdog.

---

## Rhythm — sessions per day in the last two weeks

| day | `engineer-a` | `engineer-b` |
|---|---|---|
| 2026-04-19 | 16 | 0 |
| 2026-04-26 | 15 | 0 |
| 2026-04-29 | 7 | 0 |
| 2026-04-30 | 8 | 0 |
| 2026-05-01 | 3 | **22** |
| 2026-05-02 | 6 | 13 |
| 2026-05-03 | 14 | **22** |
| 2026-05-07 | 5 | 0 |
| 2026-05-08 | 8 | 0 |

The rhythm tells the team-formation story directly:

- **April 19** — Engineer A's biggest day: 16 sessions. This is when PR #29 (NATS Actor 4 decomposition, 40k LOC) shipped.
- **April 26** — Engineer A's second-biggest: 15 sessions. The engineer-b workspace appears in the corpus for the first time the same week.
- **April 30** — Engineer A 8 sessions; Engineer B hasn't yet hit her stride.
- **May 1–3** — Engineer B's three peak days (22, 13, 22). The identity stack landing.
- **May 7–8** — Engineer A back, Engineer B quiet. The PersonId+Fleet PR work (this conversation).

The two engineers' peak days don't overlap. They've been productively async — handing off feature surfaces (Engineer B's Users tab → Engineer A's PersonId built on it) without stepping on each other's commits.

---

## What the data does *not* say

A discipline of the 90/10 principle is naming the limits. Things this report intentionally does *not* claim:

- **Productivity.** Lines and PRs are not productivity; they're surface area. A 30-LOC docs PR (#15: "Story tab hero in README") shifted the project's narrative more than some 5,000-LOC refactors.
- **Quality.** This is a shape report, not a code-review report. The introspection scripts can be extended toward quality (test cycle counts, error recovery patterns, post-merge fixup PRs) but that's its own analysis.
- **Skill.** Engineer B has 13 days of data; that's not enough to characterize anything other than her ramp. Engineer A has 48 days. Compare *patterns*, not *talents*.
- **Causation.** Engineer B has 0% plumbing because she hasn't been here long enough to refactor what she wrote. Not because she avoids refactoring.

---

*Generated by `scripts/engineer_shape.py`. Re-run anytime; the data refreshes against current sessions and `gh pr list`.*
