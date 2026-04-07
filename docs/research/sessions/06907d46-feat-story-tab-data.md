# Session report — `06907d46` (feat/story-tab-data)

**This is OpenStory pointed at itself.**

On 2026-04-06, a single 21-hour Claude Code session built the Story Tab feature on the `feat/story-tab-data` branch. This document is the story of that session — generated *from* OpenStory's own data, *by* OpenStory's own scripts, while OpenStory was the thing being built. The whole project's soul is one sentence: *"enable personal sovereignty for humans working with AI agents."* What you're reading is what that looks like when the tool turns around and applies the principle to itself.

Every number, every quoted prompt, every detected pattern came from three REST endpoints (`/api/sessions`, `/api/sessions/{id}/records`, `/api/sessions/{id}/patterns`) and four Python scripts in `scripts/`. Nothing was hand-curated. Nothing was reconstructed from memory. The data is in CloudEvents 1.0, in JSON, in human-readable formats, in the user's own filesystem. The narration is a model reading the structured fact sheet and grouping it into sections — but every claim is grounded in a script you can run yourself in 10 seconds.

> **Reproduce on your own session:**
> ```sh
> SID=06907d46-b9fb-4907-88cf-2c72bca5d305    # or any session id from `curl localhost:3002/api/sessions`
> python3 scripts/sessionstory.py $SID --unfinished
> python3 scripts/analyze_eval_apply_shape.py --session $SID
> python3 scripts/analyze_turn_shapes.py $SID
> python3 scripts/analyze_event_groups.py --session $SID
> python3 scripts/token_usage.py --session-id $SID
> ```

---

## At a glance

| | |
|---|---|
| Session id | `06907d46-b9fb-4907-88cf-2c72bca5d305` |
| Branch | `feat/story-tab-data` |
| Started | 2026-04-06 02:12 UTC |
| Ended | 2026-04-06 23:13 UTC |
| Duration | **21.02 h** (with gaps — see "Gaps" below) |
| Records | **4001** |
| Model turn boundaries (`turn_end`) | **63** |
| User prompts | **155** |
| Sidechain records | **0** |
| Tool calls | **956** |
| Eval-apply cycles | **628** (541 with tools, 87 terminal) |
| Tools per cycle | min 1, max 7, **avg 1.8** |
| Estimated cost (Sonnet rates) | **$212.27** |

The session is *huge*: 21 hours of wall-clock time, 4001 records, 956 tool calls, ~$212 of API spend. It's also unusually clean — zero sidechain records, meaning no Task-tool sub-conversations. All the subagent work happened inline.

---

## Two definitions of "turn"

Worth flagging up front because the existing scripts disagree:

| Script | Count | What it means |
|---|---|---|
| `sessionstory.py` (this commit) | **63** | `system.turn.complete` events (true model turns) |
| `analyze_event_groups.py` | **155** | User-prompt windows (one per `user_message`) |

Both are correct — they answer different questions. A model turn often spans multiple user messages because the user can interject mid-stream without ending the model's turn. The `analyze_event_groups.py` "Turn 30: 379 events" is a per-prompt window, not a model turn. The 379-event monster is the longest stretch of *unbroken* per-prompt activity, not the longest model turn.

---

## Tool histogram

```
Bash             440  ████████████████████████████████████████████
Read             178  ██████████████████
Edit             174  █████████████████
Grep              88  █████████
Write             37  ████
ExitPlanMode      16  ██
Agent              9  █
EnterPlanMode      7  █
ToolSearch         3
Glob               2
AskUserQuestion    2
```

`Bash` dominates 2.5× the next tool. Combined with the `EnterPlanMode`/`ExitPlanMode` count (7+16) you can see the rhythm: **plan → execute → verify**, with a lot of test-running and git operations. `Read` ≈ `Edit` is healthy (each edit is read-justified, not blind). `Agent` only fired 9 times, which is the inline-subagent pattern we expected from the zero-sidechain count.

---

## Per-prompt window size distribution (`analyze_event_groups.py`)

```
1–4 events    : 64 windows  ████████████████████████████████
5–19 events   : 46 windows  ███████████████████████
20–49 events  : 22 windows  ███████████
50–99 events  : 12 windows  ██████
100–199 events:  9 windows  ████
200+ events   :  2 windows  █
```

Median window: **6 events**. Mean: **25.8**. The tail dominates — two 200+ outliers and seven 100+ windows account for over half the total event budget. This is a power-law: most prompts are small clarifications, a few are massive plan-then-execute sprints.

### The ten biggest prompt-windows

| # | Events | Tools (head) |
|---|---|---|
| Turn 30 | **379** | EnterPlanMode, Bash×N, Read, Write, ExitPlanMode, Bash+115 more |
| Turn 43 | 215 | ExitPlanMode, Bash, Edit, Grep, Read, Edit+59 more |
| Turn 5 | 176 | Agent, Bash×6, Read+44 more |
| Turn 128 | 175 | Bash×N |
| Turn 132 | 163 | (no tools — pure conversation/reasoning) |
| Turn 82 | 160 | Read, Edit, Bash+38 more |
| Turn 88 | 151 | Read, Edit×N+5 more |
| Turn 86 | 131 | Read, Edit×N, Bash×N |
| Turn 65 | 110 | Bash, Read, Edit, Write, Grep×N+27 more |
| Turn 31 | 107 | Bash, Grep, Read, Edit+25 more |

These are the **plan-then-execute spikes** — the moments where a single prompt ("plan first, then do next iteration") unlocked a large autonomous run. The rhythm of the session is set by these spikes interleaved with short clarifications.

---

## Turn shape distribution (`analyze_turn_shapes.py`)

Across 63 model turns, **62 distinct shapes** — almost no two turns are alike at the event-sequence level. But classification is sharp:

| Class | Count | % |
|---|---|---|
| `multi_eval_apply` | 60 | **95.2%** |
| `multi_user_prompt` | 36 | 57.1% |
| `with_thinking` | 15 | 23.8% |
| `parallel_tools` | 3 | 4.8% |
| `pure_text` | 2 | 3.2% |
| `single_eval_apply` | 1 | 1.6% |

**Reading:** 95% of model turns involved multiple eval-apply cycles (the model called tools, got results, called more tools). 57% had multiple user prompts inside one model turn — that's Max interjecting mid-stream. Only 2 turns were pure text (no tools at all). Only 1 turn was a clean single-cycle exchange. **This is a deeply interleaved working session, not a Q&A.**

---

## Eval-apply structure (`analyze_eval_apply_shape.py`)

```
Records: 4001  Prompts: 155  Evals: 628  Tools: 956  Results: 956
Turn boundaries: 63
Eval-apply cycles: 628
  with tools: 541   terminal (text-only): 87
  tools per cycle: min=1 max=7 avg=1.8
```

628 eval-apply cycles. **86% of cycles dispatched tools**, only 14% were terminal text-only. The 1.8 average tools-per-cycle (max 7) means parallel tool dispatch was rare but happened — those are the moments the model batched independent reads/greps.

The ratio that matters: **956 tools / 628 evals = 1.52** tools per model-thought. Each eval, on average, produced one and a half tool calls. For a session this long, that's tight — the model wasn't second-guessing itself.

---

## Phase distribution (`analyze_event_groups.py`)

```
result          950×   ← tool results
other           693×
response        628×   ← assistant messages
execute         438×   ← Bash
investigate     267×   ← Read/Grep/Glob
modify          211×   ← Edit/Write
prompt          135×   ← user messages (after filtering)
turn_boundary    63×
tool_other       28×   ← ToolSearch/AskUser/etc
thinking         21×   ← reasoning blocks
delegate          7×   ← Agent
```

`execute` (438) is more than `investigate + modify` (478) combined — Bash is doing double duty as both build runner and verification. `thinking` is only 21× — the model wasn't using extended thinking blocks much; it was thinking *between* tool calls, not as explicit reasoning records.

---

## Top tool transitions

```
Bash  → Bash       322×   ← repeated builds / git / tests
Read  → Edit        79×   ← read-then-modify
Edit  → Bash        62×   ← edit-then-verify
Read  → Read        54×   ← multi-file investigation
Edit  → Edit        54×   ← multi-edit per file
Bash  → Read        49×
Grep  → Grep        47×   ← refining searches
Edit  → Read        42×
```

The `Bash → Bash` spike (322×) is striking — that's the test/build/git churn. The classic **Read → Edit → Bash** triangle (79 + 62 = 141 transitions) is the implementation rhythm. `Grep → Grep` (47×) is search refinement.

---

## Token usage (`token_usage.py`)

```
Input tokens                       2,030
Output tokens                    328,836
Cache read tokens            639,232,223   ← 639M
Cache creation tokens          4,150,081
Total                        643,713,170

Estimated Cost (Sonnet rates)
  Input               $0.01
  Output              $4.93
  Cache read        $191.77
  Cache creation     $15.56
  ────────────────────────────
  Total             $212.27
```

**90% of cost is cache reads.** The session ran for 21 hours with a steadily growing context, and every turn re-read the accumulated history from the cache. The actual *new* content (output + cache creation) is only ~$20.49 — the rest is the price of long-running context. This is the cost shape of a "working session," not a "Q&A session."

---

## Sample sentences (verbatim, from `turn.sentence` detector)

These are gold — the deterministic narrative atoms the patterns layer produced. Quoted exactly:

1. *"Claude checked 10 operations, after reading 7 files, because 'yes please' → answered"*
2. *"Claude wrote parsed-squishing-island.md, story-data-surfacing.test.ts, after reading 18 files, while testing 11 checks, because 'my thought was you could write a lot of tests across the …' → answered"*
3. *"Claude edited translate.rs, after reading eval_apply.rs, event_data.rs, translate.rs, while testing 6 checks, because 'why can't I see code blocks in the story view?' → answered"*
4. *"Claude checked 2 operations, because 'yes please' → answered"*
5. *"Claude checked 6 operations, after reading 1 source, because 'can you examine session data and see where code is being …' → answered"*

These read like a labor log. The "because '…'" pattern is the sentence builder pulling the user prompt as the *cause clause* — that's the verbatim provenance link the Story tab needs.

Total `turn.sentence` patterns emitted: **111**. Total `turn.phase`: **144** (across the 63 model turns — phases overlap).

---

## Phase mix (`turn.phase` from patterns endpoint)

```
conversation              62
execution                 27
implementation            25
implementation+testing    21
testing                    4
exploration                3
delegation                 2
```

**Only 2 turns triggered subagent delegation** — matching Max's in-session question *"so only two turns in our current session have subagent delegation?"* The classifier is right: with 9 `Agent` tool calls but only 2 delegation-classified turns, most Agent calls happened in clusters within the same model turn.

---

## Eval-apply / pattern type counts

```
eval_apply.scope_open    2754
eval_apply.eval          1584
eval_apply.apply          956
eval_apply.scope_close    721
turn.phase                144
eval_apply.turn_end       112
turn.sentence             111
error.recovery             53
test.cycle                 31
```

**Mismatch worth flagging:** 2754 `scope_open` vs 721 `scope_close` — a 4× ratio. Scopes are being opened far more than they're being explicitly closed. Two interpretations:
1. The detector is missing close events in some compound-procedure shapes
2. Subagent flushes (`SubAgentSpawned` outcomes) are closing scopes implicitly without emitting `scope_close`

Either way it's a detector instrumentation question — and one that didn't surface during the session itself. **Worth filing in BACKLOG.**

`error.recovery: 53` is high — there were 53 detected error-recovery loops. With 21 hours and 956 tool calls, that's an error rate of ~5.5% per tool — believable for a build-heavy session. `test.cycle: 31` aligns with the visible TDD discipline.

---

## Gaps

Long pauses in the user-prompt timeline — the human-eat-sleep timeline:

| Gap | Wall clock |
|---|---|
| 03:54 → 11:24 | **7.5 h** (overnight) |
| 12:08 → 15:24 | 3.3 h (probably lunch / day job) |
| 19:37 → 22:05 | 2.5 h (evening break) |

Active conversation time: ~21h − 13.3h gap = **~7.7 h of real engagement**. The session "duration" of 21 hours is misleading — actual work was 7-8 hours spread over a day.

---

## Things this report didn't answer

These are the questions the existing scripts can't answer. Each is a candidate for either a new script or an extension to `sessionstory.py`:

1. **Per-turn cost.** `token_usage.py` only reports session totals. Knowing which turns burned the most cache would tell you which prompts were most expensive.
2. **Subagent identification.** With zero sidechain records, the subagents are inline. The 9 `Agent` calls are visible but tying them to specific turns / specific prompts requires walking `parent_uuid` chains.
3. **Why scope_open >> scope_close.** Needs detector-level instrumentation, not analysis. Worth a `pattern_audit.py` script.
4. **"What was in flight at the end."** This *is* answered — by `sessionstory.py SID --unfinished` (built in this very task). The trailing assistant messages reveal the unfinished docs work. Try it.
5. **Cross-session arc.** This report is single-session. A multi-session report ("the story of `feat/story-tab-data`") would need to pull all sessions on this branch and stitch them together.

---

## TL;DR for someone scanning

A 21-hour, $212, 4001-record working session that landed `feat/story-tab-data`. The rhythm was plan-then-execute spikes (10 turns of 100+ events each) interleaved with short clarifications (64 turns of <5 events). Tool usage was Bash-dominant with a clean Read → Edit → Bash implementation triangle. 95% of model turns were multi-cycle, 57% had user interjections mid-stream — this was a deeply collaborative session, not a hand-off-and-wait. The patterns layer caught 53 error-recovery loops and produced 111 narrative sentences. The biggest open question for the detectors is the 4× scope_open / scope_close mismatch.

---

## Want this for your own sessions?

OpenStory is open source and runs locally. Point it at your Claude Code transcripts, and you get the same data this report was built from — your CloudEvents in your filesystem, your tool calls, your patterns, your story.

```sh
git clone https://github.com/OpenStoryArc/OpenStory.git
cd OpenStory
just up                                      # starts NATS + server + UI
python3 scripts/sessionstory.py latest       # narrate your most recent session
python3 scripts/sessionstory.py --list       # find a specific session id
```

Then read [the README](../../../README.md) for the full setup, [`CLAUDE.md`](../../../CLAUDE.md) for the architecture, and [`docs/architecture-tour.md`](../../architecture-tour.md) for the 14-stop guided code walkthrough — including Stop 15, *"Using OpenStory as an agent,"* which walks an AI agent through how to use the same machinery to understand its own past sessions.

The whole thing exists because **the agent's internal narrative isn't the same as what the agent actually does**, and you deserve a tool that gives you the difference.
