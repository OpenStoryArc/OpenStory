# Bounded Read-Through Projection — open-story memory design

**Date:** 2026-07-10
**Status:** design / awaiting review
**Component:** `rs/server` (state, projection, api), `rs/store`

## Problem

open-story holds the **entire projected history hot in RAM**. Measured on the
production hub (2026-07-10): **~3.18 GB for 524,596 events / ~2,300 sessions**, and
it only grows — `retention_days = 0` (keep forever) + the read model is rebuilt for
*every* session on boot and never evicted. Raising the container limit 2 GB → 4 GB
bought runway, not a fix; RAM scales with total event volume indefinitely.

Root cause (verified in code):
- `StoreState.projections: DashMap<SessionId, SessionProjection>` holds one full
  projection per session; each `SessionProjection.records: Vec<ViewRecord>` carries
  every event's content. Nothing is ever evicted.
- `full_payloads` is an **unbounded** overflow cache for tool-result bodies >100 KB.
- `reproject_all` eagerly rebuilds all projections at boot.

## Principles (hard constraints)

1. **Data preservation is sacred.** SQLite is the complete, durable truth and is
   **never** truncated destructively (verified: `insert_event` stores the full
   `serde_json::to_string(event)`). Display truncation that is **expandable to the
   full body** is fine. Bound RAM via **eviction + lazy re-derivation**, never by
   dropping data. See memory `openstory-no-truncation-principle`.
2. **Preserve the live-view richness.** Recent / live content — big files, code,
   tool output — must render **in full, inline**, not click-to-expand. Only *cold,
   old* history may be demoted.

## Design: recency-windowed bounded read-through

Two independent axes were being conflated: **recency** (hot vs cold) and
**completeness** (full vs preview). We tie completeness to recency.

### Hot working set — fully resident (no behavior change)
The Live tab (currently-streaming sessions) plus any session accessed within the
recency window keep their **full** `SessionProjection` — full content, rich inline.
Nothing you are actually looking at changes.

### Cold sessions — evicted, lazily rebuilt
Sessions outside the window are **evicted** from `projections` (RAM freed). On next
access they are **rebuilt from SQLite**: read that one session's events
(`session_events`, indexed on `(session_id, timestamp)`) and replay through
`SessionProjection::append()` — the same per-session work `reproject_all` already
does, just on demand. Cost: single-digit-to-tens of ms for a typical session.
Lossless: the projection is pure derivation, re-derived identically from the log.
**Never evict a live/streaming session.**

### Full bodies — bounded read-through cache
`full_payloads` (unbounded `DashMap`) → **byte-bounded LRU**. The existing
`GET full_payload` handler already checks RAM then falls back to
`event_store.full_payload(id)` (SQLite) — so bounding the cache means evicted entries
simply re-fetch on demand; **no new miss path required**. Expand always returns the
complete body (RAM hit or SQLite).

### Lazy boot
Reproject only the recent working set at startup; cold sessions rebuild on access.
Bounds startup RAM and shortens boot (currently ~18 s reconciling everything).

### Bounds (operator knobs, byte-first for a predictable ceiling)
- **Primary bound — `projection_cache_bytes`:** a total-bytes ceiling on resident
  `projections`. **Eviction order = least-recently-accessed cold session** (LRU by
  last access). Live/streaming sessions are **pinned** (never counted for eviction).
  Set generously — the box can run open-story at 6–8 GB — so the hot set is broad and
  only deep history pays.
- **Secondary guard (optional) — `working_set_days`:** never evict a session touched
  within this window even if under byte pressure (protects "today's work"); `0` = off.
- **Payload cache — `payload_cache_bytes`:** byte cap on the full-body LRU (default 256 MB).
- Steady-state RAM ≈ `projection_cache_bytes + payload_cache_bytes` — a number you set.

## Data-model / code changes (`rs/server`, `rs/store`)

- `projections` → recency/LRU-governed map: track last-access; evict least-recently-
  accessed **cold** session when over the working-set bound; pin live sessions.
- `full_payloads` → byte-bounded LRU.
- Touch recency on session read paths (API handlers).
- `SessionProjection::rebuild(session_id)` helper (extract the per-session body of
  `reproject_all`); call it on access-miss.
- Lazy boot: `reproject_all` variant that seeds only the working set.
- Config: `working_set_days` / `max_resident_sessions` / `projection_cache_bytes`,
  `payload_cache_bytes`. Keep the inline display cap (expandable) — it is compatible.

## Preservation guarantees

- SQLite unchanged: full events, no destructive truncation.
- Every eviction discards **derived cache only**; every access re-derives identically.
- Expanding any event returns the complete body from RAM or SQLite.

## Out of scope (future, separate decisions)

- **Workstream A — prune openactor** (339 regenerable eval sessions, ~226k events /
  42% of count). Tactical cleanup; destructive; gated on explicit confirm + backup.
  Reduces structural pressure but is *not* the root fix. Tracked separately.
- `retention_days` pruning of the **durable** store — a data-lifecycle policy, distinct
  from this in-RAM bounding.
- Shape-1 "uniform preview everywhere" — **rejected**: it would degrade the live view.

## Validation

Restore a copy of the `os-data` volume into a scratch open-story container and:
- measure RSS before/after; confirm it lands near the configured ceiling and stays flat
  as history grows;
- UI pass: Live view renders big files/code/output **in full inline**; timelines/lists
  instant; expand works; a **cold** old session opens and rebuilds to an identical view;
- confirm boot-time improvement and that live/streaming sessions are never evicted.

## Rollout

Branch in OpenStory → build+publish via the established `release.yml` (or box build) →
deploy to the hub with an `os-data` backup and rollback (re-pin the prior image sha),
same pipeline used for the `sha-4b337e5` deploy.

## Risks & mitigations

- **Cold-open latency** → one session, indexed, ms-scale; generous window keeps it rare.
- **Evicting an in-use session** → never evict live/streaming; pin the hot set; recency
  touch on every access.
- **Concurrency** (DashMap + eviction races) → careful lock discipline; evict outside
  read locks.
- **Under-sized window churn** (thrash rebuilding) → default the window generously; make
  it a tunable; add an eviction-rate metric.
