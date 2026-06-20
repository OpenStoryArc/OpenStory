# Federation & Multi-Agent — Slice Decomposition

**Status:** planning / cut-list
**Date:** 2026-06-16
**Purpose:** Break the tangled federation + multi-agent + shape work into independent,
shippable slices. Map each open branch to a slice with an honest *built vs. sketched*
status, so there's a cut-list to work down instead of a pile to stare at.

The animating question: *how do I deliver clear value in pieces, and am I
over-working the permissions problem?* Short answer to the second part: **yes** — the
permissions energy went into a NATS-account/role rabbit hole (`feat/federation-host-in-subject`,
98 commits, "Phase 6.3–6.9") when the permission boundary you can actually test and reason
about is the authenticated HTTP/WS API you already own. See §4.

---

## 1. The slices

Each slice delivers value *standalone*. The marker is whether it's independent or stacks.

| Slice | What it is | Independent? | Value alone | Complexity |
|-------|-----------|--------------|-------------|------------|
| **A — Identity** | host + user + agent stamp on every event | foundation | filter *your own* work by machine/agent | Low — mostly done |
| **B — Multi-agent ingest** | Codex (and beyond) as a first-class translator | yes | "watch all my agents in one pane," no network | Low–Med |
| **C — Read federation** | scoped read tokens + stream filtering on the existing API | needs A | see a teammate's sessions, each owns their data | Med |
| **D — Shape** | derived "shape" data + live RxJS panel | fully orthogonal | new visualization, ships on one node | Med — built |
| **E — Health** | `/api/health` self-report (every layer reports what it thinks is true) | orthogonal | makes A/C debuggable from one curl | Low |

### How they build to a point

```
A (identity) ──► C (read federation, app-auth) ──┐
B (codex translator) ────────────────────────────┼──► THE POINT:
                                                  │   multi-person, multi-agent,
E (health) makes A+C observable ─────────────────┘   multi-machine session awareness,
                                                      secured at the app layer
D (shape) — orthogonal, ships anytime ──► amplifies the climax visually
```

The climax capability (A + B + C) is the demo and the YC story. **The NATS-account
permissions work is *not* on the critical path to it.**

---

## 2. Branch → slice map (with real status)

Numbers are `behind / ahead` of `master` as of 2026-06-16.

### `feat/codex-host-stamping` — **20 behind / 1 ahead** — Slices A + B ✅ BUILT
One squashed commit: *"Codex support + host/user stamping + session recency fix"* (209 files,
+11k/-3k). This is the gem. It delivers **identity (A) and multi-agent ingest (B) together**,
and it's a clean single commit rather than the sprawl of the federation branch.
- **Built:** host/user stamping, Codex ingestion, `origin-agent` UI parsing + tests, session recency fix.
- **Status:** 20 behind master → needs a rebase, then it's shippable on its own.
- **Action:** **rebase onto master and ship first.** This is the cut-to-the-chase win.
- **Risk:** 209 files in one commit is a lot to review; consider splitting into
  `core: codex translator` + `server/ui: host stamping` if review friction is high.

### `feat/federation-host-in-subject` — **20 behind / 98 ahead** — the permissions rabbit hole ⚠️
304 files, +37k/-4k. Commits run "Phase 6.3 → 6.9": role-based directory module, role-required
gating on policy writes, per-user permission profiles, NATS conf template with per-role
permission profiles, `init-accounts-conf` subcommand, Participants admin panel, grant-role
bootstrap. **This is where the over-engineering lives.**
- **Keep:** the identity/`origin-agent` foundation — but it's *already* in `codex-host-stamping`
  in cleaner form, so harvest from there instead.
- **Shelve:** Phase 6.x (NATS account ACLs, role directory, per-role NATS conf, admin panel).
  This enforces permissions at the *transport* layer — untestable from one machine, couples
  sovereignty to NATS config correctness, and is a new primitive when the app already authenticates. See §4.
- **Action:** **do not merge as-is.** Cherry-pick nothing structural; let the Phase-6 work rest
  as "future hardening if we ever open a public, untrusted hub." 98 commits / 20 behind means it's
  already diverging — don't pour more in.

### `feat/shape-live-stream` — **1 behind / 9 ahead** — Slice D ✅ BUILT (superset of shape-layers)
Contains all of `feat/shape-layers` (the 6 build commits) **plus** live: `ShapeCounts` fold,
server broadcast of live shape counts, and a live Shapes tab. 53 files, +4.8k.
- **Built:** `open-story-shapes` crate (bash/path/change extractors), shape persistence behind
  `EventStore` (SQLite + Mongo, with conformance tests), shapes consumer (Actor 5), per-session +
  cross-shape APIs, CLI `backfill-shapes`, session ShapesPanel, **and** the live ShapesView tab.
- **Status:** nearly current (1 behind). Genuinely shippable.
- **Action:** **ship this as the standalone demo win** whenever you want momentum — it waits on
  nothing. Prefer it over `feat/shape-layers` (it's the superset). Retire `shape-layers` after.

### `feat/shape-layers` — **1 behind / 6 ahead** — Slice D (subset)
The persisted-shape + session-panel half, without live streaming. Superseded by
`feat/shape-live-stream`. **Action:** fold in / delete once the live branch lands.

### `feat/mcp-http-backend` — **0 behind / 2 ahead** — MCP/agent access ✅ MOST MERGEABLE
**Current with master**, two clean commits (15 files, +883). Switches the MCP query tools to read
through the REST API (`OPENSTORY_API_URL`) instead of direct SQLite — cwd-independent, and the
right seam for agents (including OpenStory querying itself) to consume the store.
- **Built:** `http_store.rs` (+ 349-line test), `plan_source.rs`, registration/docs switch to the
  URL model.
- **Status:** zero behind — the lowest-friction merge on the board.
- **Action:** **review + merge early.** Unblocks agent access and clears the deck.

### `feat/tailnet-federation-validation` — merged as #83 ✅ DONE
The tailnet/ACL validation harness already landed on master (`e63a2e5`). Nothing to do; listed
for completeness so it's not mistaken for open work.

---

## 3. Slice → branch coverage

| Slice | Where it lives | State |
|-------|---------------|-------|
| A — Identity | `feat/codex-host-stamping` (clean), `feat/federation-host-in-subject` (tangled) | built, needs rebase |
| B — Multi-agent ingest | `feat/codex-host-stamping` | built, needs rebase |
| C — Read federation | **nowhere yet** — the Phase-6 branch solved the *wrong layer* | **to design (the simple way)** |
| D — Shape | `feat/shape-live-stream` ⊃ `feat/shape-layers` | built, ~current |
| E — Health | backlog item (`/api/health` self-report) | sketched only |

The important gap: **C, the actual value, has no clean branch.** The effort that *should*
have been C went into transport-layer ACLs. C done simply (§4) is net-new but small.

---

## 4. The permissions verdict — enforce at the API, not the transport

`feat/federation-host-in-subject`'s Phase 6 enforces who-sees-what inside **NATS** (accounts,
subject ACLs, per-role conf). The cheaper, testable model maps to AWS IAM and reuses what exists:

- **Identity** = the host/user/agent stamp (Slice A — already in the data).
- **API boundary** = `rs/server/src/auth.rs` + the REST/WS surface (already does Bearer auth).
- **Policy** = a scoped read token: *"this token may read events where `user ∈ {…}` / `project ∈ {…}`."*
- **"Profile"** = a named scoped token a peer node uses to pull your filtered stream.

**NATS stays a dumb, fast pipe between trusted nodes; the permission boundary is each node's
authenticated HTTP/WS API.** That you can curl, unit-test, and reason about from one machine —
all three things the NATS-account path fails at (the last being a standing backlog complaint).

You only need NATS-account ACLs if untrusted parties connect to a shared hub *directly by NATS*.
Today's hub is you + Katie + a1 — trusted. Subject-level ACLs there are airport security for your
own living room.

The one real piece that doesn't vanish: **filtering the WS broadcast by the token's scope** so a
peer only *receives* authorized events (`rs/server/src/ws.rs` / broadcast consumer). Bounded and
testable — an order of magnitude smaller than account ACLs.

---

## 5. Recommended cut-list

1. **Merge `feat/mcp-http-backend`** (0 behind, clean) — clears the deck, unblocks agent access.
2. **Rebase + ship `feat/codex-host-stamping`** — lands Slices A + B (identity + multi-agent).
   Split into core/server-ui commits if review friction is high.
3. **Add Slice E** (`/api/health`) — cheap, and you'll want it the moment you debug federation
   again (e.g. *why a1 went dark on 2026-05-31*).
4. **Design + build Slice C the simple way** (§4): scoped read tokens + WS stream filtering on the
   existing API. This is where the permissions energy goes — and where it *stops*.
5. **Ship `feat/shape-live-stream`** (Slice D) whenever a morale/demo win is wanted — it waits on
   nothing.
6. **Formally shelve** `feat/federation-host-in-subject`'s Phase-6 work as "future hardening for an
   untrusted public hub." Don't merge; don't extend.

---

## Appendix — open question surfaced during this audit

`a1` (Max's Linux/5090 federation testbed) streamed nothing after **2026-05-31 18:00**. Whether
the leaf is still connected or the federation path silently dropped is exactly the kind of thing
Slice E would answer with one curl. Worth a look independent of the slice work.
