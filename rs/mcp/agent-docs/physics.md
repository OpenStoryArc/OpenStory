# Physics — what you may treat as ground

OpenStory is a **listener**. Agents unfold (coalgebra); OpenStory folds the
trace into structure you can query. Science here means: **every claim can walk
back to stored events / projections.** Interpretation (intent %, themes, “what
it meant”) is **your** job as a model — not something the store asserts.

## Layers

| Layer | What it is | How you get it |
|-------|------------|----------------|
| **Events** | CloudEvents from coding agents (immutable observation) | `search`, records via API, `subscribe_session`, transcript tools |
| **Turns** | Eval/apply structure when turn boundaries exist | patterns / story fact sheets |
| **Outcomes** | Typed world deltas (file created/modified/read, command, search, subagent, …) | embedded in patterns / file_impact style analytics |
| **Sentences** | SVO **projection** over a turn (subject/verb/object/…) | `session_sentences`, sample in `session_story` |

```text
raw transcript → events → turns → outcomes → sentences (coordinates)
```

## Sentences (coordinates, not monologue)

`session_sentences` / `turn.sentence` patterns are **deterministic projections**
from tool acts + turn structure (e.g. Write → “wrote” + path-ish object). They
are useful for orientation and reporting. They are **not**:

- the model’s private chain-of-thought
- a ground-truth label of human or agent intent
- guaranteed for every session (see soft holes)

When you report them, say what the tools did (or quote the projection), not
“the agent intended to refactor auth.”

## Soft holes (do not over-claim)

| Hole | Symptom | Fallback |
|------|---------|----------|
| No turn boundaries | Zero `turn.sentence` / sparse patterns | `session_activity`, `session_transcript`, `tool_journey` |
| Federated / remote sessions | Records exist, patterns empty | Same fallbacks; patterns may need local recompute |
| Bash-heavy turns | Verb classification softer than Write/Edit | Prefer `file_impact` / raw tool journey |
| Pure text turns | Thin objects, “explained”/“answered” | Read transcript snippet |
| Thinking / monologue | Only if the **source agent emitted it** into the store | Never invent inner monologue for closed models |

## Citation path

```text
claim
  → tool result fields (session_id, path, verb, timestamp)
  → event ids when present
  → raw event / transcript only if needed
```

If you cannot cite, you do not have physics — you have a guess. Label guesses
as yours.

## UI is not history

Driving the dashboard (`ui_control`) changes **what is shown** (`ui.*`). It does
not create, delete, or rewrite observed `events.*`. Follow the human with
`where_is_user` / `subscribe_ui_state`. Full map: `openstory://docs/agent-in-ui`.

## Related

- Motions and flows: `openstory://docs/hands`
- Help router: tool `openstory_help`
