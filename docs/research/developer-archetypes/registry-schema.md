# Theory Registry & Traceable-Assessment Schema

**Status:** schema sketch (incubating). Defines the data shapes that make an
archetype assessment traceable in both directions — down to the events, up to the
literature. Grounded in the verified findings in
[`theory-foundations.md`](./theory-foundations.md).

Two artifacts, two open formats (per OpenStory §6):

- **Theory registry** — `theory-registry.json` (machine-readable seed beside this
  doc). Canonical authoring may later move to markdown-with-frontmatter (one file per
  construct, like the soul docs), compiled to this JSON for the view. JSON first
  because the renderer and the rubric both consume it.
- **Assessment** — emitted at runtime as a CloudEvent (`type: io.arc.assessment`)
  whose `data` carries the full Toulmin chain. Same open envelope as every event it
  derives from; portable and user-owned.

---

## 1. Theory record

One record per construct. Fields:

| field | type | meaning |
|---|---|---|
| `id` | string (kebab) | stable key the rubric cites, e.g. `boden-1990` |
| `scholar` | string | originating author(s) |
| `year` | int | year of the **primary** source (anchor, not latest restatement) |
| `primary_work` | string | full title + venue |
| `primary_source` | url | traceable primary source |
| `core_claim` | string | the one-line claim, verbatim where possible |
| `measures` | enum | `person` \| `process` \| `product` \| `press` (Rhodes' Four Ps — the locus this construct assesses) |
| `kind` | enum | `style` (type, not degree) \| `degree` (level/magnitude) \| `method` |
| `lineage` | edge[] | `{relation, target_id_or_name, note}`; relation ∈ `descends_from` \| `responds_to` \| `sibling_of` |
| `history_note` | string | the O'Malley-style contextualization: what tradition, what it reacted to, how it evolved |
| `caveats` | string[] | over-claim guardrails (e.g. Amabile's extrinsic-motivation nuance) |
| `verification` | object | `{vote, sources[], pass}` — provenance of the *record itself* |

`measures` and `kind` are the load-bearing fields:
- `measures` drives the **warrant-strength rule** (§3). OpenStory observes `process`;
  citing a `person`/`product` construct caps the warrant at `analogical`.
- `kind` keeps axes honest: `style` constructs feed the **archetype axes**; `degree`
  constructs (e.g. Four-C) feed the **growth-edge / trajectory**, never the style map.

---

## 2. Assessment record (the Toulmin chain)

What the system emits. Every field is inspectable. The example below is the **actual
"Conductor" assessment that pass 2 REFUTED** — kept verbatim because a killed
assessment is the best demonstration that the mechanism works.

```jsonc
{
  "claim":   "The Conductor",                 // the verdict
  "grounds": {                                // empirical provenance (downward)
    "metrics":  { "steering": 6.1, "session_count": 340, "collaboration": 0.011 },
    "evidence_event_ids": ["…"],              // traces to raw CloudEvents
    "window":   { "days": 30 }
  },
  "warrant": "Directing many short, collaborative loops indicates an orchestration-oriented style: managing the system of creation rather than producing in long solo bursts.",
  "backing": [                                // intellectual provenance (upward)
    { "theory_id": "sawyer-2009", "warrant_strength": "metaphorical",
      "status": "REFUTED",
      "refutation": "Sawyer places the conductor-led orchestra at the PREDICTABLE pole — his counter-example to creative emergence. Citing it here inverts the author's meaning." }
  ],
  "qualifier": { "confidence": 0.0, "warrant_strength": "refuted" },
  "rebuttal":  "Overturned: the backing means the opposite of the claim."
}
```

The reader walks `grounds.evidence_event_ids` down to the sessions and
`backing[].theory_id` up to the registry record and its primary source. Here that
walk *fails productively*: the upward trace lands on a source that contradicts the
verdict. **A `status: REFUTED` on any backing forces the qualifier to `refuted` and
the assessment must not be shown as a conclusion.** That is the whole point — the
audit trail caught a conceptual error, not just a bad number.

---

## 3. `warrant_strength` — the discipline (Cronbach & Meehl 1955; Toulmin 1958)

A typed field on every link from a metric to a construct. **The qualifier never
exceeds the weakest backing.**

| tier | when it applies | nomological-net test |
|---|---|---|
| `validated` | the metric is an operationalization the construct's **own** literature endorses, with observable-touching laws | passes Cronbach-Meehl net contact |
| `analogical` | defensible reasoning theory→signal, but the construct `measures` person/product while our signal is `process` | proxy, not validated |
| `metaphorical` | the construct frames the **language/name** but does not license the number | naming only |

**Hard rule (from the research):** `measures ∈ {person, product}` + OpenStory signal
is `process` ⟹ `warrant_strength ≤ analogical`, **never `validated`.** Encoded as a
lint over every backing edge.

> Cronbach & Meehl: *"Rationalization is not construct validation… cannot maintain
> his claim in the face of recurrent negative results."* The `rebuttal` field is the
> falsifiability hook — every assessment must state what would refute it.

---

## 4. How the rubric cites the registry

`profile_dimensions.py` today blends raw metrics into dimension scores. The cited
version annotates each dimension with its backing — declarative, beside the weights:

```python
# illustrative — the citation layer over the existing rubric
DIMENSION_BACKING = {
    "execution": [
        ("boden-1990",     "analogical",   "write/throughput intensity ~ combinational+exploratory output"),
        ("space-2021",     "analogical",   "activity dimension of developer productivity"),
    ],
    "engineering": [
        ("dora-2018",      "analogical",   "test-run cadence ~ continuous-testing capability"),
    ],
    # steering → "Conductor" was REFUTED (pass 2): sawyer-2009 means the OPPOSITE.
    # No replacement backing yet. The style axis should re-ground on a process-locus
    # construct — dietrich-2004 (deliberate↔spontaneous × cognitive↔emotional) or
    # boden-1990 — the only two eligible for a `validated` (not analogical) warrant.
}
```

`build_profile()` then attaches `DIMENSION_BACKING` to each score so the view can
render the Toulmin card. A future check fails the build if any backing cites a
`theory_id` absent from the registry, or violates the §3 hard rule.

---

## 5. What's seeded vs. pending

`theory-registry.json` ships the **7 verified records** (pass 1). The 9 constructs in
the [second pass](./theory-foundations.md#gaps--needs-a-second-verification-pass) —
Kirton, Rhodes, Guilford, Csikszentmihalyi, Sawyer, Kaufman-Beghetto, et al. — are
**absent on purpose**. The registry should never contain an unverified record; the
gap is the spec for the next pass. When pass 2 lands, each verified construct appends
one record, and the `steering`/Conductor backing can finally be filled (or refuted).
