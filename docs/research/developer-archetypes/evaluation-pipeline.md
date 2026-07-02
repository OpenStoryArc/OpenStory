# The Evaluation Pipeline — every function, every variable, every warrant

The archetype "evaluation" is a composition of **pure functions**: data in, value
out, no hidden state. This document makes each stage visible and, for **every
variable**, states what research backs it and how strongly — including (honestly)
the many that are analogical, metaphorical, or unbacked. Writing this down is the
point: it turns out the **structure** is well-founded and most of the **signals**
are a first draft. This is the spec for what to validate next.

The machine-readable companion to this doc is `metric_trace.SIGNAL_BACKING` and
`metric_trace.PIPELINE_BACKING` — the same warrants, callable from code.

Warrant strength (from Cronbach & Meehl 1955, via `registry-schema.md`):
`validated` > `analogical` (defensible proxy) > `metaphorical` (framing only) > `none`.

---

## The function chain (the whole evaluation, at a glance)

```
tool_call            steps_from_records      derive_all          combine_lean         interpret
CloudEvents  ──────▶  [Step]          ──────▶ {Derivation} ─────▶ {indices,net_lean} ─▶ {claim, meaning}
(root)               (Stage 1)               (Stage 2)            (Stages 3–4)          (Stage 5)
                                                                        │
                                  session_reading(steps) = one atom ◀───┘
                                                                        │
                              many atoms ──▶ distribution()  (Stage 6: the spread, by session/kind)
```

Each arrow is one named function. Every number the report shows is the output of
running this chain on your events.

---

## Stage 1 — Observe · `steps_from_records(records) -> [Step]`

Projects raw `tool_call` CloudEvents to the normalized unit
`Step{seq, id, tool, target}`. `target` = `file_path` (file tools) or `command`
(Bash). The aggregate path uses `steps_from_journey` (no ids, faster); the
procedures don't care which — so counting logic can't diverge.

| variable | what | backing | strength |
|---|---|---|---|
| the unit = `tool_call` event | we read the **process**, not the person or product | **Rhodes 1961** — Process is one of the Four Ps, a legitimate locus | **validated** |

This is the strongest claim in the whole pipeline: OpenStory measures the Process
strand, which most creativity instruments (Person/Product) cannot.

---

## Stage 2 — Signal procedures · `derive_all(steps) -> {Derivation}`

Six pure procedures, each `Steps -> Derivation` (numerator / denominator / value +
the events it counted). **This is where the research backing gets thin.** The
*frame* they feed (deliberate↔spontaneous) is Dietrich; the *mapping of each
signal to a pole* is at best analogical.

| signal | formula (num ÷ den) | pole | why (warrant) | backing | strength | known flaw |
|---|---|---|---|---|---|---|
| `read_before_edit` | edits whose file was Read earlier ÷ edits | deliberate | examine-then-act ≈ controlled, top-down processing | dietrich-2004 | **analogical** | harness file-state → Write-then-Edit undercounts |
| `test_cadence` | test-runner Bash cmds ÷ edits | deliberate | interleaved verification ≈ analytic loop | dietrich-2004 | **metaphorical** | testing is a *delivery* discipline (dora-2018), arguably wrong axis; detector misses `python --test`, false-positives on text |
| `plan_density` | plans ÷ sessions | deliberate | goal-first planning ≈ goal-guided processing | dietrich-2004 | analogical | **NON-FUNCTIONAL** (always 0) — deflates deliberate |
| `search_fanout` | Grep+Glob ÷ all tool-calls | spontaneous | broad search ≈ exploratory/associative | boden-1990 | **analogical** | Bash `rg`/`grep` uncounted → ≈0 |
| `context_switch` | file-ops that changed file ÷ file-ops | spontaneous | file-jumping ≈ defocused attention | dietrich-2004 | **analogical** | contested: could be deliberate multi-file coordination |
| `churn` | edits to an already-edited file ÷ edits | spontaneous | re-editing ≈ trial-and-error | *(none — closer to BVSR/Campbell-Simonton)* | **metaphorical** | iterative refinement is often **deliberate** — weakest mapping |
| `seq_entropy` | normalized entropy of tool bigrams | spontaneous | varied sequences ≈ less-structured/associative | *(none — info-theoretic heuristic)* | **metaphorical** | "structured chains = deliberate" is an untested assumption |

**The honest read of this stage:** 0 of 7 signals are `validated`. Four are
`analogical` (read_before_edit, plan_density, search_fanout, context_switch),
three are `metaphorical` or mis-axed (test_cadence, churn, seq_entropy), and three
are currently broken (plan_density, search_fanout, test_cadence). The mapping
"these process signals indicate Dietrich's processing mode" has **not** been
checked against any independent measure — it is reasoned, not validated.

---

## Stage 3 — Normalize & cap (inside `combine_lean`)

Each signal is scaled to 0–1 before combining:
`read_before_edit` raw · `test_cadence`÷0.4 · `plan_density`÷1.0 · `search_fanout`÷0.2 ·
the rest raw.

| variable | backing | strength |
|---|---|---|
| the caps `0.4`, `0.2`, `1.0` (what "a full signal" looks like) | hand-picked | **none** |

These constants set how fast a signal saturates. Nothing justifies the specific
values — they are tunable guesses.

---

## Stage 4 — Combine into the axis · `combine_lean(sig) -> {indices, net_lean}`

```
deliberate_index  = mean( read_before_edit, test_cadence', plan_density' )   # 3 terms
spontaneous_index = mean( search_fanout', context_switch, churn, seq_entropy ) # 4 terms
net_lean          = deliberate_index − spontaneous_index                       # −1 … +1
```

| variable | backing | strength |
|---|---|---|
| the bipolar **deliberate↔spontaneous axis** itself | **dietrich-2004** — one of his two processing-mode dimensions | **validated-frame** (the construct is real; this operationalization is analogical) |
| equal weights within each pole | default | **none** |
| **the 3-vs-4 split** | — | **none — and it's a bug** |

⚠ **Structural tilt (surfaced by writing this out):** `deliberate_index` divides
by 3, but `plan_density` is always ≈0, so its real max is `(1+1+0)/3 = 0.67`.
`spontaneous_index` divides by 4 live signals and reaches `1.0`. The axis is
therefore **biased toward spontaneous by construction** — part of why the
distribution leaned spontaneous is the formula, not the behavior.

---

## Stage 5 — Interpret · `interpret(sig, ds)` + `lean_label(net)`

`net_lean` → a `claim` label and a plain-language `meaning`; `drivers` rank each
signal's signed push on net (`+norm/3` deliberate, `−norm/4` spontaneous).

| variable | backing | strength |
|---|---|---|
| thresholds `±0.08`, `±0.25` (the label bands) | hand-set | **none** |
| the meaning sentences ("associative… exploring… iterating") | dietrich-2004 mode descriptions | **analogical** |
| driver contribution = signal's term in the mean | follows directly from Stage 4 | mechanical (exact) |

---

## Stage 6 — Aggregate · `session_reading` (the atom) → `distribution`

`session_reading(steps)` is **the primitive**: one session's reading. The
aggregate is *not* a separate computation — it is many atoms. The distribution
reports the **median** of per-session leans and the spread, not a volume-weighted
pool.

| decision | backing | strength |
|---|---|---|
| show the **distribution**, not one number | **dietrich-2004** (mode is task/episode-dependent, not a trait) + **space-2021** ("cannot be measured by a single metric") | **validated-frame** |
| **median** over volume-weighted pool | honesty — the pool is dominated by a few giant sessions | design choice |
| segment by **task-kind** (proposed) | mode varies by task (Dietrich) | analogical, not yet built |

This stage is well-founded: refusing to collapse heterogeneous sessions into one
trait-like number is exactly what Dietrich and SPACE license.

---

## The backing ledger (the whole pipeline, summarized)

| layer | what it is | strength |
|---|---|---|
| **Process locus** (Stage 1) | measure process from tool_call events | **validated** |
| **The axis** (Stage 4) | deliberate↔spontaneous exists as a construct | **validated-frame** |
| **Distribution-not-pool** (Stage 6) | don't reduce to one trait number | **validated-frame** |
| read_before_edit, plan_density, search_fanout, context_switch | signal→pole mappings | **analogical** |
| test_cadence, churn, seq_entropy | signal→pole mappings | **metaphorical / mis-axed** |
| caps (0.4, 0.2), weights, thresholds, 3-vs-4 split | the arithmetic glue | **none** |

## The honest conclusion

**What is research-backed:** the *frame* — that we measure Process (Rhodes), that
deliberate↔spontaneous is a real axis (Dietrich), and that it should be shown as a
distribution not a single trait (Dietrich + SPACE). The skeleton is sound.

**What is not:** every individual signal→pole mapping (analogical at best; three
are metaphorical or on the wrong axis), and all the numeric glue (caps, weights,
thresholds). Plus three broken signals and one structural tilt.

**Therefore:** nothing in this pipeline may be presented as a verdict — its
warrant caps at `analogical`, and the report's qualifier must say so. The next
real work is **validation of the signal layer**: pick an independent indicator of
controlled-vs-associative work and check whether these signals actually track it
(Cronbach & Meehl's nomological-net test). Until then, this is a transparent,
honest *draft instrument* — which, given that it shows its own seams, is already
more than Paxel offers.
