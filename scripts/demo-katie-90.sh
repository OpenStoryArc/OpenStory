#!/usr/bin/env bash
# demo-katie-90.sh — the 90-second cut of the Katie loop-engineering story,
# told through the Event Spotlight (presentation mode): each stop fills the
# screen with ONE real recorded event, dims everything else, and narrates one
# line over it. Modeled exactly on demo-katie-loop.sh's structure and helpers;
# the spotlight is driven through the same control seam via
# focus_event {spotlight:true} (see ui/src/lib/ui-control.ts).
#
# Stop timeline (~90s total, pauses ~2s between stops):
#   t≈0   spotlight E_ASK      — Katie's question (2026-07-06 14:59:27)
#   t≈20  spotlight E_RECAP    — the recall, 84s later (2026-07-06 15:00:51)
#   t≈40  spotlight E_PRSTATUS — the pull request, 39min later (2026-07-06 15:39:03)
#   t≈55  spotlight E_MERGED   — merged (2026-07-06 15:45:12)
#   t≈70  (still on the merge spotlight) — the closer.
#
# Provenance: these four event ids were verified live against the store on
# 2026-07-14 — all four resolve inside session ac9cf839-7b94-48e2-87e3-279fa8634f76
# (host Katies-Mac-mini, user katie), with timestamps 14:59:27 / 15:00:51 /
# 15:39:03 / 15:45:12 on 2026-07-06. See demo-katie-loop.sh's provenance note
# for the fuller story behind the session itself.
#
# Usage:
#   ./scripts/demo-katie-90.sh              # full narrated demo
#   ./scripts/demo-katie-90.sh --fast       # skip pacing sleeps (for testing)
#   ./scripts/demo-katie-90.sh --silent     # skip narration (spotlights only)
#   OS_TOUR_VOICE=Ava ./scripts/demo-katie-90.sh
#
# Env overrides:
#   OS_TOUR_VOICE        — `say` voice name                (default: Serena (Premium))
#   OS_TOUR_KATIE_SID     — Katie's real session id          (default: the session above)
#   OS_TOUR_API          — control endpoint                 (default: local :3002)
#   OS_TOUR_EVT_ASK       — event id: Katie's question
#   OS_TOUR_EVT_RECAP     — event id: the research recap
#   OS_TOUR_EVT_PRSTATUS  — event id: PR #22 status check
#   OS_TOUR_EVT_MERGED    — event id: merge confirmation

set -uo pipefail

VOICE="${OS_TOUR_VOICE:-Serena (Premium)}"
SID="${OS_TOUR_KATIE_SID:-ac9cf839-7b94-48e2-87e3-279fa8634f76}"
B="${OS_TOUR_API:-http://127.0.0.1:3002/api/control}"
ISSUER="katie-90"

E_ASK="${OS_TOUR_EVT_ASK:-dc3f5ece-b9f2-4dae-a1b7-91e973012a8a}"
E_RECAP="${OS_TOUR_EVT_RECAP:-aeaf5006-78e4-4c07-a4c8-605335583b14}"
E_PRSTATUS="${OS_TOUR_EVT_PRSTATUS:-a340c2ae-c7d1-47f9-aba6-b1a0f413486f}"
E_MERGED="${OS_TOUR_EVT_MERGED:-70400985-7080-41a0-ac91-aa37158b8094}"

FAST=0
SILENT=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --silent) SILENT=1 ;;
    -h|--help)
      sed -n '2,36p' "$0"
      exit 0
      ;;
    *)
      echo "demo-katie-90.sh: unknown flag '$arg' (use --fast / --silent)" >&2
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
    echo "demo-katie-90: post failed (is the dashboard/server up at $B ?)" >&2
  fi
}

SEG=0
# talk <text> — BLOCKING narration; this is what paces the whole demo.
# Video mode: when OS_TOUR_AUDIO_DIR is set (see record-tour-video.sh), each
# line is rendered to an aiff, its start offset (vs OS_TOUR_T0) logged, then
# played — so the video muxer can rebuild a clean narration track instead of
# recording a microphone.
talk() {
  [ "$SILENT" -eq 1 ] && return 0
  if [ -n "${OS_TOUR_AUDIO_DIR:-}" ]; then
    SEG=$((SEG+1))
    local f="$OS_TOUR_AUDIO_DIR/seg-$(printf '%03d' "$SEG").aiff"
    /usr/bin/say -v "$VOICE" -r 178 -o "$f" "$1"
    python3 -c "import time; print(round(time.time()-${OS_TOUR_T0:-0},3))" >> "$OS_TOUR_AUDIO_DIR/offsets.log"
    echo "$f" >> "$OS_TOUR_AUDIO_DIR/files.log"
    afplay "$f"
    return 0
  fi
  /usr/bin/say -v "$VOICE" -r 178 "$1"
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
  # focus_event <sessionId> <eventId> — SPOTLIGHT an exact recorded moment:
  # spotlight:true upgrades the focus to presentation mode, so the one event
  # fills the screen and everything else dims (ui/src/lib/ui-control.ts).
  local sid="$1" eid="$2"
  post "{\"action\":\"focus_event\",\"params\":{\"sessionId\":\"${sid}\",\"eventId\":\"${eid}\",\"spotlight\":true},\"issuer\":\"${ISSUER}\"}"
}

echo "demo-katie-90: driving ${B} as issuer '${ISSUER}' (session ${SID})"
[ "$FAST" -eq 1 ] && echo "demo-katie-90: --fast (no pacing sleeps)"
[ "$SILENT" -eq 1 ] && echo "demo-katie-90: --silent (no narration)"

# ---------------------------------------------------------------------------
# Stop 1 — Katie's question, spotlit
# ---------------------------------------------------------------------------
focus_event "$SID" "$E_ASK"
pause 1
talk "Katie asks: did I research loop engineering? Anything there?"
pause 2

# ---------------------------------------------------------------------------
# Stop 2 — The recall, 84 seconds later
# ---------------------------------------------------------------------------
focus_event "$SID" "$E_RECAP"
pause 1
talk "Eighty-four seconds later: all of it. The exact June session, quoted back verbatim: Hey fam — can you do some research on loop engineering practices for agents? Let's pause and do some learning and see if we need to adjust our approach."
pause 2

# ---------------------------------------------------------------------------
# Stop 3 — The pull request
# ---------------------------------------------------------------------------
focus_event "$SID" "$E_PRSTATUS"
pause 1
talk "Thirty nine minutes later, loop engineering delivered a pull request."
pause 2

# ---------------------------------------------------------------------------
# Stop 4 — Merged, and the closer (spotlight stays up; no story zoom-out —
# Max cut it: the demo ends on the record itself)
# ---------------------------------------------------------------------------
focus_event "$SID" "$E_MERGED"
pause 1
talk "Merged."
pause 2
talk "Have your agent read your agent history to you."
pause 1

echo "demo-katie-90: done."
