# Distribution Plan: `hermes-openstory`

*How to ship the Hermes ↔ OpenStory integration as a standalone package, without asking the Hermes maintainers to merge anything.*

---

## Why a standalone package, not an upstream PR

Hermes already supports third-party plugins via the `hermes_agent.plugins` entry-point group (verified at `hermes_cli/plugins.py:68` and `hermes_cli/plugins.py:339`). Any pip-installable package that exposes a `register(ctx)` function under that group is loaded automatically by Hermes's `PluginManager._scan_entry_points()` at startup. This means:

- **No code changes to hermes-agent are required.** Users install the package and it works.
- **The integration owns its own release cadence.** It can iterate without waiting for upstream review cycles.
- **The Hermes maintainers don't take on a third-party concern.** Asking them to merge an OpenStory integration commits them to maintaining it. Asking them to *link to* an existing community plugin is a much smaller ask.
- **The OpenStory project owns the boundary.** The CloudEvent shape is OpenStory's, the translator is OpenStory's, and the plugin is the smallest possible adapter on the Hermes side. If OpenStory's CloudEvent shape changes, the plugin doesn't need to.

Upstream contribution remains an option as a **follow-up**, in the smallest possible form: a single line in Hermes's docs that lists `hermes-openstory` as a community plugin example. That's a one-paragraph PR with zero code, much easier to land than a feature merge.

---

## What gets built

### Track A — OpenStory side

1. **Verify Hermes message dict shape against a real session.** Boot a Hermes session (containerized is fine), run a 5-turn task that exercises a tool call and a thinking block, finalize, read `~/.hermes/logs/session_{id}.json`. Resolve every `# VERIFY:` marker in `translate_hermes.py`. Tests in `test_translate.py` should still pass; add new test cases for any edge shapes discovered.
2. **Port `translate_hermes.py` to `rs/core/src/translate_hermes.rs`.** Parallel to the existing `translate.rs` (Claude Code) and `translate_pi.rs` (pi-mono). The Python sketch is the executable spec — once it agrees with real Hermes data, the Rust port is mechanical translation.
3. **Add Hermes-source recognition to the file watcher.** The watcher already handles JSONL; the change is to detect Hermes-shape lines (envelope with `"source": "hermes"`) and route them through `translate_hermes.rs` instead of the Claude Code translator. Likely a path-pattern check (e.g. `*/openstory-events/*.jsonl`) plus a sniff of the first line.
4. **Add tests** for the new translator and the watcher routing, parallel to the existing tests for `translate_pi.rs` and Claude Code ingestion.
5. **Add a backfill script** at `scripts/backfill_hermes_sessions.py` that reads `~/.hermes/logs/session_*.json` files and emits Hermes-native event JSONL into the watched directory. One-shot, useful for retroactively ingesting sessions that existed before the integration.

**Effort estimate:** ~1 day if message shapes verify cleanly; ~2 days if tool-call shape varies more by provider than the prototype assumes.

### Track B — Standalone `hermes-openstory` package

1. **Verify Hermes message dict shape** (shared with Track A — same one-shot session, same artifact resolves both tracks).
2. **Verify Hermes hook signatures.** Specifically `pre_llm_call`, `post_llm_call`, `pre_tool_call`, `post_tool_call`, `on_session_start` — the kwargs Hermes passes to each. Read the `invoke_hook(...)` call sites in `hermes-agent/run_agent.py`, `gateway/run.py`, and `cli.py` and resolve the `# VERIFY:` markers in `plugin_sketch.py`.
3. **Move the prototype files into a real package layout:**
   ```
   hermes-openstory/
     pyproject.toml          # from pyproject.toml.example
     README.md
     LICENSE                 # Apache-2.0 to match OpenStory
     hermes_openstory/
       __init__.py
       plugin.py             # from plugin_sketch.py
       recall_tool.py        # from recall_tool_sketch.py
       writer.py             # _SessionWriter, factored out
     tests/
       test_writer.py
       test_recall_smoke.py  # mocks OpenStory API responses
   ```
4. **Wire up CI** — pytest on the package alone (no live OpenStory required for unit tests; mock the HTTP layer).
5. **End-to-end smoke test:** spin up OpenStory locally, install the plugin into a containerized Hermes, run a session, watch events flow through the dashboard, call the `recall` tool from inside the session and confirm it returns a structural answer.
6. **Publish to PyPI as `hermes-openstory`** at version 0.1.0.

**Effort estimate:** ~1 day for the package layout, CI, and smoke test. Verification (step 1–2) is shared with Track A, so the marginal cost is small.

### Track C — Optional follow-up

- **A docs PR to `NousResearch/hermes-agent`** that adds `hermes-openstory` to a "community plugins" list (if such a list exists, or as the first entry creating one). One paragraph, zero code, much easier to get merged than a feature contribution.
- **A blog post or research note** describing the integration and the categorical framing from `LISTENER_AS_ALGEBRA.md`. Audience: agent observability folks, anyone running Hermes who'd benefit from session-level introspection.

---

## What "verified" means

The verification step is small but it is the load-bearing prerequisite for everything downstream. It produces three concrete artifacts:

1. **One finalized Hermes session log** at `~/.hermes/logs/session_{id}.json` from a real run, captured and saved as a fixture in the prototype directory (alongside `example_hermes_events.jsonl`).
2. **A short note** — one paragraph — in this file's "Verification results" section below, listing what was confirmed and what surprised us.
3. **Updated `# VERIFY:` markers in `translate_hermes.py` and `plugin_sketch.py`** — either deleted (resolved) or rewritten (still uncertain, more specific now).

After verification, the prototype directory is no longer scaffolding — it's a working reference that the production package and the production translator can both be ported from with confidence.

---

## How to run the verification step

Max has offered to spin up a Hermes container. The minimum protocol:

1. **Boot a Hermes container** from `NousResearch/hermes-agent` (the repo has a Dockerfile and docker-compose files).
2. **Run a short session.** A reasonable verification task:
   > "Read the README.md in this directory, then list all Python files, then write a one-line summary of the project to /tmp/summary.txt."

   That exercises: a user prompt, a thinking block (Hermes will likely use one), at least three tool calls (Read, Glob/LS, Write), tool results, and a terminal assistant message. Six turns total, finalizes cleanly.
3. **Capture the session log** by copying `~/.hermes/logs/session_{id}.json` out of the container.
4. **Read it carefully.** Specifically look for:
   - The shape of `tool_calls` on assistant messages (OpenAI vs. Anthropic)
   - The shape of `tool` role messages (key names for `tool_call_id`, `tool_name`)
   - Whether `reasoning` is a top-level field or inside `content` blocks
   - Whether system-injected messages (compression summaries, etc.) appear, and how they're tagged
5. **Resolve the `# VERIFY:` markers** in `translate_hermes.py` and `plugin_sketch.py` by editing the code to match what's in the captured log. Re-run `test_translate.py` to confirm nothing breaks.
6. **Save the captured log** as a fixture in `hermes-integration/fixtures/real_hermes_session.json` (gitignore the inside, but commit a redacted version).
7. **Update the "Verification results" section** below.

**Estimated time:** ~30 minutes if Hermes boots cleanly and the shape is roughly as expected. Up to 2 hours if there are multiple provider modes to check and the shapes differ.

Two providers are worth checking, not one: an Anthropic-direct provider (e.g., `anthropic/claude-opus-4-6`) and an OpenAI-shaped provider (e.g., `openai/gpt-4o` or any OpenRouter model). The translator has branches for both and both branches should be exercised before publication.

---

## Verification results

*(To be filled in after the verification step is complete. Until then, treat the translator and plugin as scaffolding.)*

- [ ] Hermes boots in a container
- [ ] One session finalizes cleanly with a visible log file
- [ ] Tool call shape confirmed for at least one Anthropic-direct provider
- [ ] Tool call shape confirmed for at least one OpenAI-shaped provider
- [ ] `tool` role message keys confirmed
- [ ] Reasoning/thinking storage confirmed
- [ ] System-injected messages observed and tagged
- [ ] Hook kwargs confirmed for `post_llm_call`, `post_tool_call`, `on_session_start`
- [ ] All `# VERIFY:` markers in `translate_hermes.py` resolved
- [ ] All `# VERIFY:` markers in `plugin_sketch.py` resolved
- [ ] `test_translate.py` passes against a real-data fixture, not just synthetic data
- [ ] One end-to-end run: Hermes session → JSONL → OpenStory dashboard → `recall` tool answers

---

## After v0.1.0

Once the v0.1.0 package is shipped and a few people are using it, the higher-leverage features become unlockable:

| Feature | Effort | Value |
|---|---|---|
| Skill-extraction signal feed (use OpenStory synopses to inform Hermes's autonomous skill creation) | medium | high — directly improves Hermes's self-improvement loop |
| Cross-provider behavioral comparison endpoint | medium | medium — novel research output, requires running same task across providers |
| StructuralTurn training data export | medium-large | medium-high — research question about whether structural decomposition improves tool-calling models |
| Live `live.story` WebSocket subscription from inside Hermes (so the agent can react to its own observed structure in real-time) | large | depends — careful here, this edges close to closing a feedback loop into the coalgebra |

The fourth one is the dangerous one and is mentioned only to be flagged: a real-time subscription from agent to listener is *fine* as long as the agent doesn't act on it in a way that would have changed its own trace. The moment it does, the algebra/coalgebra purity from `LISTENER_AS_ALGEBRA.md` breaks. If this feature is built, it should be built with that constraint stated explicitly in the docs and enforced architecturally if possible.
