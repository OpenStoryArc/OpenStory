# OpenStory UX Review — Senior Developer Persona

**Status:** v4 (final + coordination refresh) · **Date:** 2026-06-30 · **Branch reviewed:** `feat/ui-session-visibility`
**Scope:** Pure UX/product review. Report only — no code changes. A separate loop is doing UI upgrades concurrently (see §7).
**Method:** Five grounded read-only audits of the live codebase (boot, loading/first-paint, search/navigation, secrets/encryption/trust, performance-at-scale), each producing severity-ranked pain points against real file:line references, synthesized here. Scale rankings validated against the **live event store** (1,428 sessions; largest single session 12,161 events; ~400k+ events total) — the at-scale concerns are real *today* on this instance, not hypothetical.
**v2 changes:** added the *Performance at scale* dimension (§3E). The ~44 ev/s per-event write ceiling is **already fixed** (batched transactions); remaining scale blockers are read/boot/memory-side.
**v3 changes:** reconciled findings against `docs/BACKLOG.md` (§9) — several scale/trust items are already tracked, one with a staged design; corrected the delete-durability nuance (T4).
**v4 changes:** the concurrent UI loop *shipped six features while this review ran* (⌘K palette, Overview dashboard, harness-message untruncation, Story find, D3 ribbon). §7 rewritten to reconcile against what actually landed — several findings are now partially resolved, and a few net-new claims are downgraded. Precise calibration: the ⌘K palette is **nav-only** (fuzzy over labels, no FTS content search), the Overview dashboard does **not** surface `/api/insights/*`, and Explore is **still unvirtualized** — so S1, the content-search half of S2, and L1/P3 all still stand.

---

## 1. Persona & lens

**Who:** A senior developer evaluating OpenStory on their own machine. They already run coding agents daily, care about performance, hate ceremony, and — because OpenStory's pitch is *sovereignty* — they will probe privacy claims harder than a casual user. They read config files, notice a 24h horizon, and will `curl` an endpoint to check a claim.

**What "delightful" means for them:**
- **Honest.** The UI never tells them something false — not "disconnected" when connected, not "no events" when a history exists, not "encrypted" when it isn't.
- **Fast to first value.** One command, one URL, their own data on screen in minutes.
- **In control.** Boot, ports, what's stored, what leaves the machine — all legible and adjustable without archaeology.
- **Powerful when they lean in.** The depth (FTS grammar, analytics, deep-links) is discoverable, not buried behind reading `router.rs`.

---

## 2. Executive summary — the through-line

**OpenStory is well-engineered underneath and under-delivered at the surface.** Every dimension shows the same shape: a genuinely good primitive exists in Rust, and the UI/onboarding either hides it, misframes it, or defaults it to the least-safe/least-honest setting.

The single most damaging pattern for this persona is **the UI misrepresenting a healthy system as broken or unsafe**:
- A healthy cold boot flashes **"Disconnected from server"** (default status is `disconnected`, not `connecting`).
- A machine with a full history shows **"No events yet"** on the Live tab because `initial_state` is now sidebar-only.
- A fresh install shows a **blank sidebar with zero onboarding** — reads as "broken," not "expected, go generate data."
- Setting `db_key` on a standard build **silently stores plaintext** — encryption theater.
- Joining a network **publishes all sessions by default** with no per-session consent and no redaction.

A second pattern, surfaced by the scale audit, is **cost that grows with total history instead of the working set**: the app replays every event of every session on each boot and holds a full copy of every session's records in RAM, so a power user with months of data pays linearly in boot time and memory for data they aren't looking at. The encouraging half: the *write* path was already re-architected off its old per-event bottleneck; the remaining blockers are read/boot/memory-side and are well-localized.

The good news: most of the highest-impact fixes are **truth-in-UI** changes — small, safe, and independent of the deeper architectural work. The report is organized to make that triage obvious.

**Severity roll-up across all five audits:**

| Severity | Count | Examples |
|---|---:|---|
| Blocker | ~10 | Explore timeline unvirtualized + unpaginated full-session fetch; full-corpus replay at boot; unbounded in-RAM projections; silent-no-op encryption; zero transcript redaction; default-on publish; non-durable delete; 0.0.0.0 auto-bind no-auth; insights suite has no UI entry |
| Major | ~22 | False "disconnected"/"no events" flashes; no first-run empty state; single-connection mutex serializes the store; N+1 full-session loads; missing indexes on hot columns; no global search/cmd-K; search filters not wired; key colocated with ciphertext; no UI delete/export; silent 24h backfill |
| Minor | ~16 | No skeletons (CLS everywhere); misleading "%" relevance; `?token=` log leak; config.toml.example intimidating; FTS `session_id` unindexed |

---

## 3. Findings by dimension

### 3A. Boot & startup — *"too many doors, each with a different default"*

The engineering is solid (fail-fast NATS with actionable copy `main.rs:664-687`; clean defaults→TOML→env→CLI precedence `main.rs:448`; a genuinely nice `open-story init` wizard `init.rs`; self-launching managed NATS). The UX problem is **discovery and defaults**, not capability.

| # | Pain | Sev | Where |
|---|---|---|---|
| B1 | **8+ documented start paths**, no single "one command"; README calls both Homebrew *and* `just up` "recommended" | Major | README §Quickstart/§Quick Start |
| B2 | `just up` **silently requires Docker** (pulls `mongo:7`, builds `--features mongo`) despite being presented as the default dev command; the zero-dep SQLite path is the less-obvious `up-no-mongo` | Major | `justfile:69-112,218-241` |
| B3 | The **wizard is walled off** — `open-story init` is Homebrew-only in docs; `git clone` + `just up-no-mongo` never invokes it, leaving hand-edited TOML | Major | `init.rs`, `justfile` |
| B4 | Auto-install is **Homebrew-only** — dies on Linux (`--install` shells to `brew`); no apt/dnf/pacman | Major | `check-prereqs.sh:128-135` |
| B5 | Boot recipes **don't call the friendly `just check`** — missing `cargo`/`npm` yields a bare mid-recipe `command not found` | Major | `justfile:82-85,126-129` |
| B6 | `just up` **kills whatever owns :3002/:5173 without asking** (`lsof -ti | kill`) — clobbers another Vite on 5173 | Major (footgun) | `justfile:47-62,78-79` |
| B7 | **Silent 24h backfill horizon** — weeks of history look like data loss; non-wizard paths never announce it. Second silent ceiling: `max_initial_records=2000` | Major | `main.rs:84`, `config.toml:24-25` |
| B8 | **Port story differs per door** (3002 single-process vs 5173 Vite proxy) — two "open this URL" answers | Minor/Major | `README.md:37,348` vs `CLAUDE.md:128` |
| B9 | `config.toml.example` is **66 lines of auth-tier machinery**; the 3 knobs a first-timer wants are buried | Minor/Major | `data/config.toml.example` |
| B10 | `scripts/openstory` launcher **requires hand-editing its own source** before first use | Minor | `README.md:376-382` |

**Delightful version:** one front door (`just start` / bare `open-story` → wizard on first run, prints the exact URL); recipes gate on cross-platform `just check`; boot **announces** the backfill window ("Backfilling last 24h — N sessions found; `--watch-backfill-hours=0` for all"); detect-and-confirm before reclaiming ports; a slimmed 10-line example config with advanced knobs split out.

### 3B. Loading & first-paint — *"the live core is great; the cold boot lies"*

The live-streaming path is the best-engineered part of the app: dedup by id (`sessions.ts:66-85`), 16ms `bufferTime` coalescing (~60fps), rAF-gated autoscroll, and `@tanstack/react-virtual` in Live and Story feeds. The pain is concentrated at **cold-load framing** and the **Explore tab's divergent loading philosophy**.

| # | Pain | Sev | Where |
|---|---|---|---|
| L1 | **Explore `SessionTimeline` is NOT virtualized** — fetches the entire session (no `limit`) and `rows.map()`s every card; thousands of DOM nodes → jank. The single biggest perceived-perf cliff | Blocker | `SessionTimeline.tsx:49,283` |
| L2 | **False "Disconnected from server" flash** on every healthy cold boot — status seeds `disconnected`, flips to `connecting` a tick later | Major | `use-connection-status.ts:5`, `empty-state.ts:33-39` |
| L3 | **"No events yet" despite a full history** — `initial_state` is sidebar-only post-lazy-load; Live main pane reads as empty while the data sits unloaded | Major | `sessions.ts:11-19,93-102`, `empty-state.ts:49-54` |
| L4 | **No first-run empty state** for a truly empty install; sidebar code literally comments that the zero-sessions case "wasn't previously handled" | Major | `Sidebar.tsx:590-593` |
| L5 | Explore **blocks on the full fetch** — blanks the whole pane behind "Loading events..." while Live streams progressively | Major | `SessionTimeline.tsx:217-219` |
| L6 | **No skeletons anywhere** — every loading state is unstyled single-line text → content jump/CLS on every resolve | Minor (pervasive) | `Timeline.tsx:665`, `ConversationView.tsx:42`, +6 more |
| L7 | Blank `<div id="root">`, no app-shell/inline skeleton — dark void until the bundle mounts | Minor | `ui/index.html:8` |
| L8 | Progressive backfill can **visibly reshuffle rows** as older pages stream in; no "loading earlier events" affordance once the first page lands | Minor | `sessions.ts:84`, `Timeline.tsx:364-387` |

**Delightful version:** default status `connecting`; a real zero-state that names the watch dir and backfill window; when history exists but Live is quiet, "pick a session to load its history" (or auto-select the most recent); Explore inherits Live's virtualization + progressive `streamSessionRecords`; height-reserving skeletons everywhere.

### 3C. Search & navigation — *"the headline gap: backend capability vs UI exposure"*

Deep-linking is the strong point (hash routes, working back/forward, bookmarkable session/event/file/search URLs — `hash-route.ts:79-96`). Search and analytics are the weak point: the backend has a capable FTS5 + ~8 analytics endpoints; the UI exposes a thin sliver.

| # | Pain | Sev | Where |
|---|---|---|---|
| S1 | **Entire `/api/insights/*` family has no UI entry point** — pulse, token-usage/cost, daily trends, productivity, tool-evolution, efficiency are unreachable from any tab | Blocker (discoverability) | `router.rs:216-237` |
| S2 | **No global search / no persistent search bar / no cmd-K palette** — must click Explore → "Search" sub-tab; Live has no way to search | Major | `App.tsx:88-100`, `ExploreView.tsx:14-19` |
| S3 | **Misleading "semantic" framing** over an FTS5 keyword engine ("Search by meaning…"); a synonym search returns empty and erodes trust | Major | `SemanticSearch.tsx:85`, `sqlite_store.rs:250-303` |
| S4 | Backend `project=` / `days=` filters **exist but are never sent** — silently narrows to 30 days with no UI control | Major | `SemanticSearch.tsx:43`, `api.rs:1559-1567` |
| S5 | **No filters at all** (project/session/tool/date/type) despite `record_type` being returned per result | Major | `sqlite_store.rs:161`, `api.rs:1678` |
| S6 | Click lands on the **session, not the exact matched event** — every result carries `event_id` and `scrollToEventId` exists; one-argument gap | Major | `SemanticSearch.tsx:115`, `ExploreView.tsx:36-37` |
| S7 | **No project-level navigation** — no `#/project/<id>` route; can't bookmark "everything in project X" | Major | `hash-route.ts:24` |
| S8 | **Two parallel session browsers** (Live vs Explore) with different capabilities and **no shared selection** — switching tabs drops the session | Major | `App.tsx:79-81` |
| S9 | **FTS5 grammar un-surfaced** (phrase/prefix/boolean/stemming all work) AND raw punctuation (`foo()`, `a:b`) errors as "Search failed" — no sanitization | Major | `sqlite_store.rs:266`, `SemanticSearch.tsx:53` |
| S10 | Misleading **"%" relevance** — a rescaled negative bm25 rank rendered as a probability | Minor | `SemanticSearch.tsx:127` |
| S11 | `/api/search` (flat, session-scoped, `<b>`-highlighted) entirely **unused**; `HighlightText` component unused in results | Minor | `api.rs:1519`, `HighlightText.tsx` |

**Delightful version:** global cmd-K palette (nothing binds a global shortcut today) + persistent header search wired to the existing `#/search?q=` deep link; faceted filters on the already-supported params; event-precise landing with a lingering highlight; honest "full-text" framing with syntax hints + input sanitization; an **Insights tab** turning ~8 stranded endpoints into dashboards.

### 3D. Trust, secrets & encryption — *"the sovereignty spine defaults to least-safe"*

This is the dimension the target persona will judge hardest, and it is the weakest. The docs are unusually honest about the gaps — but honesty in `docs/deploy/distributed.md` is not a safe default or a UI guardrail. **Verified:** `publish_sessions` defaults to `true` on both this branch and master (`config.rs:328-330`); the `default_share_policy` opt-in knob noted in prior hardening work was never landed.

| # | Pain | Sev | Where |
|---|---|---|---|
| T1 | **Encryption silently no-ops** — SQLCipher only activates with `--features encryption`; on a standard build a set `db_key` is accepted and ignored, plaintext DB, only a scrolled-away stderr line. No runtime "encryption active" signal | Blocker | `sqlite_store.rs:39-66` |
| T2 | **Zero transcript redaction/secret-scanning at ingest** — `.env`s, `sk-ant-…`, passwords stored verbatim in SQLite *and* JSONL, and made full-text searchable. The watcher becomes a durable searchable plaintext archive of every secret the agent touched | Blocker | `ingest.rs:562,691,759,829,879` |
| T3 | **Publishing is default-on with no per-session consent** — setting `nats_leaf_url` streams *all* sessions (secrets included) onto the bus, bidirectionally, no prompt/diff/recall | Blocker | `config.rs:383`, `distributed.md:9,66` |
| T4 | **"Delete" is incomplete** — removes SQLite rows only; leaves the JSONL backup *and* the source transcript in `~/.claude/projects`. Per BACKLOG (line 720) the inert JSONL backup does **not** resurrect the session (boot replay reads EventStore, not JSONL), but the **source transcript is untouched**, so the watcher re-ingests it on the next backfill pass within `watch_backfill_hours` — the secret comes back. Delete is incomplete + not durable *for the secret*, even if not for the session row | Blocker | `api.rs:1906-1945`, `persistence.rs`; BACKLOG:720 |
| T5 | **0.0.0.0 auto-bind with no auth and no warning** — containers/WSL bind all interfaces while `api_token` defaults empty; full read/search API + WS stream exposed to the LAN unauthenticated | Blocker | `config.rs:309-322`, `auth.rs:49-57` |
| T6 | **Key colocated with ciphertext** — `db_key` sits in world-readable `config.toml` in the *same dir* as the DB; no keyring/env-only/permission check | Major | `config.rs:538`, `sqlite_store.rs:49` |
| T7 | **JSONL backups never encrypted** — `db_key` protects only SQLite; identical content sits in cleartext JSONL | Major | `persistence.rs:35-38` |
| T8 | **No UI affordance for delete or export** — realizing a session captured a secret means hand-crafting `curl DELETE`; export endpoint is clean but UI-invisible | Major | `ui/src` (no calls), `api.rs:1898,1952` |
| T9 | **Federation delete is local-only** — no tombstone propagation; once shared, a secret can't be un-shared network-wide | Major | `distributed.md:57-67` |
| T10 | No **token-mint affordance** — path of least resistance is "exposed and open"; `?token=` query auth leaks to logs/Referer | Major/Minor | `auth.rs:11-14` |

**Delightful version:** encryption that can't lie (ship SQLCipher; **refuse to boot** if `db_key` set on a non-cipher build; expose `encryption_active` on `/api/health`; encrypt or stop writing JSONL when on); fail-safe binding (non-loopback + empty token → refuse or auto-mint + print once); **redaction by default at ingest** (the pattern set already exists in `scripts/scrub_check.py`, just not wired in); consent before anything leaves the machine (default publish off, pre-publish secret scan, redacted-shape publish mode from `CONSTELLATION.md`); real "Delete everywhere" in the UI (rows + JSONL + source ignore-list/tombstone).

### 3E. Performance at scale — *"the write wall is gone; the boot/read/memory walls remain"*

As history grows (months of transcripts, sessions with tens of thousands of events, hundreds of projects), most costs today scale with **total corpus size**, not the working set. **This is not hypothetical:** the live instance already holds **1,428 sessions**, a largest single session of **12,161 events**, and **~400k+ events total** — so boot replay (P1) already walks hundreds of thousands of events, and opening that biggest session (P3) already fetches 12k events unpaginated. **Verified good news:** the old ~44 ev/s per-event write ceiling is *already fixed* — the persist consumer batches inserts, FTS indexing, and JSONL appends into one transaction per batch (`consumers/persist.rs:139-160,199-277`; `sqlite_store.rs:399-436,874-885`), turning ~300 fsyncs per 100-event batch into ~3. The remaining pain is on the read/boot/memory side.

| # | Pain | Sev | Where / scaling |
|---|---|---|---|
| P1 | **Full-corpus replay on every boot** — `replay_boot_sessions` loops all sessions and re-`append`s every event to rebuild `seen_ids` + projections; O(total events) CPU **and** RAM at every startup, no recency gate. The worst at-scale behavior in the system | Blocker | `ingest.rs:371-489` |
| P2 | **`projections` DashMap holds a full in-RAM copy of every session** — each `SessionProjection.records: Vec<ViewRecord>` + several O(events) maps, one per session, populated for *all* sessions at boot, never evicts. Resident memory = O(total events across all history) | Blocker | `projection.rs:153-155`, `ingest.rs:201` |
| P3 | **`GET /api/sessions/{id}/records` loads + serializes the entire session, unbounded** — no limit/pagination/cursor; client feeds all records into `buildEventGraph` + facet memos with no virtualization. A 10k-event session pays full DB load + full JSON + full client graph build on every open (this is the backend half of L1) | Blocker | `api.rs:783-808`, `SessionTimeline.tsx:49,79-82` |
| P4 | **Single `Mutex<Connection>` serializes the whole store** — every read and write funnels through one guarded connection, nullifying WAL's multi-reader benefit; API reads block behind backfill writes and vice-versa | Major | `sqlite_store.rs:23-26,92` |
| P5 | **`GET /api/sessions` loads ALL rows, sorts in memory, then paginates**, + a full plan-store scan per call; `ORDER BY last_event DESC` has **no index** → full-table scan + sort on every sidebar refresh | Major | `api.rs:115-277`, `sqlite_store.rs:457-461` |
| P6 | **N+1 full-session loads** — `session_digests`, `catch_up`, `reproject`, and boot replay each do `list_sessions()` then `session_events()` per session (100 sessions × 10k events ≈ 1M row loads/request) | Major | `api.rs:82-113`, `catch_up.rs:34-36` |
| P7 | **Missing indexes + per-row `json_extract` on analytics** — no `idx_events_timestamp`, no `idx_sessions_last_event`; every insights query full-scans `events` and re-parses each row's JSON payload at query time | Major | `queries.rs:173,423,572-580`; `sqlite_store.rs:104-105` |
| P8 | **Patterns consumer writes turns/patterns one INSERT at a time** — reintroduces per-item transaction cost on the shared mutex during backfill (the events path was batched; this one wasn't) | Major | `server/mod.rs:265-269` |
| P9 | **FTS `session_id` is UNINDEXED** — per-session search matches the whole corpus first then filters as an auxiliary column; `delete_session` does a full FTS scan | Minor/Major | `sqlite_store.rs:159-164,260-269,808` |
| P10 | **Fan-out re-work** — 4 consumers each independently `serde_json`-reparse every event; **no retention surfaced** so events table, FTS, and projection RAM grow monotonically (`cleanup_old_sessions` exists but isn't exposed) | Minor | `server/mod.rs:216-269`, `sqlite_store.rs:817` |

*Refuted suspicions (good, worth stating):* the `initial_state` WS handshake is already bounded — labels + patterns for recent sessions only, no records, stays a few KB at 10k sessions (`ws.rs:57-137`). `max_initial_records=2000` is effectively vestigial for the WS path. And first-boot backfill is mtime-windowed by `watch_backfill_hours`, so a huge `~/.claude/projects` doesn't ingest all history at once.

**Delightful at-scale version:** boot without full replay — persist `seen_ids`/labels/token-totals as durable session metadata and lazily hydrate a projection only when a session is opened, held in a **bounded LRU** (boot becomes O(active sessions), not O(total events)); keyset-paginated `/records` (`?limit&before=cursor` on the existing `(timestamp,id)` index) + client virtualization; single `GROUP BY` aggregates instead of N+1 per-session loads; a connection pool (read-pool + single writer) so WAL pays off; batch the pattern writer; add `idx_events_timestamp` / `idx_sessions_last_event` + generated columns for hot analytic fields; surface `cleanup_old_sessions` as a retention knob so everything stays bounded to a rolling window.

---

## 4. Cross-cutting themes

1. **Truth-in-UI is the cheapest delight.** Four of the highest-impact issues (L2, L3, L4, T1) are the product asserting something false. Fixing them is small and mostly independent of deeper work. Do these first — they convert "feels broken/untrustworthy" into "feels solid" at near-zero risk.
2. **Capability exists; exposure doesn't.** Search grammar, analytics suite, delete/export, event-precise landing, the init wizard — all built, all stranded. The roadmap is disproportionately *wiring*, not *building*. High leverage.
3. **Two philosophies live in one app.** The Live path (virtualized, progressive, deduped) is delightful; the Explore path (full-fetch, unvirtualized, blocking) is not. Converging Explore onto Live's patterns removes the worst perf cliff and a whole class of loading-jank.
4. **Sovereignty is the brand, and it's the weakest dimension.** For *this* persona the secrets/encryption gaps aren't edge cases — they're the pitch failing under inspection. This is where "delightful" and "trustworthy" are the same requirement.
5. **The wizard is the model for everything.** `open-story init` is exactly the right onboarding instinct — it just needs to be reachable from the clone path and mirrored by in-product empty states.
6. **Cost scales with total history, not the working set.** Boot replays everything; RAM holds everything; opening a session loads everything. For the persona most likely to accumulate months of data, the app gets slower precisely as they invest in it. The fix pattern is uniform — *bound everything to the recent/active window* (LRU projections, keyset pagination, retention knob, lazy hydration) — and the write path already proved the team knows how to do this (batched txns replaced the per-event wall).

---

## 5. Roadmap (three horizons)

Sequenced by *impact ÷ effort* and *risk*. Horizon 0 is safe truth-in-UI; Horizon 2 is the architectural sovereignty spine.

### Horizon 0 — "Stop lying" (days, low risk, high trust ROI)
1. Default connection status `connecting`, not `disconnected` (L2).
2. Real first-run / empty-history states that name the watch dir + backfill window (L3, L4, B7).
3. Announce the 24h backfill horizon as a boot line (B7).
4. `encryption_active` on `/api/health` + boot line that doesn't scroll away; **refuse to boot** if `db_key` set without cipher support (T1).
5. Warn (or refuse) on non-loopback bind + empty `api_token` (T5).
6. Skeletons replacing single-line "Loading…" text (L6, L7).

### Horizon 1 — "Parity & delight" (1–2 sprints, medium effort)
7. **Virtualize Explore** `SessionTimeline` + **keyset-paginate `/records`** (`?limit&before=cursor`) so the client streams a window instead of the whole session (L1, L5, **P3**). *(Concurrent loop is adding a ribbon here — coordinate, see §7.)*
7a. **De-N+1 the session list & digests** — serve the sidebar from the `sessions` table alone, add `idx_sessions_last_event`, paginate in SQL; replace per-session `session_events()` with `GROUP BY` aggregates (**P5, P6**). Directly speeds the most-hit endpoint at scale.
8. **Global cmd-K palette + persistent header search** wired to `#/search?q=` (S2).
9. Faceted search filters (project/days/type) + event-precise result landing + honest "full-text" framing + input sanitization (S3–S6, S9, S10).
10. **Insights tab** surfacing `/api/insights/*` + project-level recent-files (S1).
11. Per-session **Delete + Export buttons** in the UI (T8).
12. One front door: `just start` → wizard on first run; recipes gate on cross-platform `just check`; detect-and-confirm before killing ports (B1–B6).

### Horizon 2 — "Sovereignty spine + scale spine" (strategic, higher effort/design)
12a. **Boot without full replay** — persist `seen_ids`/labels/token-totals as durable session metadata; lazily hydrate projections into a **bounded LRU** on session-open (**P1, P2**). Turns boot + steady-state RAM from O(total events) into O(active sessions) — the highest-leverage scale fix.
12b. **Connection pool + batched pattern writes + hot-column indexes/retention** — read-pool + single writer so WAL pays off (P4); batch `insert_turn`/`insert_pattern` (P8); add `idx_events_timestamp` + generated columns for analytics (P7); surface `cleanup_old_sessions` as a retention knob (P10).
13. **Redaction at ingest by default** — port `scrub_check.py` patterns into the pipeline; store redacted body, sealed raw only on opt-in; flag "N suspected secrets" in the UI (T2).
14. **Consent before publish** — default `publish_sessions` off (or prompt on first `nats_leaf_url`); pre-publish secret-scan gate; redacted-shape publish mode (T3).
15. **Real, durable, network-wide delete** — rows + JSONL + source ignore-list + federation tombstone propagation (T4, T9).
16. **Encryption that can't lie, end to end** — ship SQLCipher default; encrypt JSONL or drop it when on; keyring/env-only key storage + colocation check (T6, T7).

---

## 6. Suggested feature backlog (candidate `BACKLOG.md` entries)

Prioritized; each is a self-contained "what + why." Grouped to map onto the horizons above.

**Trust & sovereignty (highest strategic value for this persona)**
- **Secret redaction at ingest** — detect `sk-ant-*`, bearer tokens, `.env` assignments, private keys; store redacted + opt-in sealed raw; surface a "suspected secrets" badge. *Why: the sovereignty pitch fails inspection without it.*
- **Encryption honesty** — health signal + refuse-to-boot on misconfigured `db_key` + JSONL coverage. *Why: silent plaintext is worse than no encryption.*
- **Consent-gated publishing** — default off, pre-publish scan, redacted-shape mode. *Why: default-on network publishing of secrets is disclosure-by-construction.*
- **Delete-everywhere** — durable local delete + backfill ignore-list + federation tombstones, with a UI button. *Why: "delete" that resurrects on reboot is a broken promise.*
- **Fail-safe binding** — token auto-mint/refuse on 0.0.0.0. *Why: the environment that most needs a token (containers/WSL) currently gets none.*

**Findability**
- **Command palette (cmd-K)** — global fuzzy jump to sessions/projects/events + search.
- **Global header search + faceted filters** — wire the already-supported params; event-precise landing.
- **Insights tab** — dashboards over the stranded `/api/insights/*` suite (cost, productivity, tool-evolution).
- **Project route** — `#/project/<id>` for per-repo history.
- **Unified session selection** across Live/Explore (shared route state).

**First-run & onboarding**
- **One front door + wizard on clone path** — collapse 8 doors; reachable `open-story init`.
- **In-product empty/onboarding states** — watch dir, backfill window, "run a session to see it here."
- **Cross-platform prereqs/auto-install** — apt/dnf/pacman/winget in `check-prereqs.sh`; recipes gate on `just check`.
- **Skeleton loading system** — height-reserving placeholders, kill CLS.

**Performance at scale (bound everything to the working set)**
- **Boot without full replay** — durable session metadata + lazy LRU projections. *Why: boot time and RAM currently grow with total history, not active work.*
- **Keyset-paginated `/records` + Explore virtualization** — open a 50k-event session in constant time.
- **De-N+1 the session list/digests + hot indexes** — `GROUP BY` aggregates, `idx_sessions_last_event`, `idx_events_timestamp`; serve sidebar from `sessions` alone.
- **Connection pool (read-pool + single writer)** — let WAL's many-readers actually pay off; stop API reads blocking on backfill.
- **Retention knob** — surface `cleanup_old_sessions` so events/FTS/RAM stay bounded to a rolling window.
- **Batch the pattern writer** — mirror the events batching already proven on the write path.

---

## 7. Coordination with the concurrent UI-upgrade loop

The concurrent loop **shipped six features on this branch while this review ran** (commits `76f422a`→`ce25e81`). The two efforts are already converging — the UI loop's own review log (`docs/reports/ui-loop-reviews.md`, Review #1) cites *this* report ("the prior UX review flagged the app 'tells falsehoods' on boot"). Reconciliation below is precise, because "shipped" ≠ "fully resolved."

**What landed and how much of this report it resolves:**

| Shipped (commit) | Report finding | Resolution |
|---|---|---|
| **⌘K command palette** (`ce25e81`, `CommandPalette.tsx`, `command-palette.ts`) | S2 (no global search / no cmd-K) | **Partial.** Global keyboard nav + fuzzy session jump now exist — the navigation half of S2 is *done*. But the palette fuzzy-matches over labels only; it does **not** call `/api/search`/FTS, so **content search across events is still unaddressed.** The remaining S2 ask is "let ⌘K also search event *content* via the existing FTS backend." |
| **Sessions Overview dashboard** `#/overview` (`3edec49`, calendar heatmap + facets) | S1 (analytics discoverability), S7 (project nav), S8 (parallel browsers) | **Partial.** A strong new global session browser with project/host/user/branch/status/agent facets — materially improves findability and gives a project-facet path (softens S7). But grep confirms it does **not** consume `/api/insights/*` (pulse, token-cost, productivity, tool-evolution) — so **S1's core gap survives**: the analytics suite is still API-only. |
| **Harness-message untruncation** (`55ac735`, `harness-message.ts`) | — | New; not in this report's scope. Good call — improves timeline legibility. |
| **Story sidebar find** (`fe124ec`) | S2-adjacent | In-view search for Story; complements the palette. |
| **D3 Session Activity Ribbon** (`76f422a`, Explore) | — | New viz; **verified it did *not* virtualize** `SessionTimeline` — `rows.map()` at `SessionTimeline.tsx:283`, no `useVirtualizer`, no `?limit`/`before` pagination. |

**Still open, confirmed against the landed code (do not assume the UI loop covered these):**
- **L1 / P3 — Explore virtualization + `/records` pagination.** Untouched; the ribbon sits *on top of* the same unvirtualized full-fetch list. This is now the clearest hand-off: the UI loop owns `SessionTimeline.tsx`, so they are the natural owner of virtualizing it — flag it into their roadmap rather than opening a parallel effort.
- **S1 — Insights tab over `/api/insights/*`.** The Overview is session-metadata, not the token-cost/productivity/tool-evolution analytics. Still a distinct, un-built surface.
- **Content-search half of S2** — wire ⌘K (or a dedicated search view) to the FTS backend, with the filters/event-precise-landing findings (S4–S6, S9).
- Everything in the **boot**, **trust/secrets**, and **scale** dimensions — the UI loop is UI-only and touches none of it.

**Their roadmap vs. this report (avoid collisions, note the new idea):** the UI loop's next tasks are a **turn trace view** (tool-call durations, waterfall) and continued palette work; their Review #1 also independently surfaced *URL-encoded filter state*, *skeleton loading* (= this report's L6), and *error-first affordances*. The **trace view is a genuinely new idea not in this report** — endorse it; "where did the time go in this turn" is exactly the story-telling signal the senior-dev persona wants, and it composes cleanly with the ribbon. Where the two lists overlap (skeletons), treat it as corroboration, not duplication.

**Net:** the UI loop has closed the *navigation* and *session-browsing* gaps faster than this report could recommend them — genuinely good. The gaps this report should keep pointing at are the ones the UI loop structurally can't reach from the front-end (boot, trust, scale) plus the two front-end items it hasn't taken (**Explore virtualization** and **content-search / insights surfacing**).

---

## 8. Open questions / things to verify before acting

- **`publish_sessions` default** — confirmed `true` on branch+master; the `default_share_policy` opt-in knob from earlier hardening was never landed. Decide: flip default, or gate on first `nats_leaf_url`?
- **Encryption build** — is the shipped/Homebrew binary compiled with `--features encryption`? If not, T1 is live in production, not hypothetical.
- **Backfill vs. "no data"** — how often do real first-runs hit the 24h cliff? (Answerable from the event store — worth a `scripts/` query before prioritizing B7.)
- **Redaction cost** — inline secret-scanning at ingest adds per-event CPU. The write path is now *batched* (not the old ~44 ev/s per-event wall), so the question is whether regex scanning erodes batch throughput. Prototype in `scripts/` first (principle 8).
- **Scale realism** — how large are real corpora today? Boot replay (P1) and projection RAM (P2) are O(total events); worth measuring current boot time + RSS against the live store before ranking them against UX work. A `scripts/` query can pull total-event and per-session-max counts.
- **Concurrent-loop landing** — the ribbon/viz work touches `SessionTimeline.tsx`; confirm it's merged before scheduling the virtualization/pagination work that also touches it.

---

## 9. Reconciliation with `docs/BACKLOG.md`

Honesty check: several findings — especially on the scale and trust axes — are **already tracked**, one with a fully staged design. This report's value is the UX framing, cross-dimension prioritization, and the un-tracked items. Reviewers should treat the "already tracked" column as *validation* (the team independently reached the same conclusion), not novelty.

**Already tracked (credit the backlog; some have designs):**
| Finding | BACKLOG | Status there |
|---|---|---|
| Full-corpus replay at boot (P1) | line 126 | **Staged design** — `bootstrap_session_index` + lazy `hydrate_session` via `OnceCell`, `POST /api/admin/rehydrate`. This is the canonical fix; endorse it. |
| Unbounded `full_payloads` cache (part of P2) | line 129 | Configurable LRU by byte size, EventStore fallback on miss. |
| Live-event-during-replay consistency (P2-adjacent) | line 132 | Convergence test + document window as expected. |
| Empty/"looks broken" rows during replay (L3/L4-adjacent) | line 58 | `replay_status` on `/api/health` + handshake → "Reconstructing sessions…" hint. Directly the honest-empty-state fix. |
| Encryption: SQLCipher functional + encrypt JSONL + vault unlock (T1/T7) | line 548 | Phased "End-to-End Encryption" item. |
| Delete leaves JSONL / forget-completely decision (T4) | line 720 | Open decision; note the *source-transcript* re-ingest angle (§3D T4) is the sharper risk and isn't captured there. |
| Bounded LRU session cache / 20KB payload cliff (P2/P10) | line 661 | Chunked backfill + LRU. |
| Virtualizer layout shift on expand (L-adjacent) | line 420 | Tracked (Live path). |
| Streaming record pagination (Live path, near P3) | line 648 | `streamSessionRecords` exists for Live; E2E fixture gap. **Explore still unpaginated (L1/P3) — not tracked.** |
| NATS token in logs/URL (trust-adjacent) | lines 479, 533 | Redaction hotfix + `.creds` files. |

**Net-new (not in the backlog — the report's original contributions):**
- **Truth-in-UI cold-boot fixes** — default status `connecting` not `disconnected` (L2); first-run *empty-install* onboarding distinct from the replay-window hint (L4).
- **Findability suite** — ~~global cmd-K palette~~ **shipped by the UI loop (`ce25e81`), but nav-only** → remaining ask is wiring ⌘K/search to the **FTS content** backend + the unused `project=`/`days=`/`record_type` filters (S4/S5), event-precise result landing (S6), and honest "full-text" framing + syntax hints + input sanitization (S3/S9). **Insights tab** over the stranded `/api/insights/*` endpoints (S1) — *not* covered by the shipped Overview dashboard (that's session-metadata, not analytics). `#/project/<id>` route (S7) partially softened by Overview facets.
- **Redaction at ingest** (T2) — *distinct from* the tracked encryption item; the `scrub_check.py` pattern set exists but isn't wired into the pipeline. Highest-leverage trust fix.
- **Consent-gated publishing** (T3) — default-off / prompt on first `nats_leaf_url`; pre-publish secret scan; redacted-shape mode.
- **Fail-safe binding** (T5) — token auto-mint/refuse on non-loopback bind.
- **Boot front-door consolidation** (B1–B6) — one command + wizard on the clone path; cross-platform prereq auto-install; ask-before-killing-ports.
- **Explore virtualization + keyset pagination** (L1/P3), **de-N+1 + hot indexes** (P5/P6/P7), **connection pool** (P4), **batch pattern writer** (P8), **retention knob** (P10) — the read-side scale spine beyond the already-designed boot-replay fix.
- **Skeleton loading system** (L6) — CLS elimination, distinct from the virtualizer-shift item.

**Rough effort (T-shirt) for the net-new roadmap:**
- **S** (hours–1 day): default `connecting` status; 0.0.0.0-bind warning; announce backfill horizon; skeletons; honest search framing + input sanitization; event-precise landing.
- **M** (days): first-run empty states; cmd-K + header search; wire search filters; Insights tab; UI delete/export buttons; de-N+1 + missing indexes; Explore virtualization + keyset `/records`; boot front-door + wizard-on-clone.
- **L** (weeks, design-first): redaction at ingest; consent-gated publishing; connection pool; real network-wide delete/tombstones; encryption-that-can't-lie end to end (overlaps tracked line 548).

---

*Loop complete (v4 = final; v4 added the coordination refresh against the concurrent UI loop's shipped work). This report is the deliverable — no code was changed. Recommended entry points for action, now that the UI loop has closed the navigation/session-browsing gaps: (1) **Horizon 0 truth-in-UI fixes** (§5) — almost all "S", low-risk, and the UI loop's own Review #1 already agrees the app "tells falsehoods" on boot; (2) hand **Explore virtualization + `/records` pagination** (L1/P3) to the UI loop since they own `SessionTimeline.tsx`; (3) anchor the scale track on the already-designed boot-replay fix (BACKLOG:126); (4) the **trust/secrets** dimension (redaction-at-ingest, consent-gated publishing, fail-safe binding) is the one no UI loop can reach and the one this persona will judge hardest — it deserves its own owner.*
