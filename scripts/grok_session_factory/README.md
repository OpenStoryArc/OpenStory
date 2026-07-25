# Grok Session Factory

Prototype: generate **statistically viable** Grok Build session trees from an
**SVO action catalog**, not from five hand-written scenarios.

## Intent

OpenStory's Grok support needs more than wire-shape smoke tests. It needs a
**coverage set of software acts** — the physics of harness agency:

| Role | In a session | Examples |
|------|----------------|----------|
| **Subject** | agent principal | `grok-build` main session |
| **Verb** | tool / speech act | `read_file`, `search_replace`, `run_terminal_command`, answer |
| **Object** | tool args / path / query | `src/main.rs`, `git status`, FTS query |
| **Motive** | user prompt + thought | "fix the bug" + reasoning chunk |
| **Result** | tool outcome / turn end | ok, error, recovered, tests green |

Real sessions are **compositions** of these atoms inside an eval-apply loop.
This factory:

1. Declares an action catalog (verbs + object templates + story roles)
2. Composes multi-turn sessions as sequences of **stories**
3. Emits a full session tree (`updates.jsonl` + optional `events.jsonl`)
4. Measures coverage against the catalog
5. **Loops** until coverage targets are met (or max sessions)

## Layers (what already exists vs this)

| Layer | Tool | Purpose |
|-------|------|---------|
| Hand scenarios 01–05 | `scripts/gen_grok_fixtures.py` | Deterministic goldens |
| Real turn extracts | `scripts/extract_grok_session_seed.py` | Wire fidelity from life |
| **This factory** | `scripts/grok_session_factory/` | Statistical action coverage |

## Run

```bash
# One multi-turn corpus session (default out/)
python3 scripts/grok_session_factory/main.py generate

# Loop until catalog coverage ≥ target (default 0.9)
python3 scripts/grok_session_factory/main.py loop --target 0.95 --max-sessions 40

# Coverage report only (against last out/)
python3 scripts/grok_session_factory/main.py cover

# Self-test
python3 scripts/grok_session_factory/main.py --test
```

Output layout (watchable if pointed at `out/sessions`):

```
out/sessions/%2Fworkspace%2Fdemo/<session-id>/
  updates.jsonl      # ACP stream (OpenStory primary watch)
  events.jsonl       # loop kinematics (optional second sensor)
  chat_history.jsonl # reconstructed narrative (optional)
  MANIFEST.json      # which catalog acts were used
```

## Design bets

1. **SVO is the unit of coverage**, not "number of JSONL lines."
2. **Stories** (e.g. explore-then-edit, fail-then-recover, mcp-query) are the
   composition grammar — closer to real sessions than random tool soup.
3. **Multiple files are sensors on one physics**, not competing formats.
4. **Loop** means: emit → measure gaps → bias next session toward missing acts.
