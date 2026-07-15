#!/usr/bin/env bash
# demo-katie-loop.sh — narrated, agent-driven USER-STORY demo of the OpenStory
# dashboard: the true story of Katie coming back to a research thread on
# "loop engineering" she had commissioned weeks earlier, and driving it to a
# real merge. Modeled directly on ui-tour.sh's structure and control seam.
#
# Provenance (read before editing): the brief for this demo claimed Katie
# heard about "loop engineering" from a teammate, Max, and went to explore
# his workflow. The OpenStory store does not contain that. What it DOES
# contain, verbatim, in session ac9cf839-7b94-48e2-87e3-279fa8634f76 (host
# Katies-Mac-mini, user katie): Katie asking her own assistant to recall a
# deep-research pass on "loop engineering" SHE commissioned back in June for
# her own agent-orchestrator project, Raptor — and then driving the findings
# to a real, merged pull request. That is the story this script tells. It is
# a better story anyway: it is OpenStory recalling Katie's OWN memory back to
# her, mid-session, which is exactly the point of the product. See stop 8 for
# the honest note about that pivot.
#
# Drives the UI entirely through the control seam (POST JSON to
# /api/control) — the same write path any MCP/operator agent uses — and
# narrates over `say`. Nothing here touches the DOM directly; the dashboard
# stays a pure sink reacting to `control` messages broadcast over its
# WebSocket. See ui/src/lib/ui-control.ts for the control vocabulary this
# drives.
#
# Usage:
#   ./scripts/demo-katie-loop.sh              # full narrated demo
#   ./scripts/demo-katie-loop.sh --fast       # skip pacing sleeps (for testing)
#   ./scripts/demo-katie-loop.sh --silent     # skip narration (banners/toggles only)
#   OS_TOUR_VOICE=Ava ./scripts/demo-katie-loop.sh
#
# Env overrides:
#   OS_TOUR_VOICE        — `say` voice name                (default: Serena (Premium))
#   OS_TOUR_KATIE_SID     — Katie's real session id          (default: the session described above)
#   OS_TOUR_API          — control endpoint                 (default: local :3002)
#   OS_TOUR_EVT_ASK       — event id: Katie's question
#   OS_TOUR_EVT_RECAP     — event id: the research recap + tension
#   OS_TOUR_EVT_PRSTATUS  — event id: PR #22 status check
#   OS_TOUR_EVT_MERGED    — event id: merge confirmation

set -uo pipefail

VOICE="${OS_TOUR_VOICE:-Serena (Premium)}"
SID="${OS_TOUR_KATIE_SID:-ac9cf839-7b94-48e2-87e3-279fa8634f76}"
B="${OS_TOUR_API:-http://127.0.0.1:3002/api/control}"
ISSUER="katie-loop-story"

# Real event ids inside $SID, found by paging its conversation and grepping
# for "loop" — see the provenance note above for how these were verified.
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
      sed -n '2,34p' "$0"
      exit 0
      ;;
    *)
      echo "demo-katie-loop.sh: unknown flag '$arg' (use --fast / --silent)" >&2
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
    echo "demo-katie-loop: post failed (is the dashboard/server up at $B ?)" >&2
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

toggle() {
  # toggle <target> <value>
  post "{\"action\":\"toggle\",\"params\":{\"target\":\"${1}\",\"value\":\"${2}\"},\"issuer\":\"${ISSUER}\"}"
}

banner() {
  # banner <message> — the human-facing "present" call. Kept short (<=160 chars).
  post "{\"action\":\"present\",\"params\":{\"message\":\"${1}\"},\"issuer\":\"${ISSUER}\"}"
}

# stop <banner-text> <narration-text> — call AFTER issuing this stop's
# navigate/toggle action(s). Banner lands first (visible immediately),
# then a beat to let the UI settle, then the spoken narration.
stop() {
  banner "$1"
  pause 1
  talk "$2"
  pause 1.5
}

echo "demo-katie-loop: driving ${B} as issuer '${ISSUER}' (session ${SID})"
[ "$FAST" -eq 1 ] && echo "demo-katie-loop: --fast (no pacing sleeps)"
[ "$SILENT" -eq 1 ] && echo "demo-katie-loop: --silent (no narration)"

# ---------------------------------------------------------------------------
# Stop 1 — Cold open: introduce Katie and the premise
# ---------------------------------------------------------------------------
open_view explore
stop \
  "This is a true story, reconstructed entirely from one real recorded session in OpenStory." \
  "This next one is not a feature tour, it is a real story. Katie runs her own agent orchestrator project called Raptor, and back in June she asked her assistant to go do some deep research on something called loop engineering. Then life happened, and it sat untouched. Let's watch her come back to it, using nothing but her own recorded history."

# ---------------------------------------------------------------------------
# Stop 2 — Her real question, deep-linked to the exact event
# ---------------------------------------------------------------------------
open_view explore "$SID" "$E_ASK"
stop \
  "Weeks later, mid-session, Katie asks her assistant to remember what that research found." \
  "Here is the real session, and here is the exact moment. Katie typed this, word for word: I would like to know, I did a deep research on loop engineering for raptor and an assessment on what we needed to do to make it work better, anything there. She was not asking her assistant to think something up. She was asking it to remember."

# ---------------------------------------------------------------------------
# Stop 3 — Trace lens: watch the recall happen live
# ---------------------------------------------------------------------------
toggle session.lens trace
stop \
  "Flip to the trace lens: the assistant is searching OpenStory itself for that old thread." \
  "Switch the lens to trace and you can watch the recall actually happen. The assistant fires an OpenStory search for loop engineering, then loop engineering raptor, reaching back through weeks of sessions the way you or I would scroll back through old messages, except it actually finds the thing it's looking for."

# ---------------------------------------------------------------------------
# Stop 4 — Back to conversation: the origin surfaces, and the tension lands
# ---------------------------------------------------------------------------
toggle session.lens conversation
pause 1
open_view explore "$SID" "$E_RECAP"
stop \
  "Found it: a June research session, kicked off by one open-ended question, now hitting home." \
  "It finds a research session from June and surfaces the line that started it. Katie had written, hey fam, can you do some research on loop engineering practices for agents, let's pause and do some learning and see if we need to adjust our approach. Her assistant then points out the part that should really get her attention: that research predicted, almost exactly, the failure mode that is jamming her current sprint right now."

# ---------------------------------------------------------------------------
# Stop 5 — The concrete evidence: a real, clean pull request
# ---------------------------------------------------------------------------
open_view explore "$SID" "$E_PRSTATUS"
stop \
  "The research was not left abstract. It became three named backlog items, sitting in a real pull request." \
  "This is not a summary of a summary. The assessment turned into three specific, named backlog items, filed into Raptor's own backlog, sitting in an open pull request, PR twenty two, one file changed, zero code, blocked only by a single review gate. Real commits, waiting on one click."

# ---------------------------------------------------------------------------
# Stop 6 — The merge: the items stop languishing
# ---------------------------------------------------------------------------
open_view explore "$SID" "$E_MERGED"
stop \
  "Katie clears the gate. The three loop-engineering items land on main and stop languishing." \
  "Katie merges it. Her assistant confirms it plainly: the three research recommendations now live in the backlog on main, they've stopped languishing. And it names the real tension out loud, that her current sprint is jammed by the exact mechanism one of those three items was meant to fix. The research she almost forgot about turned out to be the fix she needed."

# ---------------------------------------------------------------------------
# Stop 7 — Zoom out: this session in the shape of her real work
# ---------------------------------------------------------------------------
open_view story "$SID"
pause 1
toggle story.sort tokens
stop \
  "Zooming out: Story turns this same session into plain sentences, sorted by what it actually cost." \
  "Here's that same session in Story view, read back as plain sentences instead of raw events, sorted by token cost. It's the same conversation we just walked through, just from a different angle, and it's a fast way to see that a single afternoon of catching up on old research was not a cheap detour, it was real, substantial work."

# ---------------------------------------------------------------------------
# Stop 8 — The honest note, and the takeaway
# ---------------------------------------------------------------------------
banner "One honest note: the tip was that this was about a teammate's workflow. The record said otherwise."
pause 1
talk "One honest note before we close. The tip that started this demo was that Katie had heard about loop engineering from a teammate, Max, and went to go explore his workflow. That is not what the record shows. What OpenStory actually had, verbatim, was something better, Katie recalling her own commissioned research, in her own words, months later, and riding it all the way to a merged pull request. Every quote you just heard came straight out of one real session, nothing here was invented."
pause 1

banner "This story was reconstructed entirely from recorded session data. OpenStory is the memory that made it tellable."
talk "That's the whole point of this demo. Nobody wrote this story down anywhere. It was sitting inside one session, a question, a recall, a recap, a merge, and OpenStory is what made it possible to pull all of that back out and tell it straight. Thanks for watching."
pause 1

banner "Demo complete. Take the wheel back any time, just click or navigate."
echo "demo-katie-loop: done."
