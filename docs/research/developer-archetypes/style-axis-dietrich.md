# The Style Axis, Re-grounded on Dietrich (2004)

**Decision (2026-06-20):** after "The Conductor" was refuted (see
[`theory-foundations.md`](./theory-foundations.md)), the creative-**style** axis
re-grounds on **Dietrich 2004**, *"The cognitive neuroscience of creativity"*
(Psychonomic Bulletin & Review 11(6):1011-1026) — registry id `dietrich-2004`.

Chosen because, of all 16 verified constructs, only `dietrich-2004` and `boden-1990`
are **both** style-axis typologies **and** Process-locus (Rhodes). Since OpenStory
measures Process from session events, these are the only two eligible for a
`validated` warrant rather than a perpetual `analogical` one. Dietrich wins over
Boden for the *axis* because it is an explicit 2×2 with an observable processing-mode
dimension; Boden stays in the registry as the `kind:style` sibling for *what kind of
move* a change is (combinational / exploratory / transformational).

## Dietrich's 2×2 — and the honest boundary

```
                  cognitive content
                        │
   DELIBERATE  ─────────┼─────────  SPONTANEOUS
   (controlled,         │          (associative,
    effortful,         │           defocused,
    prefrontal)         │           unconscious)
                        │
                  emotional content
```

Two independent axes:
1. **Processing mode — deliberate ↔ spontaneous.** *Observable from session events.*
2. **Content source — cognitive ↔ emotional.** **NOT session-observable.** A
   process-from-text observer sees *what was done*, not the affective source driving
   it. We do not see emotion.

> **Honest scope:** OpenStory can validate **one** of Dietrich's two axes
> (deliberate↔spontaneous) against its signal. The cognitive↔emotional axis is
> **parked as out-of-reach**, not faked. Claiming we measure it would be the exact
> over-reach Cronbach & Meehl warn against. The style axis is therefore a *line*
> (one validated dimension), not yet the full quadrant.

## Operationalization: the nomological net for deliberate ↔ spontaneous

Per Cronbach & Meehl, a `validated` warrant needs the construct to touch observables.
Candidate session signals (all already derivable from the REST analytics + tool-journey):

| pole | session observables |
|---|---|
| **deliberate** (controlled, sequential) | plan-before-action (ExitPlanMode / `/plans`); high read-before-edit ratio; test-runs interleaved with edits; long coherent tool chains; low tool-sequence entropy; structured eval-apply decomposition |
| **spontaneous** (associative, exploratory) | breadth-first search bursts (Grep/Glob fan-out); rapid file/context switching; edit→run→edit churn (trial-and-error); low pre-planning; high tool-sequence entropy |

A session (or a developer over a window) gets a **deliberate↔spontaneous score** from
the balance of these signals. This is `validated`-**eligible** — it becomes
`validated` only after the mapping is empirically checked (does the signal track an
independent measure of the construct?). Until then it is tagged `analogical` and the
qualifier says so. **No free upgrades.**

**Falsifiability hook (the `rebuttal` field):** the mapping is refuted if
deliberate-signal sessions do not differ from spontaneous-signal sessions on any
independent indicator of controlled vs. associative work — e.g. if "high read-before-edit"
sessions show the *same* outcome-revision churn as low ones. A construct that cannot
fail is not validated (Cronbach & Meehl: *"Rationalization is not construct validation."*)

## What this replaces

- The hand-tuned `steering` dimension (which produced "Conductor") is **retired** as
  the style signal. Its component metrics (session count, collaboration, focus) are
  not discarded — several map onto the deliberate↔spontaneous net above.
- **Archetype names** (Paxel-style flavor) may stay, but every name carries
  `warrant_strength: metaphorical` by definition — a name licenses no number. The
  rule that "Conductor" violated: *a metaphorical name may reference a construct in
  its backing only if it does not contradict that construct's meaning.* Sawyer's
  conductor contradicts emergence, so no orchestration name may cite Sawyer.

## The two-axis map, now grounded

- **Creative-style axis** — Dietrich deliberate↔spontaneous (Process, validated-eligible).
- **Delivery-flow axis** — DORA capabilities + SPACE (the "not a single metric" warrant),
  bounded to session-observable signals (lead-time-like, activity/flow; *not* prod CFR/MTTR).

Both are **styles/positions, not scores.** Neither ranks the developer.

## Next concrete step (not yet done)

Prototype a `deliberate↔spontaneous` signal extractor over `tool-journey` + patterns
for a window, output the single style score with its backing + `analogical` qualifier,
and eyeball it against real sessions before claiming anything. That is the smallest
honest increment — measure the one axis we can actually see, tagged at the strength we
can actually defend.
