# Agent-Directed Watch Focus — Design Spec

**Status:** Approved (brainstorming → spec)
**Date:** 2026-06-25
**Branch:** `feat/agent-directed-watch-focus`

## Problem

When you ask the agent to "watch session X," there is no live path from the
agent to the UI. The agent (in this CLI) is **turn-based**: it can open an MCP
`subscribe_session` stream, but the resulting `notifications/openstory/stream`
pushes land *between* its turns and are dropped. In practice the agent's "watch"
degrades to polling REST on demand — it misses live bursts (verified: a 12-event
burst on a federated a1 session landed silently; the agent only reconstructed it
later by polling `/api/sessions/.../records`).

The UI, however, is **already a live surface**: `ui/src/streams/connection.ts`
holds an auto-reconnecting WebSocket to `/ws`, fed by the broadcast consumer off
NATS (including federated events). The UI can consume the push the agent cannot.

**So the agent shouldn't be the live tail — it should aim the one that already
exists.** This feature lets the agent point the already-live UI at a session.

## Soul alignment

- **Observe, never interfere (the source):** unchanged. This adds no write path
  to any agent or transcript. It only nudges *our own UI*, and only with the
  user's prior consent (see UI reception).
- **Reuse existing dataflow:** no new transport. The focus signal rides the
  existing `state.broadcast_tx → /ws` channel that the UI already drinks from.
- **Minimal, honest code:** one new endpoint, one new message variant, one pure
  reducer, one small banner component.

## Decisions (locked in brainstorming)

1. **Trigger = REST endpoint.** `POST /api/watch/{session_id}` on the server
   emits a focus message over the existing `/ws` broadcast. The agent fires it
   with `curl`. An MCP tool wrapper is explicitly **out of scope** (future
   follow-up; same contract).
2. **UI reception = context-aware.** If the UI is already on the **Live** tab,
   it switches focus to the session **instantly**. If it is on any other tab, it
   shows a **dismissible banner** (`Agent is watching <session> — Follow →`)
   that navigates on click. Never yanks the user off another tab unasked.

## Architecture & data flow

```
you (chat) ──ask──▶ agent
   agent ── curl POST /api/watch/{session_id} ──▶ server
                                                    │ look up session metadata (enrich)
                                                    │ 404 if unknown
                                                    ▼
                       state.broadcast_tx.send(BroadcastMessage::Focus{…})   ← existing channel
                                                    │
                                              /ws (already live)
                                                    ▼
                  connection.ts → message$ → focus handler in App
                                                    │
                         on Live tab  → navigate live/<session>  (instant)
                         elsewhere    → WatchBanner "Follow →"
```

The endpoint returns `{ status: "focusing", session_id, delivered_to: <n> }`
where `delivered_to` is the count of connected WebSocket subscribers
(`broadcast_tx.receiver_count()`), so the agent can honestly report
"no UI open" (`0`) vs "focused on N tabs."

## Components

### Backend (`rs/server`)
- **`broadcast.rs`** — new variant on `BroadcastMessage` (a `#[serde(tag =
  "kind")]` enum):
  ```rust
  #[serde(rename = "focus")]
  Focus {
      session_id: String,
      #[serde(skip_serializing_if = "Option::is_none")] label: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")] project_name: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")] host: Option<String>,
      #[serde(skip_serializing_if = "Option::is_none")] user: Option<String>,
  },
  ```
- **`api.rs`** — new handler `watch_session(State(state), Path(session_id))`:
  1. Look up the session summary from the store; if absent, return `404`.
  2. Build `BroadcastMessage::Focus` enriched with label/project/host/user.
  3. `let n = state.broadcast_tx.send(msg).map(|n| n).unwrap_or(0);`
     (a send error means zero receivers — report `delivered_to: 0`, still `200`).
  4. Return `Json({ status, session_id, delivered_to: n })`.
- **`router.rs`** — `.route("/api/watch/{session_id}", post(api::watch_session))`.
  Sits under the existing `/api` tree, so the `api_token` auth middleware already
  covers it. No new middleware.

### Frontend (`ui/src`)
- **`types/websocket.ts`** — add `FocusMessage { kind: "focus"; session_id;
  label?; project_name?; host?; user? }` to the `WsMessage` union.
- **`lib/watch-focus.ts`** *(new, pure — the testable core)* —
  `decideFocusAction(currentView: ViewMode, msg: FocusMessage): FocusAction`
  where `FocusAction = { type: "navigate"; sessionId } | { type: "banner"; msg }`.
  Rule: `currentView === "live"` → navigate; else → banner.
- **`components/WatchBanner.tsx`** *(new)* — dismissible banner with a
  `Follow →` button. Renders the enriched label/project/host/user. On Follow,
  navigates to `live/<session>` and clears itself. On Dismiss, clears.
- **`App.tsx`** — subscribe to `message$` filtered to `kind === "focus"`; run
  `decideFocusAction`; either call the existing hash-route navigation
  (`buildHash({ view: "live", sessionId })`) or set banner state.

## Error handling

| Case | Behavior |
|------|----------|
| Unknown `session_id` | `404` — honest signal. Federated sessions *are* in the store, so this is real validation, not a false negative. |
| No UI connected | `200` with `delivered_to: 0`. Agent tells the user to open the UI. |
| Multiple UIs/tabs | All receive the focus; each reacts per its own current view. `delivered_to` = total. |
| UI connects *after* the signal | **Known v1 limitation:** focus is fire-and-forget; a tab connecting later won't catch it. Out of scope. Future fix: stash last-focus in `AppState`, replay in the `initial_state` handshake. |

## Testing (BDD/TDD)

- **Rust integration** (`rs/tests/`, helpers `test_state()` + `send_request()`):
  - subscribe to `broadcast_tx`; `POST /api/watch/{known_id}`; assert a `Focus`
    message arrives with the right `session_id` and enrichment; assert response
    `delivered_to == 1`.
  - `POST /api/watch/{unknown_id}` → `404`.
  - `POST` with no subscribers → `200`, `delivered_to == 0`.
- **UI unit** (`ui/tests/`, `scenario(given, when, then)` from `bdd.ts`):
  - `decideFocusAction("live", msg)` → `{ type: "navigate" }`.
  - `decideFocusAction("explore", msg)` → `{ type: "banner" }`.
  - `WsMessage` parse: a `{ kind: "focus", … }` frame deserializes to
    `FocusMessage`.
- **E2E** (optional, later): Playwright opens the Live tab, `POST /api/watch`,
  asserts the focused session switches.

## Scope

**In:** single focused session (latest-wins); REST trigger; context-aware UI
reception; enriched focus message; `delivered_to` receiver count; tests.

**Out (YAGNI / later):** MCP tool wrapper; the 2×2 command-center Watch tab
(separate feature — wireframes in `docs/research/watch-command-center/`);
reconnect persistence/replay; multi-session focus.

## Implementation order

1. Backend: `Focus` variant → handler → route, with Rust integration tests
   (red→green).
2. Frontend: `FocusMessage` type → `decideFocusAction` pure reducer + unit specs
   → `WatchBanner` → wire into `App.tsx`.
3. Manual verify: `curl POST /api/watch/<a1 session>` with the UI open on Live
   (instant) and on Explore (banner).
