#!/usr/bin/env bash
# demo-what-it-is.sh — narrated, agent-driven POSITIONING demo of the
# OpenStory dashboard: proves the marketing site's "What it is" section,
# beat by beat, on real data sitting in this exact store. Ad-spot energy,
# under 90 seconds. Modeled on demo-katie-loop.sh's structure and control
# seam; heavily compressed and restructured for pace + visual motion at
# Max's request mid-build.
#
# The pitch this demo proves ("open-source agent history", six words):
#   Capture — your own history, as it happens
#   Store   — the records where you want them
#   See     — what's being written live, now
#   Reflect — on & reference what happened yesterday
#   Individuals — see into their own agentic work
#   Teams       — work from a common history
#
# Approx. timeline (for an external Playwright camera to grab a frame per
# beat; real-world curl/say startup latency can push this +/- a few
# seconds, but not near the 90s ceiling):
#   t≈0   cold open — Live tab
#   t≈6   capture + see — Live tab, THIS session streaming in
#   t≈17  canvas / store — Canvas, sunburst, grouped by user (the fleet, at scale)
#   t≈23  canvas / individuals + teams — Canvas switches to board mode (motion)
#   t≈33  reflect — focus_event zoom to a real "check OpenStory" ask (money beat)
#   t≈47  close — the six words again
#   t≈54  end (estimated)
#
# Runtime estimate: 143 narration words / 3.1 words-per-sec (say -r 190) ≈
# 46s of speech, + 6 stops x ~1s pre/post pause ≈ 6s, + ~2s of curl/network
# overhead across ~9 control posts ≈ 54s total. Comfortably under the 90s
# hard limit Max set mid-build (see git log for the two refinement asks:
# tighten to <90s, then push the visual weight onto Canvas).
#
# Provenance (read before editing): every session/event id and every UI
# control value below was verified live — against this hub's API for the
# data, against ui-control.ts / canvas-modes.ts for the seam — before being
# written into this script. Nothing is invented.
#   - Capture/See: session 917baaad-5bf7-4e16-9329-e0f2331724dd (host
#     Maxs-Air, project openstory-deploy) was `status: ongoing`, events
#     landing minutes old at verification time — it is, in fact, the very
#     session that built this script. Not staged; the product working on
#     itself.
#   - Store / Canvas: GET /api/sessions returned `total: 1892`, spanning
#     2026-02-20 through the day of verification (~5 months), across 7
#     hosts (Maxs-Air, Maxs-Air-ht-home, Maxs-MacBook-Air, a1,
#     Katies-Mac-mini, two containers). GET /api/admin/topology confirms
#     `shape: solo` — every one of those is a local SQLite file on
#     somebody's own machine. Canvas's own default state already renders
#     this: DEFAULT_CANVAS_MODE is "sunburst" ("the strongest first
#     impression" per its own source comment) and the canvas's default
#     groupBy is "user" (ui/src/components/canvas/SessionsCanvas.tsx) — so
#     `open_view canvas` alone already opens on "every person's sessions,
#     radial, at scale." `canvas.mode=board` and `canvas.groupBy=user` are
#     both real, wired toggle values (CANVAS_MODES / DIMS in
#     ui/src/lib/canvas-modes.ts and SessionsCanvas.tsx), not invented ones.
#   - Reflect: session 6f88157e-9914-4b42-9721-7788fb37def0 (host a1).
#     Verbatim, seq 2: "Find my most recent session in OpenStory on
#     architecture and tell me where we're at please :)" — and seq 7, the
#     assistant's reply: "Found it — your most recent architecture session
#     is `80d15523` from yesterday ... 662 events ... Running sessionstory
#     on it." That reply lands on the word "yesterday" unprompted — the
#     positioning copy proving itself. (Other real candidates exist and
#     were checked too: 5a3d1801-9804-4f1e-a18d-95cab87122d3 seq 2 "Would
#     you check openstory for my most common prompts..." and
#     628b1813-6a57-4706-956d-ffe8f17a3cb2 seq 2/21. This one was chosen
#     for being shortest, cleanest, and for that verbatim "yesterday.")
#   - Individuals/Teams (folded into the Canvas beat for pace): GET
#     /api/users returns a real per-person rollup array (hosts, projects,
#     activity_24h, last_active) backing the Users view, and Canvas grouped
#     by user is that same multi-person data rendered as one board.
#     Session ac9cf839-7b94-48e2-87e3-279fa8634f76 (host Katies-Mac-mini,
#     user katie, project raptor-agentic-team) and Max's sessions live in
#     this same store — that's the honest claim ("one common history"),
#     not that anyone searched a teammate's session (see
#     demo-katie-loop.sh's own provenance note on this exact session for
#     the same honesty check). That fuller Katie story isn't retold here;
#     this demo only needs the one-line proof that the store is shared.
#
# Drives the UI entirely through the control seam (POST JSON to
# /api/control) — the same write path any MCP/operator agent uses — and
# narrates over `say`. Nothing here touches the DOM directly; the
# dashboard stays a pure sink reacting to `control` messages broadcast
# over its WebSocket. See ui/src/lib/ui-control.ts for the control
# vocabulary this drives.
#
# Usage:
#   ./scripts/demo-what-it-is.sh              # full narrated demo
#   ./scripts/demo-what-it-is.sh --fast       # skip pacing sleeps (for testing)
#   ./scripts/demo-what-it-is.sh --silent     # skip narration (banners/toggles only)
#   OS_TOUR_VOICE=Ava ./scripts/demo-what-it-is.sh
#
# Env overrides:
#   OS_TOUR_VOICE               — `say` voice name           (default: Serena (Premium))
#   OS_TOUR_API                 — control endpoint            (default: local :3002)
#   OS_TOUR_LIVE_SID            — Capture/See focal session id
#   OS_TOUR_REFLECT_SID         — the Reflect session id
#   OS_TOUR_EVT_REFLECT_ASK     — event id: the real ask (the zoom target)
#   OS_TOUR_EVT_REFLECT_ANSWER  — event id: the "yesterday" reply (referenced
#                                  in narration; not separately navigated to,
#                                  to keep this beat to one zoom under budget)

set -uo pipefail

VOICE="${OS_TOUR_VOICE:-Serena (Premium)}"
B="${OS_TOUR_API:-http://127.0.0.1:3002/api/control}"
ISSUER="what-is-it"

# Real ids, verified against this hub's API — see provenance note above.
SID_LIVE="${OS_TOUR_LIVE_SID:-917baaad-5bf7-4e16-9329-e0f2331724dd}"
SID_REFLECT="${OS_TOUR_REFLECT_SID:-6f88157e-9914-4b42-9721-7788fb37def0}"
E_REFLECT_ASK="${OS_TOUR_EVT_REFLECT_ASK:-82cebccf-b4c3-4854-a593-530c224e4a35}"
E_REFLECT_ANSWER="${OS_TOUR_EVT_REFLECT_ANSWER:-6d89c81d-df7f-47a3-82ba-aa64ee18c644}"

FAST=0
SILENT=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --silent) SILENT=1 ;;
    -h|--help)
      sed -n '2,88p' "$0"
      exit 0
      ;;
    *)
      echo "demo-what-it-is.sh: unknown flag '$arg' (use --fast / --silent)" >&2
      exit 1
      ;;
  esac
done

# post <json-body> — fire a control action at the dashboard. Best-effort:
# a dropped stop shouldn't kill a live demo, so failures are logged, not fatal.
post() {
  if ! curl -sS -m 5 -o /dev/null -X POST "$B" \
    -H 'Content-Type: application/json' \
    -d "$1"; then
    echo "demo-what-it-is: post failed (is the dashboard/server up at $B ?)" >&2
  fi
}

SEG=0
# talk <text> — BLOCKING narration; this is what paces the whole demo.
# Rate is bumped to 190 (from the 178 other demo scripts use) to hold this
# one to its <90s hard budget — ad-spot pace, not documentary pace.
# Video mode: when OS_TOUR_AUDIO_DIR is set (see record-tour-video.sh), each
# line is rendered to an aiff, its start offset (vs OS_TOUR_T0) logged, then
# played — so the video muxer can rebuild a clean narration track instead of
# recording a microphone.
talk() {
  [ "$SILENT" -eq 1 ] && return 0
  if [ -n "${OS_TOUR_AUDIO_DIR:-}" ]; then
    SEG=$((SEG+1))
    local f="$OS_TOUR_AUDIO_DIR/seg-$(printf '%03d' "$SEG").aiff"
    /usr/bin/say -v "$VOICE" -r 190 -o "$f" "$1"
    python3 -c "import time; print(round(time.time()-${OS_TOUR_T0:-0},3))" >> "$OS_TOUR_AUDIO_DIR/offsets.log"
    echo "$f" >> "$OS_TOUR_AUDIO_DIR/files.log"
    afplay "$f"
    return 0
  fi
  /usr/bin/say -v "$VOICE" -r 190 "$1"
}

# pause <seconds> — skipped entirely under --fast.
pause() {
  [ "$FAST" -eq 1 ] && return 0
  sleep "$1"
}

open_view() {
  # open_view <view> [sessionId] [eventId] — eventId deep-links Explore (or
  # Story) straight to a single recorded moment inside that session.
  local view="$1" sid="${2:-}" eid="${3:-}"
  if [ -n "$sid" ] && [ -n "$eid" ]; then
    post "{\"action\":\"open_view\",\"params\":{\"view\":\"${view}\",\"sessionId\":\"${sid}\",\"eventId\":\"${eid}\"},\"issuer\":\"${ISSUER}\"}"
  elif [ -n "$sid" ]; then
    post "{\"action\":\"open_view\",\"params\":{\"view\":\"${view}\",\"sessionId\":\"${sid}\"},\"issuer\":\"${ISSUER}\"}"
  else
    post "{\"action\":\"open_view\",\"params\":{\"view\":\"${view}\"},\"issuer\":\"${ISSUER}\"}"
  fi
}

focus_event() {
  # focus_event <sessionId> <eventId> [view] — zoom the Events view straight
  # to one exact recorded moment; lands zoomed and expanded on that event.
  # Distinct control verb from open_view's eventId-carrying form (same
  # underlying route, but the intent-revealing verb an agent should reach
  # for when the whole point of the stop is one exact moment).
  local sid="$1" eid="$2" view="${3:-explore}"
  post "{\"action\":\"focus_event\",\"params\":{\"sessionId\":\"${sid}\",\"eventId\":\"${eid}\",\"view\":\"${view}\"},\"issuer\":\"${ISSUER}\"}"
}

toggle() {
  # toggle <target> <value>
  post "{\"action\":\"toggle\",\"params\":{\"target\":\"${1}\",\"value\":\"${2}\"},\"issuer\":\"${ISSUER}\"}"
}

banner() {
  # banner <message> — the human-facing "present" call. Kept short (<=160 chars).
  post "{\"action\":\"present\",\"params\":{\"message\":\"${1}\"},\"issuer\":\"${ISSUER}\"}"
}

# stop <banner-text> <narration-text> — call AFTER issuing this stop's
# navigate/toggle action(s). Short pre/post pauses (<=1s combined "settle"
# either side) keep this demo inside its 90s budget — no long documentary
# settles.
stop() {
  banner "$1"
  pause 0.4
  talk "$2"
  pause 0.6
}

echo "demo-what-it-is: driving ${B} as issuer '${ISSUER}'"
[ "$FAST" -eq 1 ] && echo "demo-what-it-is: --fast (no pacing sleeps)"
[ "$SILENT" -eq 1 ] && echo "demo-what-it-is: --silent (no narration)"

# ---------------------------------------------------------------------------
# t≈0 — Cold open: the six words this demo is going to prove
# ---------------------------------------------------------------------------
open_view live
stop \
  "OPEN-SOURCE AGENT HISTORY — six words, proven live." \
  "Six words: capture, store, see, reflect, individuals, teams. This is open-source agent history — watch it prove itself."

# ---------------------------------------------------------------------------
# t≈6 — Capture + See: this very session, streaming in as we watch
# ---------------------------------------------------------------------------
open_view live "$SID_LIVE"
stop \
  "CAPTURE & SEE — your own history, as it happens; what's being written live, now." \
  "Capture: your own history, as it happens. See: what's being written live, now. This is that same session, right now, writing this demo into the record as we watch."

# ---------------------------------------------------------------------------
# t≈17 — Canvas / Store: the fleet, at scale, the strongest screen in the app
# ---------------------------------------------------------------------------
open_view canvas
pause 0.3
toggle canvas.groupBy user
stop \
  "STORE — the records where you want them." \
  "Store: the records where you want them — nineteen hundred sessions, seven machines, all local, all yours."

# ---------------------------------------------------------------------------
# t≈23 — Canvas / Individuals + Teams: same board, switched to bubbles — motion
# ---------------------------------------------------------------------------
toggle canvas.mode board
stop \
  "INDIVIDUALS & TEAMS — every person's history, one common board." \
  "This is every person's history — Max, Katie, all of it — on one board. Individuals see into their own agentic work. Teams work from a common history."

# ---------------------------------------------------------------------------
# t≈33 — Reflect: the money beat — a real ask, zoomed to the exact moment
# ---------------------------------------------------------------------------
focus_event "$SID_REFLECT" "$E_REFLECT_ASK"
stop \
  "REFLECT — on & reference what happened yesterday." \
  "Reflect: on and reference what happened yesterday. Weeks ago, someone typed: find my most recent session in OpenStory on architecture and tell me where we're at. The reply, seconds later: found it — from yesterday, six hundred sixty two events."

# ---------------------------------------------------------------------------
# t≈47 — Close: the six words again
# ---------------------------------------------------------------------------
banner "Capture. Store. See. Reflect. Individuals. Teams. Built in the open."
pause 0.4
talk "Capture, store, see, reflect, individuals, teams. Open-source agent history — built in the open. Thanks for watching."
pause 0.6

banner "Demo complete. Take the wheel back any time, just click or navigate."
echo "demo-what-it-is: done."
