# MongoStore Analytics Parity — Test/Problem Space

> **Status:** declarative plan, pre-implementation. The Phase 5 follow-up to
> the MongoDB sink work that shipped on `feat/mongodb-sink` (commits c7d5e0a → 0e927c6).
> No code in this document — only the *shape* of the test problem,
> grouped so the work can be sequenced TDD-style.
>
> **Reference:** the SQL implementations live in `rs/store/src/queries.rs`.
> The MongoStore stub leaves all 12 query methods at the trait's empty
> default impls (`Vec::new()` / `None`). The conformance suite at
> `rs/store/tests/event_store_conformance.rs` currently has 30 BDD helpers
> covering writes/reads/lifecycle/FTS but **zero** analytics coverage —
> that's the gap this plan fills.

---

## 1. Problem statement, declaratively

> **Given** the same logical event store contents (sessions, events, projections),
> **when** an analytics query is run against either `SqliteStore` or `MongoStore`,
> **then** both backends answer the same *question* in their own native
> idiom, and the conformance suite enforces the answer — not the
> implementation.

This is **semantic parity, not byte parity.** SQLite and MongoDB are
different tools with different strengths (see §1.6). Forcing one
backend to mimic the other's internal shape is the wrong abstraction.
Instead, each backend implements each query using whatever primitive is
natural for it — `json_extract` and `strftime` on SQLite, dotted-path
access and `$dateFromString` on Mongo — and the conformance suite
verifies that both produce the same answer to the user's question.

"Same answer" is defined per query, in one of three patterns. See §1.6.

This document defines:

- **what data** each query consumes (the input contract)
- **what shape** each query returns (the output contract)
- **what SQL idiom** each one uses, and which Mongo aggregation primitive
  is the equivalent
- **what divergence risks** each one carries — BSON type fidelity,
  ordering tie-breaks, time-window edge cases, JSON-path extraction shape
- **what shared fixture** can exercise all 12 from one seed
- **what order** the implementation TDD walk should take

The point of writing this down before any code is the same point as the
existing `event_store_conformance.rs` file: **the test contract is the
spec**. When MongoStore implementations land in Phase 5, they'll be
walked one query at a time against assertions defined here.

---

## 1.5. Source data format (verified, not assumed)

Before risk analysis or fixture design, we look at what the data
actually is. Three sources of truth, all in agreement:

1. The on-disk JSONL Claude Code emits at
   `~/.claude/projects/<project>/<session>.jsonl`
2. The translator at `rs/core/src/translate.rs:473` and
   `rs/core/src/translate_pi.rs:330`
3. The live event store across 22 sessions / ~10K events

### The format

```text
YYYY-MM-DDTHH:MM:SS.sssZ
```

- **ISO 8601** with **millisecond precision**
- **Always `Z` suffix** (UTC, never a numeric offset like `+05:00`)
- **Zero-padded fixed width** — exactly 24 characters
- Example: `2026-04-07T12:44:03.304Z`

### Where the format comes from

Both Claude Code and pi-mono emit a top-level `timestamp` field on
every JSONL line that has a temporal meaning (`user`, `assistant`,
`system`, `progress`, `queue-operation`, `file-history-snapshot`,
`attachment`). Records with no temporal meaning (`permission-mode`)
have no `timestamp` at all.

The translators do **pure pass-through**, no parse, no normalize:

```rust
// translate.rs:473 (Claude Code)
let timestamp = line.get("timestamp")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());

// translate_pi.rs:330 (pi-mono) — identical
let timestamp = line.get("timestamp")
    .and_then(|v| v.as_str())
    .map(|s| s.to_string());
```

The string in the JSONL becomes the string in `CloudEvent::time` —
`Option<String>` because some records don't have one.

### Where the format ends up

At the EventStore boundary:
- **SqliteStore**: stored as TEXT in `events.timestamp`. Empty string
  for records without a source `timestamp`.
- **MongoStore**: stored as BSON String at the top-level `timestamp`
  field of the event document via `event_to_doc`. Same empty-string
  fallback.

Both use the **same `unwrap_or_default()` path**, so the format
invariant survives all the way through.

Sessions-table dates (`first_event`, `last_event`) are computed by
the projection as `min`/`max` over the events' `time` field. They
inherit the exact same format because they ARE the same strings.

### Lexical order ⇒ chronological order (the load-bearing fact)

Because the format is fixed-width and zero-padded with a `Z` suffix:

- `WHERE timestamp >= cutoff` works as a string compare
- `ORDER BY timestamp DESC` works as a string compare
- `MAX(timestamp)` returns the chronologically latest value
- This is **why** the queries don't need typed `DateTime` columns —
  the format invariant makes string compare safe

This is the same property that lets BSON String fields stand in for
BSON Date fields without correctness loss in any of the analytics
queries. **No timestamp is ever parsed back into a `DateTime`** at
the storage layer in either backend; only the SQL `strftime` and the
Rust `chrono::DateTime::parse_from_rfc3339` (used to compute
`duration_secs` in `synopsis`) ever touch it as a typed value.

### Live store evidence

Across the 22 sessions in the running dev store:

- 22/22 sessions have `start_time` matching the shape
- 1299/1299 events in the current session have non-empty `time`
  matching the shape (when sampled with a regex over `^\d{4}-\d{2}-\d{2}`)
- 5 randomly-sampled sessions across 6,878 events: every single
  timestamp matches
- 0 timestamps with non-`Z` suffix
- 0 timestamps with non-millisecond precision
- 0 timestamps with non-zero-padded fields

### Implication for this plan

**Three risks in §6 below are eliminated by this invariant:**
- §6.1 (BSON type fidelity for date round-trips) — the field is
  always BSON String, never BSON Date, so no round-trip drift can
  occur
- §6.2 (UTC vs local hour extraction) — source is always UTC, both
  backends extract UTC hour
- The "construct fixture timestamps in UTC" instruction in §7.4 is
  no longer a discipline reminder — it falls out of producing
  timestamps via `chrono::Utc::now().format(TS_FORMAT)` with the
  same format the translator pass-through produces

**One real bug surfaces** while reading the SQL: see §6.8 (the
cutoff format gotcha).

The fixture seeder format constant (used everywhere we construct a
fixture timestamp):

```rust
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";
```

This format is byte-identical to what the translator emits. **Every
fixture timestamp in this plan is constructed via this constant.**
Never use `chrono::DateTime::to_rfc3339()` in fixtures or in cutoff
construction — see §6.8 for why.

---

## 1.6. The three-category parity model

After implementing both backends in Phases 0–9, it became clear that
"force every analytics query to produce byte-identical results" is the
wrong frame. SQLite and MongoDB are different tools (see §1.7); some
queries have answers that are mathematically defined and identical,
some have answers that are well-defined as a *set* but ambiguous in
*tie order*, and at least one query has an API that bakes a cosmetic
divergence into its return type. Each category needs a different
conformance pattern.

### Category 1 — Strict equality (`assert_eq!`)

The query has an answer that is mathematically defined. Both backends
must produce byte-identical output. Examples: `tool_count` is a
`COUNT(*)`; `total_input_tokens` is a `SUM`; `session_events` is an
ordered list with a well-defined sort key (timestamp ASC). For these,
the conformance helper does:

```rust
assert_eq!(sqlite_result, mongo_result);
```

If they don't match, one backend has a real bug. No tolerance, no
slack.

### Category 2 — Canonical-sort then equality

The query has an answer whose *set membership* is well-defined but
whose *order within tied groups* is implementation-defined. Examples:
`top_tools` is "top N tools by count" — within tied counts, the order
is unspecified by the query semantics. SQLite's `ORDER BY count DESC`
and Mongo's `$sort: { count: -1 }` are both within their rights to
return either order for ties.

For these, the conformance helper sorts both outputs by a canonical
secondary key (typically alphabetical on a stable string field) before
comparing:

```rust
let mut sqlite_canonical = sqlite_result.clone();
let mut mongo_canonical  = mongo_result.clone();
sqlite_canonical.sort_by(|a, b| (a.count, &a.tool).cmp(&(b.count, &b.tool)));
mongo_canonical .sort_by(|a, b| (a.count, &a.tool).cmp(&(b.count, &b.tool)));
assert_eq!(sqlite_canonical, mongo_canonical);
```

The looseness is **encoded at the assertion site**, where any future
reader of the test sees exactly what's tolerated and why. Not buried
in fixture data, not hidden in a guard.

**Three queries fall in this category:** `top_tools` (helper inside
`synopsis`), `session_efficiency` (NULL `last_event` ordering),
`recent_files` (same-timestamp ties).

### Category 3 — Redesign the API to remove the cosmetic field

When the divergence between backends is in a *cosmetic* field that's
doing too much work (both bucket identifier and display label), the
right fix is to redesign the API. Replace the cosmetic field with the
underlying structural data and let the UI handle formatting.

**One query falls in this category: `tool_evolution`.** Currently
returns `Vec<{ week: String, tool: String, count: u64 }>`. The `week`
field is both a bucket identifier (used for grouping) and a display
label (rendered by the dashboard). SQLite produces `2026-W14` via
`strftime('%Y-W%W')`; Mongo would produce `2026-W14` via `$isoWeek`,
and the two formats disagree at year boundaries.

The fix: change the return type to expose the bucket boundaries
directly:

```rust
pub struct ToolEvolution {
    pub bucket_start: String,  // ISO 8601 date — Monday of the week
    pub bucket_end:   String,  // ISO 8601 date — Sunday
    pub tool: String,
    pub count: u64,
}
```

Both backends now compute the same `bucket_start` (the Monday of the
week containing the event timestamp) regardless of how they label
weeks. Byte equality emerges naturally; the UI formats `bucket_start`
into whatever week label it wants. **Small breaking change to the
SQLite output, but the only consumer is the dashboard which renders
opaquely — ~10 lines of TypeScript on the frontend side.**

### What the categories let us drop

Several "risks" from the original §6 evaporate under this model:

- **§6.5 (week-boundary edge cases):** category 3 — solved by API
  redesign. No fixture seeder dance required.
- **§6.4 (time-relative `now()` calls):** category 1 — fixture
  timestamps relative to `Utc::now()` work fine because both backends
  compute the same hour buckets from the same input. Clock injection
  remains in BACKLOG as a separate concern.
- **§6.3 (ORDER BY tie instability):** category 2 — encoded at the
  assertion site, not the fixture site. Fixtures don't need to use
  artificially distinct counts.

---

## 1.7. SQLite vs MongoDB — pros and cons (post-implementation)

After building both backends, here's what each is genuinely good and
bad at for OpenStory's actual workload. This frames the §1.6 model:
the backends are different tools, the conformance suite respects that.

### SQLite — strengths

| Strength | Why it matters here |
|---|---|
| **Single file, zero deps** | `data/open-story.db` is the data. `cp` it, `sqlite3` it, email it. The "your data is yours" promise from `docs/soul/philosophy.md` is one filesystem operation away. |
| **In-process — no network round trip** | Persist consumer hot path is microseconds. Mongo over localhost is milliseconds; over network is more. For high-burst sessions (1000+ events) the difference is real. |
| **`json_extract` + `strftime` are built in** | Date bucketing and JSON path queries are one function call. No `$dateFromString` round-trip, no aggregation pipeline ceremony. |
| **FTS5 with porter + `snippet()`** | Mature, snappy, ranked, with server-side highlighted snippets. Mongo `$text` doesn't have a snippet primitive — we fall back to a 120-char truncation in MongoStore. |
| **ACID across `delete_session`** | One transaction wraps 5 table deletes atomically. Mongo's equivalent does 6 collection deletes that are NOT atomic without an explicit multi-document transaction (which requires a replica set). On Mongo, a process death mid-delete leaves the session partially deleted. **Real durability difference.** |
| **Sovereignty story is concrete at the file level** | The user can move the file. The user can `sqlite3 .dump` it. The user can email it. There's no daemon to mediate the relationship. |

### SQLite — weaknesses

| Weakness | Evidence |
|---|---|
| **Single-host** | Only one process can write at a time. Multi-host deployment isn't possible without external replication (Litestream is one option but it's stream-based, not multi-writer). |
| **`payload LIKE '%foo%'` is a sequential scan over TEXT** | Currently fast at session scale. Slow at 1M+ events. The optimizer can't index a substring. |
| **`json_extract` parses the entire payload per row** | Same — fine now, slow at scale. There's no way to put a covering index on a JSON path. |
| **`WHERE x IN (?1,?2,?3)` placeholder ceiling** | SQLite's default limit is 999 placeholders. `token_usage` builds a placeholder list per session ID and could hit that ceiling on large stores. |
| **Lexical-string-compare regime** | The §6.8 cutoff bug is a symptom: ASCII collation is the source of truth for date comparisons. Safe today, fragile in principle. Doesn't apply to Mongo. |

### MongoDB — strengths

| Strength | Why it matters here |
|---|---|
| **Nested field access is structurally indexed** | `payload.data.raw.message.usage.input_tokens` is a B-tree path lookup, not a JSON parse. `$exists: true` is precise where SQL's `LIKE '%input_tokens%'` is a substring scan with false positives. |
| **`$dateFromString` + `$hour`/`$isoWeek` are well-defined** | No `strftime` quirks, no platform-specific behavior, no integer-cast dance. The aggregation framework was built for date bucketing. |
| **Native horizontal scale** | Replica sets, sharding, change streams. Production tooling out of the box. The whole reason Mongo got added. |
| **Schema-free evolution** | Adding a new field to `CloudEvent` (like `tool_outcome`) doesn't need an `ALTER TABLE` migration. |
| **Aggregation pipeline composes** | `$lookup`, `$facet`, `$group` with multiple aggregators, `$accumulator` for custom logic. What `recent_files` and `tool_evolution` need natively. |
| **Cursor streaming** | For huge result sets like `daily_token_usage` over a year, Mongo can stream rows from the server. SQLite returns the whole `Vec` at once. |

### MongoDB — weaknesses

| Weakness | Evidence |
|---|---|
| **Network round trips** | Every query is a TCP call, even on localhost. The conformance suite spins up a container per test (~1s warm start). The dev loop is heavier. |
| **Driver size** | `mongodb` + `bson` add ~10MB to the binary. Store crate compile time roughly doubles with `--features mongo`. |
| **BSON has its own type system** | `i32` vs `i64`, `Date` vs `String`, document path semantics. We sidestep most of this by storing the whole CloudEvent at `payload`, but the day someone wants a BSON Date column, they'll hit a snag. |
| **Aggregation pipeline syntax is verbose** | A Mongo equivalent of `tool_evolution`'s SQL is 4-5 stages and ~30 lines of doc-builder Rust. The SQL is 8 lines. |
| **Server-flavored tooling** | `mongosh` works, but `sqlite3` works without a server. |
| **Operational overhead** | Daemon, port, auth, container management. Even in `just up` there's a `docker run` step that can fail in ways `cp` can't. |
| **Not actually faster at small scale** | Both backends return a 10K-event session in ~10ms. Mongo isn't winning until the cluster matters. |

### When each backend is the right choice

| Use case | Pick |
|---|---|
| Local single-user, "just run it" | **SQLite** |
| Personal sovereignty / "the data is mine on disk" | **SQLite** |
| High-throughput persist consumer (no network) | **SQLite** |
| Want zero ops overhead | **SQLite** |
| Want server-side FTS snippets | **SQLite** |
| Want ACID across multi-collection operations | **SQLite** |
| Multi-host deployment | **MongoDB** |
| Multiple writer processes | **MongoDB** |
| Want to query/index nested fields without functional indexes | **MongoDB** |
| Want `$lookup` joins, change streams, replica sets | **MongoDB** |
| Already have Mongo expertise / Atlas integration | **MongoDB** |
| Schema evolution without migrations | **MongoDB** |

The **CHOICE** between backends is a real product feature. Phase 5
makes both genuinely usable for analytics; the user picks based on
their actual deployment shape, not on which one has nicer syntax for
any given query.

---

## 2. Inventory of the 12 queries

Each row is one method on the `EventStore` trait. SQL impls live in
`rs/store/src/queries.rs`. Output structs all derive `Serialize` and live
in the same file.

The **Parity** column refers to the §1.6 categories:
- **C1** = strict equality (`assert_eq!`)
- **C2** = canonical-sort then equality (tie order is implementation-defined)
- **C3** = API redesign required to remove cosmetic divergence

| # | Method | Input | Output | Parity | Hardness |
|---|--------|-------|--------|--------|----------|
| 1 | `query_session_synopsis(session_id)` | `&str` | `Option<SessionSynopsis>` | C1 (counts) + C2 (`top_tools` ties) | **medium** |
| 2 | `query_tool_journey(session_id)` | `&str` | `Vec<ToolStep>` | C1 (timestamp ASC is well-defined) | **medium** |
| 3 | `query_file_impact(session_id)` | `&str` | `Vec<FileImpact>` | C1 (Rust-side post-sort is deterministic) | **medium** |
| 4 | `query_session_errors(session_id)` | `&str` | `Vec<SessionError>` | C1 | **easy** |
| 5 | `query_project_pulse(days)` | `u32` | `Vec<ProjectPulse>` | C1 (counts and sums) | **easy** |
| 6 | `query_tool_evolution(days)` | `u32` | `Vec<ToolEvolution>` ⚠️ **redesigned** | **C3** + C1 (counts after redesign) | **hard** (date bucketing) |
| 7 | `query_session_efficiency()` | `()` | `Vec<SessionEfficiency>` | C2 (NULL `last_event` ordering) | **medium** |
| 8 | `query_project_context(project_id, limit)` | `&str, usize` | `Vec<ProjectSession>` | C1 (well-ordered by `last_event DESC` if non-null) | **easy** |
| 9 | `query_recent_files(project_id, session_limit)` | `&str, usize` | `Vec<String>` | C2 (same-timestamp ties) | **hard** (cross-collection join) |
| 10 | `query_productivity_by_hour(days)` | `u32` | `Vec<HourlyActivity>` | C1 (UTC hour buckets are well-defined) | **medium** (hour extraction) |
| 11 | `query_token_usage(days, session_id, model)` | `Option<u32>, Option<&str>, &str` | `TokenUsageSummary` | C1 (sums + Rust-side cost calc) | **hardest** |
| 12 | `query_daily_token_usage(days)` | `Option<u32>` | `Vec<DailyTokenUsage>` | C1 (date prefix is well-defined) | **hard** |

**⚠️ ToolEvolution API change (Category 3 — see §1.6):**

```rust
// BEFORE (current SQLite output, baked-in week label)
pub struct ToolEvolution {
    pub week: String,    // "2026-W14" — disagrees between %W and %V at year boundaries
    pub tool: String,
    pub count: u64,
}

// AFTER (Phase 5)
pub struct ToolEvolution {
    pub bucket_start: String,  // ISO 8601 date — Monday of the week, e.g. "2026-04-06"
    pub bucket_end:   String,  // ISO 8601 date — Sunday,                e.g. "2026-04-12"
    pub tool: String,
    pub count: u64,
}
```

Both backends compute the same `bucket_start` (the Monday of the week
containing the event timestamp) regardless of how they label weeks
internally. The dashboard formats `bucket_start` into whatever week
label it wants. **Small breaking change to the SQLite output, but the
only consumer is `/api/insights/tool-evolution` rendered opaquely by
the dashboard — ~10 lines of TypeScript on the frontend.**

**Cost breakdown:**
- 4 easy queries: #4, #5, #8, plus #1's metadata path
- 5 medium queries: #1, #2, #3, #7, #10
- 3 hard queries: #6, #9, #11, #12

**Parity breakdown:**
- 8 queries are pure C1 (strict equality)
- 3 queries are C2 (canonical-sort): #1's `top_tools`, #7, #9
- 1 query is C3 (API redesign): #6

---

## 3. Input shape taxonomy

Every query falls into one of four input shapes. Group the conformance
seed data so each shape gets exercised cleanly.

### Shape A: Per-session
**Methods:** `synopsis`, `tool_journey`, `file_impact`, `session_errors`

These take a `session_id` and scope all reads to that session. The
fixture needs at least **two sessions** so we can verify cross-session
isolation (querying session A doesn't pull events from session B).

### Shape B: Time-windowed cross-session
**Methods:** `project_pulse(days)`, `tool_evolution(days)`,
`productivity_by_hour(days)`, `token_usage(days, _, _)`,
`daily_token_usage(days)`

These compute `cutoff = now() - days` internally and filter on
`timestamp >= cutoff`. The fixture timestamps must be **recent enough**
that any reasonable test `days` value (say `days = 365`) includes them.
Concretely: timestamps within the last 30 days of wall clock.

This is the source of the biggest risk in the suite — see §6 "Time
relativity".

### Shape C: Project-scoped
**Methods:** `project_context(project_id, limit)`, `recent_files(project_id, session_limit)`

These take a `project_id` and scope to sessions belonging to that
project. The fixture needs **two projects** so we can verify scoping —
querying `proj-alpha` doesn't return sessions from `proj-beta`.

### Shape D: Global / no input
**Methods:** `session_efficiency()`, `token_usage(None, None, _)`

Scans all sessions, ordered. No filtering. The fixture's total session
count is the input.

---

## 4. Output shape taxonomy

### Output A: `Option<Struct>` — single row or None
- `synopsis` only

**Conformance assertion:** both backends return `Some` with structurally
equal struct, OR both return `None`. Easy.

### Output B: `Vec<Struct>` — ordered list
- `tool_journey`, `file_impact`, `session_errors`, `project_pulse`,
  `tool_evolution`, `session_efficiency`, `project_context`,
  `productivity_by_hour`, `daily_token_usage`

**Conformance assertion:** `Vec::len()` equal AND every position equal.
**Risk:** ordering ties (see §6.3). Fixtures must use distinct sort keys
to make the order deterministic.

### Output C: `Vec<String>` — ordered list of file paths
- `recent_files`

Same as Output B, but the elements are bare strings. Same tie risk.

### Output D: `TokenUsageSummary` — nested struct with `Vec<SessionTokenUsage>`
- `token_usage`

The complex one. Top-level fields: `session_count`, `usage`, `cost`,
`sessions`. The `sessions` Vec is ordered by `output_tokens DESC`. The
`cost` field is computed from `usage` via `estimate_cost(model)` —
**both backends must use the exact same `estimate_cost` function** so
the cost field is byte-identical.

**Conformance assertion:** structural equality of the whole struct, with
the inner `sessions` Vec asserted position-by-position.

---

## 5. SQL → Mongo translation cheat sheet

This is the per-idiom mapping the implementation phase will reach for.
Documented up front so the conformance test author knows what semantic
divergences to write tolerances for.

| SQL idiom | Mongo equivalent | Notes |
|-----------|------------------|-------|
| `WHERE session_id = ?` | `$match: {session_id: ...}` | already used in MongoStore — see existing reads |
| `WHERE subtype = 'X'` | `$match: {subtype: 'X'}` | extracted at write time, indexed |
| `WHERE timestamp >= ?` | `$match: {timestamp: {$gte: ...}}` | works on RFC3339 strings (lexicographic == chronological for zero-padded ISO 8601) |
| `COUNT(*) GROUP BY x` | `$group: {_id: '$x', count: {$sum: 1}}` | straightforward |
| `ORDER BY x DESC LIMIT N` | `$sort: {x: -1}, $limit: N` | |
| `json_extract(payload, '$.data.agent_payload.tool')` | `$payload.data.agent_payload.tool` (dotted path in `$match`/`$project`) | the full original CloudEvent is stored under the `payload` field by `event_to_doc`, so the projection path is `payload.data.agent_payload.tool` |
| `COALESCE(a, b, c, d)` | `{$ifNull: [a, {$ifNull: [b, ...]}]}` nested | verbose but mechanical |
| `strftime('%Y-W%W', ts)` | `{$dateToString: {format: '%G-W%V', date: {$dateFromString: {dateString: '$timestamp'}}}}` | **divergence risk:** SQLite `%W` is week-of-year starting Sunday; Mongo `%V` is ISO 8601 week. `%G` is ISO 8601 week-numbering year. They agree on most days but differ at year boundaries — fixture must avoid Dec 28-Jan 3 dates |
| `CAST(strftime('%H', ts) AS INTEGER)` | `{$hour: {$dateFromString: {dateString: '$timestamp'}}}` | SQLite's `%H` returns local-time hour by default for `now()` but UTC hour for stored RFC3339; Mongo's `$hour` is always UTC. **All fixture timestamps must be in UTC** (RFC3339 with `Z` suffix) to match — this is already true for production CloudEvents |
| `payload LIKE '%input_tokens%'` | `$match: {'payload.data.raw.message.usage.input_tokens': {$exists: true}}` | Mongo can do better than the SQLite substring scan because the field is structured |
| events JOIN sessions | `$lookup: {from: 'sessions', localField: 'session_id', foreignField: '_id', as: 'session'}` then `$unwind` | only used by `recent_files`. Simpler alternative: do a `find` on sessions first to get the list of session_ids, then a `find` on events filtered by `$in` |

---

## 6. Risk catalog (the things that will bite during implementation)

### 6.1 BSON type fidelity for nested extraction (~~partially eliminated~~)

> **Update after data inspection:** the date-fidelity half of this risk
> is eliminated by the §1.5 invariant. The `events.timestamp` field is
> always a BSON String, the source is fixed-format ISO 8601, and no
> code path ever stores it as a BSON Date. **What remains** is the
> integer-width concern for `$group $sum` results — see below.

**Remaining risk:** when `bson::to_bson` serializes a
`serde_json::Value`, integer width is preserved (`i32` stays `i32`,
`i64` stays `i64`). When the analytics query reads
`payload.data.raw.message.usage.input_tokens` back, it might come back
as `i32` or `i64` depending on the original JSON's representation. If
we then `as_u64()` it, we're fine — but if the Mongo `$group $sum`
operates on mixed int widths, the BSON result might be `i64` where
SQLite returned `u64`. The conformance assertion needs to compare on
the typed Rust struct (which is `u64`) **after** deserialization, not
on raw BSON.

**Mitigation:** the conformance helpers compare typed structs, not raw
documents. This risk is structural, not behavioral.

**Test that catches it:** any query that returns `{count: u64}` after a
`$group $sum`. Specifically `tool_count` and `error_count` in
`SessionSynopsis`, the `count` field in `ToolStep`/`ToolEvolution`, and
all the `*_tokens` fields in `TokenUsage`.

**Future-proofing:** add one focused conformance test
`it_stores_timestamp_as_bson_string` that asserts the field type via
`db.events.findOne().timestamp` introspection. Catches a future
migration that converts the field to BSON Date and silently breaks
every analytics query that does string compare on it.

### 6.2 ~~Time-of-day extraction (UTC vs local)~~ — ELIMINATED

> **Update after data inspection:** §1.5 verified all source timestamps
> are UTC with `Z` suffix, not numeric offsets. SQLite's `strftime('%H')`
> and Mongo's `$dateFromString → $hour` both interpret `Z` as UTC and
> return identical hour buckets. The "wrong on the day someone seeds
> from their local clock" failure mode requires constructing fixture
> timestamps with `chrono::Local`, which is impossible if §1.5's
> `TS_FORMAT` constant is used (it forces `Z` suffix).
>
> **No fixture invariant needed** — the format constant enforces it.

### 6.3 ORDER BY tie instability — handled by §1.6 Category 2

Several queries `ORDER BY count DESC` where ties are possible. SQLite's
behavior on ties is implementation-defined (typically insertion order
within the group). Mongo's `$sort` on ties is also implementation-
defined.

This is **not a fixture problem** — it's a contract problem. The
queries themselves don't promise an order within ties, and forcing one
backend to mimic the other's tie behavior would be solving the wrong
problem.

**Resolution under §1.6:** the three queries with possible ties
(`top_tools` inside synopsis, `session_efficiency`, `recent_files`)
are Category 2. The conformance helper sorts both outputs by a
canonical secondary key before comparing. The looseness is encoded at
the assertion site, not the fixture site. Fixtures can have ties; the
canonical sort handles them deterministically.

**No fixture invariant required.** Fixtures can use repeated counts,
identical timestamps, whatever. The Category 2 assertion handles it.

### 6.4 Time-relative `now()` calls

`project_pulse`, `tool_evolution`, `productivity_by_hour`, the `days`
variants of `token_usage`, and `daily_token_usage` all call
`chrono::Utc::now()` internally to compute their cutoff. They are
**not** pure functions of their inputs — they depend on wall clock.

**Why this is fine under §1.6:** the time-relative `now()` call is
applied identically by both backends at the same instant during the
test, so both see the same cutoff, both filter on the same events, and
both produce the same answer. The flakiness concern is not "the two
backends disagree" — it's "the output drifts between test runs as the
wall clock advances." That's a cross-time concern, not a cross-backend
concern.

**Resolution:** seed fixtures relative to `Utc::now()` at test time and
call queries with a generous `days` window. Fixture events at
`now() - 24h` are always inside `days = 7`, both backends see them, both
return the same answer. Conformance assertion is byte-equal.

**Why no clock injection in Phase 5:** clock injection (refactoring
queries to take `as_of: DateTime<Utc>`) is the right long-term answer
for full determinism, but it touches every query signature, every API
handler, and the CLI surface. **Filed in BACKLOG as a separate
follow-up** because it doesn't change parity behavior, only test
determinism.

### 6.5 Week-boundary edge cases

`tool_evolution` groups by week using SQLite's `strftime('%Y-W%W', ts)`.
SQLite's `%W` is "week of year, where week 1 is the first week with a
Monday in it" (ISO-ish but not strictly ISO). Mongo's closest equivalent
is `$isoWeek` (or `$dateToString` with `%V` format), which **is** strict
ISO 8601 week.

**Where they disagree:** late December / early January, where ISO weeks
can roll back to the previous year. A fixture event timestamped
`2025-12-30T12:00:00Z` belongs to ISO week `2026-W01` but to SQLite's
`%Y-W%W` it's `2025-W52`.

> **Note after data inspection:** this risk is unchanged by §1.5 — the
> source format is well-defined, but `%W` vs `%V` is a *SQL semantics*
> divergence, not a date format divergence. Both backends parse the
> same `Z`-suffixed string correctly; they disagree on which week
> number to call it.

**Mitigation:** **fixture seeder discipline** — generate timestamps via
`Utc::now() - Duration::hours(N)` and shift backward by 14 days if
`Utc::now()` is in the Dec 28–Jan 3 window. The §1.5 `TS_FORMAT`
constant guarantees the format is right; this guards the *value*.

**Alternative mitigation:** change the SQL to use `strftime('%Y-W%V')`
(ISO week explicit) so both backends agree. **This is a behavioral
change to SQLite output format**, so it needs explicit user buy-in
before doing it. See open question Q1 in §11.

### 6.6 The `payload LIKE '%input_tokens%'` substring scan

`token_usage` and `daily_token_usage` use `payload LIKE '%input_tokens%'`
as a cheap pre-filter before the JSON extraction. This is a SQLite
optimization — substring scan over the TEXT column is faster than
parsing the JSON for every row.

**The Mongo equivalent is more correct, not less.** Mongo can do
`{'payload.data.raw.message.usage.input_tokens': {$exists: true}}`
which is a structural index lookup, not a substring scan.

**Risk:** the SQLite version's substring filter is over-inclusive — it
matches any payload that *mentions* the literal string "input_tokens"
anywhere, including in a tool argument or assistant message text. The
`extract_usage_from_payload` helper then tries to parse the field and
returns `None` if the structure isn't right, so the over-inclusive
filter is harmless in practice. But **the row counts that hit
`extract_usage_from_payload` may differ between backends** — Mongo's
structural filter is exact, SQLite's substring filter has false
positives.

**Mitigation:** the conformance assertion is on the *output* of the
aggregation, not on intermediate row counts. As long as both backends
produce the same `TokenUsage` totals, the difference in pre-filter
selectivity is invisible. **Fixture invariant:** the analytics fixture
must not contain any event that mentions "input_tokens" as a literal
string except in the actual usage field.

### 6.8 The cutoff format gotcha (NEW — discovered while reading the SQL)

**Found while answering "what does the source data format look like?"**
This is a real bug in the existing SQL queries that currently works by
lexical accident, and it needs to be fixed in **both** backends as part
of Phase 5.

**The setup:**

The time-windowed queries (`project_pulse`, `tool_evolution`,
`productivity_by_hour`, `token_usage` with days, `daily_token_usage`)
all compute a cutoff like this in `queries.rs`:

```rust
let cutoff = chrono::Utc::now() - chrono::Duration::days(d as i64);
let cutoff_str = cutoff.to_rfc3339();
// ... WHERE timestamp >= ?1 ... cutoff_str passed as ?1
```

`chrono::DateTime<Utc>::to_rfc3339()` produces:

```text
2026-04-07T12:44:03.304+00:00
```

But the **stored** values produced by the translator (per §1.5) are:

```text
2026-04-07T12:44:03.304Z
```

These represent the same instant but they are **different strings**.
SQLite is comparing them lexicographically, not parsing them as
datetimes.

**Why it currently works (by accident):**

ASCII `Z` (0x5A) sorts after `+` (0x2B). So at the same instant:

```text
"2026-04-07T12:44:03.304Z"      lexically >  "2026-04-07T12:44:03.304+00:00"
```

This means `WHERE timestamp >= cutoff` includes events at the exact
cutoff instant — which is the desired behavior. The lexical accident
gives the right answer.

**Why it's fragile:**

1. The accident only holds because ALL stored values use `Z`. If even
   one event ever lands in the store with a `+00:00` suffix (e.g., a
   future translator migration, or a backfill from an external source),
   the comparison degenerates: `"...304+00:00" >= "...304+00:00"` is
   true, but `"...304Z" >= "...303+00:00"` is also true even though
   the `Z` event is later than the cutoff by 1ms — which happens to be
   the right answer here, but the reasoning has now leaked from
   "compare datetimes" to "rely on ASCII collation in a specific
   suffix regime."
2. The Mongo equivalent of this query inherits the same fragility if
   the cutoff is constructed the same way.
3. It makes the conformance test load-bearing on a property that
   nobody documented and nobody is testing: "the cutoff format and
   the storage format must use the same UTC suffix, OR the suffix
   chars must satisfy `stored_suffix > cutoff_suffix` lexically."

**The fix (one line, both backends):**

Replace `cutoff.to_rfc3339()` with the canonical format that matches
storage:

```rust
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";
let cutoff_str = cutoff.format(TS_FORMAT).to_string();
```

Now stored values and cutoffs are byte-identical at the suffix, and
`>=` becomes a true lexical compare on chronologically meaningful
strings — exactly the property §1.5 promises.

**Where to apply:**

Five call sites in `rs/store/src/queries.rs`:
- `project_pulse` (~line 286)
- `tool_evolution` (~line 327)
- `productivity_by_hour` (~line 516)
- `token_usage` with `days` (~line 634)
- `daily_token_usage` (~line 744)

Plus any equivalent cutoff construction in the Mongo aggregation
implementations of the same five queries (Phase 5 work).

**Test that catches it:**

A targeted conformance helper
`it_includes_events_at_the_exact_cutoff_instant` that:
1. Seeds one event at `Utc::now() - Duration::hours(1)`
2. Calls a time-windowed query with `days = 1`
3. Asserts the event is in the result

This currently passes by accident. After the fix, it passes by
specification. The test stays even after the fix as a regression
guard.

**Status:** filed as a Phase 5 side fix. Lands in the same commit
as the first time-windowed query implementation. Single-line change
× 5 sites. The test is the proof.

> **Note:** under the semantic parity model from §1.6, this fix only
> needs to land on the SQLite path (where the lexical-string-compare
> regime exists). Mongo's implementation can use a real `$gte` against
> a `$dateFromString` and never touch the cutoff format issue at all.
> Same problem, two solutions, one per backend.

### 6.7 The `recent_files` cross-collection JOIN

`recent_files` is the only query that joins events to sessions:
"give me files modified by Edit/Write/NotebookEdit in any session
belonging to project X". The Mongo idiom for this is `$lookup`, but a
simpler approach is two queries: first `find` the sessions in the
project, then `find` the matching events filtered by `session_id $in`.

**Risk:** the SQLite version uses `LIMIT ?2` (where `?2 = session_limit
* 20`) and ORDER BY `e.timestamp DESC`. The "limit" semantic here is
"return up to N distinct files, considering the most recent events
first." Mongo's two-query approach needs to match this — fetch events
in timestamp DESC order, accumulate distinct files until we have N,
return them.

**Mitigation:** the conformance test seeds a known-good fixture with
exactly N+5 distinct files, expects N back, and asserts the N most
recent ones are returned. The Mongo implementation will need a
`$group: {_id: '$payload.data.agent_payload.args.file_path'}` with
`$first: '$timestamp'` to dedupe while preserving order, then a
`$sort + $limit`.

---

## 7. The shared analytics fixture

One fixture seeder, used by all 12 conformance helpers. Defined in
`event_store_conformance.rs` alongside the existing `test_event` /
`test_session_row` / `test_pattern` helpers.

### 7.1 Topology

**Two projects:**
- `proj-alpha` ("Alpha") — 2 sessions
- `proj-beta` ("Beta") — 1 session

**Three sessions:**
- `sess-A1` (project alpha, label "build feature X", 12 events)
- `sess-A2` (project alpha, label "fix auth bug", 8 events)
- `sess-B1` (project beta, label "explore data", 5 events)

**Total: 25 events** spread across the 3 sessions.

### 7.2 Event mix per session

Distribution chosen so that every query has at least one non-empty
output and every aggregation bucket has distinct counts (no ties).

| Session | tool_use | tool_result | text | error | total |
|---------|----------|-------------|------|-------|-------|
| sess-A1 | 5 | 5 | 1 | 1 | 12 |
| sess-A2 | 3 | 3 | 1 | 1 | 8 |
| sess-B1 | 2 | 2 | 1 | 0 | 5 |

Tool distribution within tool_use events (designed to give distinct
counts for `top_tools` and `file_impact`):

- **sess-A1:** Edit ×3, Bash ×1, Read ×1
- **sess-A2:** Read ×2, Bash ×1
- **sess-B1:** Grep ×1, Glob ×1

**Aggregate top tools:** Edit=3, Read=3, Bash=2, Grep=1, Glob=1.
*Note the Edit/Read tie* — see §6.3. Either:
- (a) Bump one Read to a Write to make Edit=3, Read=2, Write=1 (eliminates the tie)
- (b) Make the conformance helper for `top_tools` tie-tolerant

**Recommendation: (a)**, because eliminating ties at fixture level is
simpler than adding tolerance to the assertion.

### 7.3 File impact distribution

Files touched by Edit/Write/NotebookEdit (writes) and Read/Glob/Grep (reads):

- `src/main.rs` — 2 Edits, 1 Read (in sess-A1)
- `src/lib.rs` — 1 Edit, 0 reads (in sess-A1)
- `Cargo.toml` — 0 edits, 1 Read (in sess-A1)
- `tests/auth.rs` — 0 edits, 2 Reads (in sess-A2)
- `data/raw/` — 0 edits, 1 Glob, 1 Grep (in sess-B1)

Distinct counts on `(reads + writes)`: src/main.rs=3, src/lib.rs=1,
Cargo.toml=1, tests/auth.rs=2, data/raw/=2. **Two ties** — needs
fixture adjustment (e.g., add a second Edit on src/lib.rs to make it
src/lib.rs=2, then src/main.rs=3, src/lib.rs=2, tests/auth.rs=2, ...
which still ties). Likely cleanest: bump src/main.rs to 4 and
data/raw/ to 1.

**Action item for the implementer:** finalize the fixture event mix so
every assertion's expected ordering is unambiguous. This is the kind
of thing that's faster to do interactively at the keyboard than to
spec exhaustively here.

### 7.4 Timestamp distribution

All fixture timestamps are computed at seed time as offsets from
`Utc::now()`, using the `TS_FORMAT` constant from §1.5 so the produced
strings are byte-identical to what the translator pass-through emits.

```rust
/// Canonical translator format. See §1.5 for derivation. Never use
/// chrono::DateTime::to_rfc3339() in fixtures or cutoffs — see §6.8.
const TS_FORMAT: &str = "%Y-%m-%dT%H:%M:%S%.3fZ";

/// Returns a fixture timestamp at `hours` hours and `minutes` minutes
/// before now, in the canonical translator format.
fn ts_offset(hours: i64, minutes: i64) -> String {
    (chrono::Utc::now()
        - chrono::Duration::hours(hours)
        - chrono::Duration::minutes(minutes))
        .format(TS_FORMAT)
        .to_string()
}
```

That's it. **No year-boundary guards, no `fixture_anchor`, no
calendar-month branches.** The §1.6 Category 3 redesign of
`ToolEvolution` (replacing `week: String` with `bucket_start`/
`bucket_end`) eliminates the only reason we'd need to dodge week
boundaries — both backends compute the same Monday-of-the-week from
the same input timestamp regardless of how they label weeks.

The fixture distributes events across three sessions like this:

```text
sess-A1 events: ts_offset(24, 0), ts_offset(24, -1), ..., ts_offset(24, -11)
                (12 events, ~24h ago, 1-minute increments)
sess-A2 events: ts_offset(12, 0), ts_offset(12, -1), ..., ts_offset(12, -7)
                (8 events, ~12h ago, 1-minute increments)
sess-B1 events: ts_offset(6, 0),  ts_offset(6, -1),  ..., ts_offset(6, -4)
                (5 events, ~6h ago, 1-minute increments)
```

**Properties this provides:**

| Property | How |
|----------|-----|
| Recent enough that `days = 7` always includes them all | offsets max out at 24h |
| Spread enough that hour-bucketing shows multiple distinct hours | three anchor offsets at 24h, 12h, 6h give three distinct UTC hours |
| All timestamps are UTC `Z`-suffixed | `TS_FORMAT` enforces it |
| All timestamps lexically sortable in chronological order | `TS_FORMAT` is fixed-width zero-padded |
| Distinct timestamps per session | 1-minute increments — no within-session ties |
| Year-boundary safe | not required; ToolEvolution C3 redesign removes the failure mode |
| Tie order in `top_tools` is implementation-defined | C2 canonical sort handles it; fixtures may have ties |

### 7.5 Token usage events

Three `assistant.text` events have nested `data.raw.message.usage`
fields. Distinct token counts so the per-session ordering in
`token_usage.sessions` (sorted by `output_tokens DESC`) is unambiguous.

| Event | Session | input | output | cache_read | cache_creation |
|-------|---------|-------|--------|------------|----------------|
| evt-tok-1 | sess-A1 | 1000 | 500 | 200 | 100 |
| evt-tok-2 | sess-A2 | 800 | 300 | 150 | 50 |
| evt-tok-3 | sess-B1 | 600 | 200 | 100 | 25 |

Aggregate totals: input=2400, output=1000, cache_read=450,
cache_creation=175, total=4025, message_count=3, session_count=3.

Cost (model="sonnet"): input = 2400 × 3.0 / 1e6 = 0.0072,
output = 1000 × 15.0 / 1e6 = 0.015, etc. The conformance assertion
checks the cost field is byte-identical between backends — both must
call the same `estimate_cost` function with the same `TokenUsage` to
get the same f64.

### 7.6 Error events

Two `system.error` events (one in sess-A1, one in sess-A2). Both with
distinct timestamps and distinct messages so `session_errors` ordering
is unambiguous.

---

## 8. Conformance helper catalog (per query)

Each helper added to `event_store_conformance.rs` follows the existing
shape: pure async fn taking `Arc<dyn EventStore>`, runs the query,
asserts on the typed result. New helpers are added to the
`for_each_conformance_test!` macro so both `sqlite_backend` and
`mongo_backend` mods get the wrapper for free.

The **§1.6 Category** is called out per helper: **C1** = strict
`assert_eq!`, **C2** = canonical-sort then assert.

Helpers grouped by query, with the assertion shape laid out
declaratively. **No code, just intent.**

### 8.1 `it_returns_a_synopsis_for_a_known_session` *(C1 + C2)*
**Setup:** seed analytics fixture, target = `sess-A1`.
**Action:** call `query_session_synopsis("sess-A1")`.
**Assert:** returns `Some(SessionSynopsis { ... })` with:
- `session_id == "sess-A1"` *(C1)*
- `label == Some("build feature X")` *(C1)*
- `project_id == Some("proj-alpha")` *(C1)*
- `event_count == 12` *(C1)*
- `tool_count == 5` *(C1)*
- `error_count == 1` *(C1)*
- `duration_secs == 11 * 60` *(C1)*
- `top_tools` *(C2)* — sort both backends' `top_tools` by
  `(count DESC, tool ASC)` before comparing. Fixtures are allowed to
  have tied counts; the canonical sort handles them.

### 8.2 `it_returns_none_for_unknown_session_synopsis` *(C1)*
**Setup:** seed analytics fixture.
**Action:** `query_session_synopsis("does-not-exist")`.
**Assert:** returns `None`.

### 8.3 `it_returns_tool_journey_in_timestamp_order` *(C1)*
**Setup:** seed, target = `sess-A1`.
**Action:** `query_tool_journey("sess-A1")`.
**Assert:** Vec of length 5 in timestamp ASC order, each `ToolStep`
has the right tool name and the right `file` (from the COALESCE
chain on file_path/file/path/command). Timestamps are distinct in
the fixture, so the order is unambiguous and `assert_eq!` works.

### 8.4 `it_returns_tool_journey_empty_for_unknown_session` *(C1)*
**Action:** `query_tool_journey("does-not-exist")`.
**Assert:** empty Vec.

### 8.5 `it_returns_file_impact_with_reads_and_writes_separated` *(C1)*
**Setup:** seed, target = `sess-A1`.
**Action:** `query_file_impact("sess-A1")`.
**Assert:** Vec of `FileImpact` deeply equal to the expected hand-pinned
output. The Rust-side post-sort `(reads+writes) DESC` is deterministic
because both backends emit the same `(target, tool, count)` triples
which get aggregated and sorted by the same Rust code.

### 8.6 `it_returns_session_errors_in_timestamp_order` *(C1)*
**Setup:** seed, target = `sess-A1`.
**Action:** `query_session_errors("sess-A1")`.
**Assert:** Vec of length 1 (sess-A1 has one error) with the right
timestamp and message text.

### 8.7 `it_returns_project_pulse_grouped_by_project` *(C1)*
**Setup:** seed analytics fixture.
**Action:** `query_project_pulse(365)` (generous window per §6.4).
**Assert:** Vec of length 2 (alpha + beta), sorted by `total_events DESC`.
Counts and `last_activity` match the fixture totals.

### 8.8 `it_returns_tool_evolution_bucketed_by_week` *(C3 + C1)*
**Setup:** seed analytics fixture.
**Action:** `query_tool_evolution(365)`.
**Assert:** uses the **redesigned** `ToolEvolution` struct with
`bucket_start`/`bucket_end` (§1.6 Category 3, §2 inventory).
- All fixture events are within ~24h of `now()`, so they all bucket
  into the same week
- Both backends compute the same `bucket_start` (Monday of the
  current week, in `YYYY-MM-DD` format) regardless of week-numbering
  conventions
- Each `(bucket_start, tool, count)` row is byte-equal between
  backends after sorting by `(bucket_start, tool ASC)`
- The week-label divergence that motivated the original C3 fix
  vanishes — there is no week label in the new struct

### 8.9 `it_returns_session_efficiency_for_all_sessions` *(C2)*
**Setup:** seed.
**Action:** `query_session_efficiency()`.
**Assert:** Vec of length 3. Sort both outputs by
`(last_event DESC, session_id ASC)` before comparing. Canonical sort
handles the NULL-`last_event` ordering divergence (SQLite puts NULLs
last on DESC; Mongo's default puts them first). Each entry has the
right `event_count`, `tool_count`, `error_count`, `duration_secs`.

### 8.10 `it_returns_project_context_recent_sessions` *(C1)*
**Setup:** seed.
**Action:** `query_project_context("proj-alpha", 5)`.
**Assert:** Vec of length 2 (alpha has 2 sessions), most-recent first,
each `ProjectSession` byte-equal between backends. Fixture session
`last_event` values are distinct, so the order is unambiguous.

### 8.11 `it_scopes_project_context_to_the_project` *(C1)*
**Action:** `query_project_context("proj-beta", 5)`.
**Assert:** Vec of length 1, only sess-B1.

### 8.12 `it_returns_recent_files_for_a_project` *(C2)*
**Setup:** seed.
**Action:** `query_recent_files("proj-alpha", 1)`.
**Assert:** Vec of distinct file paths from Edit/Write events in
proj-alpha sessions. Sort both outputs alphabetically before
comparing — the within-timestamp tie order between backends is
implementation-defined. Set membership and length are strict.

### 8.13 `it_returns_productivity_by_hour_bucketed` *(C1)*
**Setup:** seed.
**Action:** `query_productivity_by_hour(365)`.
**Assert:** Vec of `HourlyActivity` byte-equal between backends. Both
backends parse the same `Z`-suffixed UTC timestamps and compute the
same hour buckets (0–23) — see §6.2 (eliminated). The expected hour
buckets are computed from the fixture timestamps at seed time using
the same chrono ops the test runs against, so the assertion has
exact expected values, not "structural" hand-waving.

### 8.14 `it_returns_token_usage_summary_for_all_sessions`
**Setup:** seed.
**Action:** `query_token_usage(None, None, "sonnet")`.
**Assert:** `TokenUsageSummary` with:
- `session_count == 3`
- `usage.input_tokens == 2400`, output==1000, cache_read==450,
  cache_creation==175, total==4025, message_count==3
- `cost.model == "sonnet"`
- `cost.input == 0.0072` (and other rates by spec)
- `sessions.len() == 3`, ordered by `output_tokens DESC` →
  [sess-A1, sess-A2, sess-B1]

### 8.15 `it_returns_token_usage_for_a_specific_session`
**Action:** `query_token_usage(None, Some("sess-A1"), "sonnet")`.
**Assert:** `session_count == 1`, only sess-A1's tokens, the cost
computed from sess-A1's totals.

### 8.16 `it_returns_token_usage_filtered_by_days`
**Action:** `query_token_usage(Some(365), None, "opus")`.
**Assert:** all 3 sessions in window, `cost.model == "opus"` with the
opus rates applied.

### 8.17 `it_returns_token_usage_with_zero_cost_when_empty`
**Setup:** empty store (no fixture).
**Action:** `query_token_usage(None, None, "sonnet")`.
**Assert:** `session_count == 0`, all `TokenUsage` fields zero,
`cost.model == "sonnet"`, `cost.total == 0.0`, `sessions` empty.

### 8.18 `it_returns_daily_token_usage_bucketed_by_date`
**Setup:** seed.
**Action:** `query_daily_token_usage(Some(7))`.
**Assert:** `Vec<DailyTokenUsage>` ordered by date ASC. Buckets
correspond to the fixture event date prefixes (`2026-04-07`, etc.).
Total tokens across all buckets equals the fixture aggregate (4025).

---

## 9. Implementation order (the TDD walk)

Once the conformance helpers above are written and asserting against
SQLite (which already passes them), the Mongo implementation TDD walk
follows the same red→green pattern as Phases 3+4+6 of the original
work.

**Order from cheapest to hardest:**

1. **`query_project_context`** (#8) — pure session metadata, no event
   scan, no JSON extraction. One `find` + sort + limit. Smallest
   possible Mongo aggregation. Use it to validate the conformance
   helper machinery before tackling harder queries.
2. **`query_project_pulse`** (#5) — session metadata + group by
   project_id. Still no event scan.
3. **`query_session_errors`** (#4) — first event-scan query, but no
   nested extraction (just timestamp + message text from a known
   subtype).
4. **`query_session_synopsis`** (#1) — combines #4 patterns with
   `top_tools` extraction. Introduces the `payload.data.agent_payload.tool`
   nested path that powers the next 4 queries.
5. **`query_tool_journey`** (#2) — same nested extraction +
   COALESCE chain on file/path/command. Validates `$ifNull` chains
   and ordered output.
6. **`query_file_impact`** (#3) — same extraction + Rust-side
   reads/writes categorization. The categorization stays in Rust;
   Mongo just returns the raw (target, tool, count) triples.
7. **`query_session_efficiency`** (#7) — N+1 queries (one for sessions,
   then per-session COUNT(*) loops). Mirror the SQLite shape exactly,
   or replace with one $lookup pipeline. Either works.
8. **`query_recent_files`** (#9) — first cross-collection query.
   Two-step approach: find sessions in project, then find events
   filtered by `$in`. DISTINCT via `$group: {_id: '$file'}`.
9. **`query_productivity_by_hour`** (#10) — first date-extraction
   query. `$dateFromString` → `$hour` → `$group`.
10. **`query_tool_evolution`** (#6) — second date-extraction query.
    Drops the `week: String` field, adds `bucket_start`/`bucket_end`
    (§1.6 Category 3). Both backends compute Monday-of-week
    independently. Includes a small dashboard TypeScript update.
11. **`query_token_usage`** (#11) — the hardest. Three input modes,
    deep nested extraction, per-session aggregation, cost calculation.
    Save for last because it's the highest test-debugging cost.
12. **`query_daily_token_usage`** (#12) — shares the extraction logic
    with #11, just buckets by date prefix instead of session.

**Per-step rhythm:**
- Run the Mongo conformance helper for that query → see `todo!()` panic
  or empty-vec mismatch
- Replace the Mongo `EventStore` impl method with the aggregation
- Run the helper again → green
- Move to next

This is exactly the rhythm Phases 3+4+6 used. It worked then, it'll
work now.

---

## 10. In scope (lifted from "out of scope" by the §1.6 reframe)

Two items were originally filed as out-of-scope but the §1.6 model
brings them back in:

1. **`ToolEvolution` API redesign** *(was #2 in the old "out of
   scope" list)* — drops the `week: String` cosmetic field, adds
   `bucket_start: String` and `bucket_end: String`. This is a small
   breaking change to `/api/insights/tool-evolution` whose only
   consumer is the dashboard. Required to make `tool_evolution` a C1
   query in both backends. **Cost:** ~10 lines of TypeScript on the
   frontend + the SQLite query rewrite + the Mongo aggregation.
   **Lands in:** the same commit that implements `tool_evolution`
   on Mongo.

2. **§6.8 cutoff format normalization** — single-line fix at 5 SQL
   call sites in `queries.rs`. Replaces `cutoff.to_rfc3339()` with
   `cutoff.format(TS_FORMAT).to_string()` so the cutoff string
   matches the storage format. Removes the lexical-accident
   reasoning. **Lands in:** the first time-windowed query commit
   (probably `project_pulse`). Mongo equivalent uses typed `$gte`
   so doesn't need this fix.

## 10.1 Out of scope (filed as separate work)

These remain excluded from Phase 5 and are filed in BACKLOG when
this work ships:

1. **Clock injection for query determinism** — refactoring queries to
   take an `as_of: DateTime<Utc>` parameter instead of calling
   `Utc::now()` internally. Would make `productivity_by_hour`,
   `tool_evolution`, and the time-windowed token_usage variants
   fully time-independent. **Why deferred:** touches every query
   signature + every API handler + the CLI. Doesn't affect parity
   between backends, only test-run determinism. The §1.6 model
   doesn't need it because both backends call `Utc::now()` at the
   same instant during the test.

2. **Indexes for analytics performance** — Phase 6 added indexes for
   the basic CRUD reads but not for `payload.data.agent_payload.tool`
   or `payload.data.raw.message.usage.input_tokens`. Once Phase 5
   ships and someone hits performance issues at scale, add a wildcard
   index or specific path indexes. **Why deferred:** optimization
   without measurement.

3. **Streaming aggregation for huge result sets** — `daily_token_usage`
   over a 1-year window scans every assistant message in the database.
   For a heavy user that's tens of thousands of events. Both backends
   currently materialize the whole result in memory. A streaming
   variant (cursor → channel → React) would matter at scale. **Why
   deferred:** the API contract returns `Vec`, not `Stream`; changing
   that is a big refactor.

4. **Conformance assertions for `query_token_usage(model)` cost
   tolerance** — the `f64` cost field is computed via floating-point
   multiplication. SQLite and Mongo backends both call the same Rust
   `estimate_cost` function on the same `TokenUsage` struct, so they
   produce byte-identical f64 values. If a future refactor moves
   cost calculation into the database (e.g., a Mongo aggregation
   `$multiply`), the conformance helper would need `assert_relative_eq!`
   with a tolerance. **Why deferred:** not a problem until cost
   moves into SQL/aggregation.

---

## 11. Open questions (resolved)

The original draft had 6 open questions. After the §1.6 reframe and
the user dialogue that produced it, all 6 are resolved:

1. **Tie-tolerance vs distinct-counts in fixtures** → ✅ resolved.
   §1.6 Category 2 (canonical-sort) handles ties at the assertion
   site. Fixtures may freely have ties.

2. **Productivity-by-hour determinism** → ✅ resolved. §8.13 is now a
   pure C1 deep-equal. Both backends compute the same UTC hour
   buckets from the same fixture timestamps. Clock injection
   remains in BACKLOG as a separate concern (§10.1) but doesn't
   block Phase 5.

3. **Week-boundary fixture invariant** → ✅ resolved by §1.6
   Category 3 (`ToolEvolution` API redesign). No date-shift dance.
   No `fixture_anchor`. The fixture seeder is just `ts_offset(h, m)`.

4. **Analytics fixture lives where** → ✅ resolved: inline in
   `event_store_conformance.rs` next to existing helpers. Consistency
   with the Phase 1 pattern.

5. **Snapshot testing vs hand-written assertions** → ✅ resolved:
   stay consistent with the existing conformance suite, use inline
   `assert_eq!`. Snapshot framework is BACKLOG material if it ever
   matters.

6. **Update BACKLOG entry to point at this plan** → ✅ yes, when
   Phase 5 work begins, the BACKLOG entry filed by commit `0e927c6`
   gets a one-line addition: "Plan: see
   `docs/research/mongo-analytics-parity-plan.md`."

There are no remaining blockers. Phase 5 is unblocked.

---

## 12. Definition of done

Phase 5 is complete when:

- All 12 query methods are implemented in `MongoStore` (no `todo!()`,
  no fall-throughs to the trait default)
- All 18 conformance helpers (§8.1–§8.18) added to
  `event_store_conformance.rs` and registered in
  `for_each_conformance_test!`, each tagged with its §1.6 category
  in a comment so future readers see which parity pattern is in
  effect
- `cargo test -p open-story-store --features mongo` shows **48
  passing tests** (existing 30 + 18 new) for both `sqlite_backend`
  and `mongo_backend` mods → 96 total
- `ToolEvolution` struct redesigned per §1.6 Category 3
  (`bucket_start`/`bucket_end` replace `week: String`); SQLite query
  rewritten; dashboard TypeScript updated to format `bucket_start`
  into a week label client-side; `/api/insights/tool-evolution`
  returns the new shape
- §6.8 cutoff format normalized in all 5 SQL call sites in
  `queries.rs`; one targeted conformance test
  (`it_includes_events_at_the_exact_cutoff_instant`) pins the fix
- `python3 scripts/sessionstory.py {session} --url $mongo_url` produces
  a synopsis section with non-empty `tool_count`, `error_count`,
  `top_tools` (currently zeros when run against Mongo)
- BACKLOG "MongoStore Analytics Query Parity" entry is removed
- BACKLOG "Clock injection for query determinism" entry is added
  (filed as a separate follow-up under §10.1 #1)
- Short addition to `CLAUDE.md` Architecture section: analytics now
  work on both backends; the §1.6 three-category model is the parity
  contract
- `python3 scripts/check_docs.py` stays 13/13 green
- One commit per group from §9 (or one combined commit if the work
  fits cleanly), with phase rationale in the commit body matching
  the existing Phase 0–9 commit style on `feat/mongodb-sink`

---

## Appendix A: Why this plan is in `docs/research/` and not `docs/plans/`

There is no `docs/plans/` directory in the project, and the `docs/`
top-level is reserved for shipped documentation (`BACKLOG.md`,
`architecture-tour.md`, `soul/`). Pre-implementation analysis lives in
`docs/research/` alongside the session reports and the eval-apply
prototype work. When Phase 5 ships, this file becomes a historical
artifact — the canonical answer to "why does the analytics conformance
suite look the way it does?" — same as how
`docs/research/eval-apply-prototype/` documents the design decisions
behind the eval-apply detector.

## Appendix B: Why no Mongo aggregation pipelines are written here

The implementation TDD walk in §9 is intentionally vague about *which*
Mongo operators to use. Three reasons:

1. **The conformance suite is the spec.** As long as the helper passes
   for both backends, the implementer is free to use whichever
   aggregation primitive they prefer. Pinning the operator choice in
   the plan would make this document a maintenance burden when Mongo
   3.x → 4.x deprecates an operator.

2. **The §5 cheat sheet is enough.** It maps each SQL idiom to its
   Mongo counterpart at the right level of abstraction.

3. **Two-step `find` + `find` is often simpler than `$lookup`.** For
   cross-collection queries like `recent_files`, the right answer is
   often "two queries from Rust" not "one $lookup pipeline." Pinning
   the operator in advance would prevent that simplification.

The implementer has full latitude to choose the aggregation shape.
The conformance suite catches divergence either way.
