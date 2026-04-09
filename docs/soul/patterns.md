# Patterns

What we've learned works — and what doesn't — from building Open Story.

## Patterns that work

### Boundary table BDD

Write the full spec as a data table before implementing. Cover: empty, single, many, overflow, tie-breaking, error cases. The table IS the spec.

```typescript
const CASES: [string, Input, Expected][] = [
  ["empty input",    [],    []],
  ["single item",    [x],   [y]],
  ["overflow",       [...],  [...]],
  ["tie-breaking",   [...],  [...]],
];

it.each(CASES)("%s", (_, input, expected) => {
  scenario(
    () => input,
    (x) => transform(x),
    (result) => expect(result).toEqual(expected),
  );
});
```

Every edge case in one place. Readable, compact, catches what narrative tests miss.

### scenario(given, when, then)

Pure pipeline: data flows explicitly through three functions. No shared mutable state, no hidden closures. Every test follows this shape. It makes tests scannable and prevents setup pollution between tests.

### Prototype in scripts, implement in production code

Query real data with a script. Validate the data model. Print human-readable output. Then implement in production code. The prototype catches wrong assumptions before you invest in UI.

This prevented us from building a tree UI for data that isn't a tree. It revealed that most sessions are subagent spawns. It showed that truncation saves almost nothing.

### Shared rendering, separate data ownership

`EventCard` is shared between Live and Explore. Same card, different data source. Share the presentation, never the data fetching.

### Single-pass index building

Build all indexes (turns, files, tools, agents, errors) in one iteration. No second passes, no lookups during construction. O(n) and cache-friendly.

### Noise filtering at fetch time

Strip non-displayable record types (token_usage, file_snapshot, session_meta) immediately after fetch. Filter once, not on every render.

### File hint propagation

To syntax-highlight Read tool results, the file path comes from the parent tool_call. Instead of Map lookups, track `lastToolFilePath` as you iterate — set on tool_call, consumed on tool_result, cleared after. Zero overhead.

### Pure functions as the unit of work

Every feature starts as a pure function with a boundary table. Components compose those functions. Test the functions, trust the composition.

### Semantic parity, not byte parity

When a trait has multiple implementations — say, an `EventStore` trait with a SQLite backend and a MongoDB backend — the natural instinct is to write a conformance suite that asserts byte-for-byte identical results. That instinct is wrong. It treats the two backends as fungible and forces one to mimic the other's idiom. The right model is **semantic equality per query**: each query has a contract (what question it answers), and each backend honors the contract using whatever primitive is natural for it. The conformance suite enforces the *answer*, not the *implementation*.

Three test patterns make this concrete:

- **C1 — Strict equality (`assert_eq!`)** — for queries where the answer is mathematically defined: counts, sums, ordered sets with well-defined sort keys. Both backends must produce byte-identical output. If they don't, one has a real bug.
- **C2 — Canonical-sort then equality** — for queries where the SET is well-defined but tie order is implementation-defined. The conformance helper sorts both outputs by a stable secondary key before comparing. The looseness is encoded at the assertion site, where any future reader sees exactly what's tolerated and why. Not buried in fixture data, not hidden in a guard.
- **C3 — Redesign the API to remove the cosmetic field** — when the divergence between backends is in a *cosmetic* field doing too much work (both bucket identifier and display label), fix the API. Replace the cosmetic field with the underlying structural data and let the consumer handle formatting.

Worked example: the MongoDB sink (`feat/mongodb-sink` branch) implements 12 analytics queries on top of the `EventStore` trait. SQLite uses `json_extract` + `strftime` + `LIKE`. Mongo uses dotted-path access + `$dateFromString` + `$exists`. They're different idioms reaching the same answer. Of the 12 queries, 8 are pure C1, 3 are C2 (`top_tools`, `session_efficiency`, `recent_files`), and 1 needed C3 (`tool_evolution` — its old `week: String` field was both a bucket identifier AND a display label, with SQLite's `%W` and Mongo's `$isoWeek` disagreeing at year boundaries; the fix was replacing it with `bucket_start: String` + `bucket_end: String`, which both backends compute identically). See `rs/store/tests/event_store_conformance.rs` for the 47 helpers and `docs/research/mongo-analytics-parity-plan.md` §1.6 for the full model.

The principle: **let each backend be itself.** Forcing identical internal shapes is solving the wrong problem. The CHOICE between backends is a real product feature — different deployment shapes legitimately want different storage primitives. The conformance suite's job is to surface contract violations across backends, not to homogenize the implementations behind one canonical shape.

How to apply it:
- When you're tempted to write a tolerance, a fudge factor, or a "skip if backend == X" branch in a conformance test, stop. Ask: what is the actual question this query answers, and is the field I'm trying to compare load-bearing for the answer or cosmetic?
- If cosmetic, redesign the API to remove the cosmetic field (Category 3). The test goes back to strict equality on the structural data.
- If load-bearing, the divergence is a real bug in one of the backends. Fix the bug, don't mask it.
- Tag every conformance helper with its category (C1/C2/C3) in the docstring. Future readers see which parity pattern is in effect at the assertion site.

### Claim-vs-reality checks

When a fact is stated in two places — once as a **claim** in prose (docs, README, CLAUDE.md, comments) and once as a **reality** in code (`Cargo.toml` workspace members, source files, the filesystem, struct fields) — they can drift. The drift is invisible because each side is internally consistent: every doc agrees with every other doc, every line of code agrees with every other line. But neither side knows about the other. Mechanical checks that compare the two sides surface the drift before it metastasizes.

Two examples in this codebase:

- **`scripts/sessionstory.py`** exposed a vocabulary collision between two existing analysis scripts: one counted "turn" as `system.turn.complete` events (63 in a sample session), the other counted "turn" as user-prompt windows (155 in the same session). Both scripts called the variable "Turn N". Reading either script in isolation gave you a coherent picture. Comparing them surfaced the lie. Filed in BACKLOG as "Turn Vocabulary Collision".
- **`scripts/check_docs.py`** caught that four docs (`README.md`, `CLAUDE.md`, `architecture-tour.md`, `soul/architecture.md`) all claimed "9 crates" while `rs/Cargo.toml`'s workspace `members` array had 8. The orphaned 9th crate (`rs/semantic/`) existed on disk with its own `Cargo.toml` but was never wired into the workspace. Every doc was internally consistent with every other doc. None of them were consistent with the build. Filed in BACKLOG as "Remove Orphaned Semantic Crate".

The principle: **OpenStory exists because the agent's internal narrative isn't the same as what the agent actually does.** Applied inward, the same principle says: the project's internal narrative isn't the same as what the project actually is. Internal consistency is not truth. Mechanical comparison against the source-of-truth side (the code, the build, the filesystem) is the only thing that catches it.

How to apply it:

- When you write a check that compares a claim to reality, save it as a script: `scripts/check_docs.py`, `scripts/check_api.py` (proposed), `scripts/check_config.py` (proposed). Each is small, pure-ish, has a `--test` flag with synthetic fixtures, and is independent of the others.
- Run the checks before committing docs, and ideally in CI as a gate against future drift.
- When you find a new claim/reality pair worth checking, add a new check function to the relevant script (or write a new sibling script). Don't merge them into a unified `check.py` until you have at least three reasons to.

The shape is the same shape as the rest of the codebase: pure functions, side effects at the edges, `--test` flag, dataclasses for output, no clever abstractions. The validators are scripts, not frameworks.

---

## Anti-patterns to avoid

### Burying cross-system divergence behind once-a-year code paths

When two systems disagree at a calendar boundary — say, SQLite's `%W` and MongoDB's `$isoWeek` produce different week labels for dates near Dec 28–Jan 3 — the tempting fix is to write a fixture seeder that shifts dates to dodge the boundary. **Don't.** That code path runs once a year and can't be exercised the other 358 days. When it eventually breaks, no one will know why. The divergence is hidden behind a calendar guard that's invisible to anyone reading the test.

Three honest fixes, in order of preference:

1. **Structural assertion at the test site.** Make the test assert on the *shape* of the result ("all rows have the same week label, whatever it is") instead of the specific calendar string. The looseness is now visible at the assertion site, where any future reader learns the contract.
2. **Fix the underlying divergence.** If both systems can be made to agree (e.g., switch SQLite from `%W` to `%V` so both use ISO weeks), do that. One-line behavior change beats a recurring calendar workaround.
3. **Redesign the API to remove the cosmetic field** — see "Semantic parity, not byte parity" above. If the divergent field was doing too much work (bucket identifier AND display label), drop it and expose the structural data instead. This is the fix the MongoDB sink work landed for `tool_evolution` (replaced `week: String` with `bucket_start: String` / `bucket_end: String`).

What to avoid: code paths gated on the current calendar month/day. They're untestable, undiscoverable, and they convert "the test passes" into "the test passes when run on certain days." Time-dependent test flakes are debugging hell.

The general principle: **make divergences visible, don't paper over them.** The conformance suite's job is to surface differences so you can choose how to resolve them, not to silently mask the ones that happen rarely. This pattern was caught by Max during planning the MongoStore analytics parity work — see `docs/research/mongo-analytics-parity-plan.md` §6.5 and §6.8 for the specific divergences this principle was named to prevent.

### Building before looking at the data

We built a tree abstraction, then discovered the data is a linked list. Query real data first. Write a script, print the shape, understand the distribution. Ten minutes of analysis saves hours of wrong implementation.

### Merging data from different sources into one view

WebSocket data is live and ephemeral. REST data is durable and complete. Merging them creates a view that's partially both and fully neither. Sessions appear but have no events. Formats mismatch. Keep views honest about their data source.

### Premature abstractions

We wrote a lazy-loading list abstraction for sessions with 500-2000 records. The data fits in memory, renders in milliseconds. The abstraction solved a problem that doesn't exist. When in doubt, skip the abstraction — three clear lines beat a clever helper.

### Truncation without measurement

A 2KB truncation threshold was set without measuring payloads. Analysis showed it affected 3% of records and saved less than 1MB total. The truncation metadata sometimes made payloads larger. Measure first, then decide.

### Assuming structure from field names

A field called `parent_uuid` suggests a tree. In practice, it creates a sequential chain where each event points to the previous one. Check the actual data shape — depth distributions, branching factors, real examples.

### Mutating raw or normalizing agent-specific fields

When integrating pi-mono, we initially mutated the `raw` field in the translator — renaming `toolCall` → `tool_use`, restructuring `toolResult` content blocks, normalizing `input` → `input_tokens` — so the views layer could parse it without changes. This violated two principles at once.

**Functional purity:** The translator is supposed to be a pure function — data in, CloudEvent out. Mutating `raw` introduced a hidden side effect: the output no longer contained the input data. The translator was secretly rewriting history. A pure translator extracts and transforms; it doesn't alter the source.

**Data sovereignty:** `raw` is the user's data. It's what the agent actually wrote. Reshaping it to fit a different agent's conventions destroys the original signal. If pi-mono says `toolCall`, that's what `raw` should contain. The data is yours, in open formats — that means the *actual* format, not a translation of it.

The fix was structural: add an `agent` discriminator field to CloudEvents (`"claude-code"`, `"pi-mono"`), let each translator preserve native field names, and move format-awareness to the views layer where it belongs. The views layer is the place that understands how to *render* different formats — it's not a pure function, it's a transform that produces UI-ready records, and it's the right place to branch on agent type.

### Inline data analysis

Running ad-hoc Python one-liners in the shell produces results that vanish, can't be reviewed, and break on Windows. Write scripts with test flags, argparse, and clear output. Scripts are artifacts — they tell the story of how you learned what you know.

### Locking down SSH before verifying alternative access

A deploy script disabled root SSH login and password auth before confirming the deploy user could SSH in with keys. Result: locked out of the server entirely, requiring Hetzner web console rescue. Never automate SSH lockdown — do it manually as the last step after verifying key-based access works.

### Docker inode exhaustion

Docker builds (especially Node.js projects with large `node_modules`) create millions of tiny files that exhaust filesystem inodes. A runaway OpenClaw container created 9.1 million files in a Docker volume, consuming every inode on a 150GB disk that showed 82GB free. `df -h` looks fine but `df -i` shows 100% usage. Always check inodes when "no space left on device" doesn't match disk usage. Fix with `docker system prune -a --force` and `docker volume rm` for corrupted volumes.

### Wrong entrypoint in compose overrides

The compose file ran `node dist/index.js` but the actual OpenClaw entrypoint is `node openclaw.mjs`. The result: 100% CPU spin, no logs, no port listening, healthcheck failures — a completely silent failure. When overriding a Dockerfile's CMD in compose, verify the entrypoint matches what the Dockerfile actually uses.

### Heredocs over SSH

Copy-pasting heredoc commands (`cat > file << 'EOF'`) over SSH consistently produces corrupted files — `EOF` markers included as content, leading spaces, line wrapping breaking values. Use `nano` for interactive editing or `printf` for non-interactive file creation on remote servers.
