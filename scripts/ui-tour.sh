#!/usr/bin/env bash
# ui-tour.sh — narrated, agent-driven product tour of the OpenStory dashboard.
#
# Drives the UI entirely through the control seam (POST JSON to
# /api/control) — the same write path any MCP/operator agent uses — and
# narrates over `say`. Nothing here touches the DOM directly; the dashboard
# stays a pure sink reacting to `control` messages broadcast over its
# WebSocket. The last stop names this on purpose: the tour itself is an
# agent driving the observed/observer seam it just spent ten minutes showing
# off. See ui/src/lib/ui-control.ts for the control vocabulary this drives.
#
# Usage:
#   ./scripts/ui-tour.sh              # full narrated tour
#   ./scripts/ui-tour.sh --fast       # skip pacing sleeps (for testing)
#   ./scripts/ui-tour.sh --silent     # skip narration (banners/toggles only)
#   OS_TOUR_VOICE=Ava ./scripts/ui-tour.sh
#
# Env overrides:
#   OS_TOUR_VOICE  — `say` voice name              (default: Samantha)
#   OS_TOUR_SID    — demo session id to drive       (default: rich demo session)
#   OS_TOUR_API    — control endpoint               (default: local :3002)

set -uo pipefail

VOICE="${OS_TOUR_VOICE:-Samantha}"
SID="${OS_TOUR_SID:-917baaad-5bf7-4e16-9329-e0f2331724dd}"
B="${OS_TOUR_API:-http://127.0.0.1:3002/api/control}"
ISSUER="ui-tour"

FAST=0
SILENT=0
SHORT=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --silent) SILENT=1 ;;
    --short) SHORT=1 ;;
    -h|--help)
      sed -n '2,20p' "$0"
      exit 0
      ;;
    *)
      echo "ui-tour.sh: unknown flag '$arg' (use --fast / --silent)" >&2
      exit 1
      ;;
  esac
done

# post <json-body> — fire a control action at the dashboard. Best-effort:
# a dropped stop shouldn't kill a live tour, so failures are logged, not fatal.
post() {
  if ! curl -sS -m 5 -o /dev/null -X POST "$B" \
    -H 'Content-Type: application/json' \
    -d "$1"; then
    echo "ui-tour: post failed (is the dashboard/server up at $B ?)" >&2
  fi
}

SEG=0
# talk <text> — BLOCKING narration; this is what paces the whole tour.
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
  # open_view <view> [sessionId]
  local view="$1" sid="${2:-}"
  if [ -n "$sid" ]; then
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

echo "ui-tour: driving ${B} as issuer '${ISSUER}' (session ${SID})"
[ "$FAST" -eq 1 ] && echo "ui-tour: --fast (no pacing sleeps)"
[ "$SILENT" -eq 1 ] && echo "ui-tour: --silent (no narration)"

# ---------------------------------------------------------------------------
# Stop 1 — Cold open: what OpenStory is
# ---------------------------------------------------------------------------
open_view live
stop \
  "Welcome. This is OpenStory, the flight recorder for your agent fleet." \
  "Hey there. This is OpenStory, and the easiest way to think about it is your agent fleet's flight recorder. Every session, every tool call, every token your agents spend gets captured here, live, as it happens. Let's take a quick tour of it."

# ---------------------------------------------------------------------------
# Stop 2 — Explore: the sessions browser
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
open_view explore
stop \
  "Explore is the sessions browser: every run, searchable and sortable in one place." \
  "This is Explore, the browser for everything your agents have ever done. You can search across sessions, sort by recency or by token cost, and drill into any run without losing your place. It's the front door for digging back through history."

# ---------------------------------------------------------------------------
fi
# Stop 3 — Open the rich demo session: the session pane, conversation-first
# ---------------------------------------------------------------------------
open_view explore "$SID"
stop \
  "Here's a real session. The transcript leads, tokens and tool trace are one click away." \
  "Now we're inside one real session. Notice the conversation comes first, right at the top, because the story of what the agent did matters more than a raw event feed. Above it you've got a token summary and an activity ribbon, and just below, tabs for the tool trace, any subagents, and the full detail wall."

# ---------------------------------------------------------------------------
# Stop 4 — Flip lenses: tool trace, then subagents
# ---------------------------------------------------------------------------
toggle session.lens trace
pause 1
toggle session.lens subagents
stop \
  "Flipping lenses: the tool trace, then the subagent fan-out this session spawned." \
  "Let's flip lenses. Tool trace shows every call the agent made, in order, with timing. And if this session delegated work out, subagents breaks that fan-out apart too, so you can see exactly who did what inside a single run."

# ---------------------------------------------------------------------------
# Stop 5 — Declutter: compact the ribbon, fold up the token math
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
toggle ribbon.compact on
pause 1
toggle tokens.collapsed on
stop \
  "Compacting the ribbon and folding up the token math: same data, a lot less chrome." \
  "For long sessions, all that detail can get busy, so both the activity ribbon and the token report can compact down on demand. Same underlying data, just a tighter footprint, which matters once a session runs for hours instead of minutes."

# ---------------------------------------------------------------------------
fi
# Stop 6 — Theme flip to light
# ---------------------------------------------------------------------------
toggle theme light
stop \
  "Let's see it in daylight. The whole theme flips instantly, no reload." \
  "The whole interface can flip from dark to light in an instant, no reload, no flash. It's a real preference, not an afterthought, and it's remembered per device. Some people just read logs better on a bright screen in the middle of the day."

# ---------------------------------------------------------------------------
# Short cut: restore dark without ceremony (stop 7 is skipped).
if [ "$SHORT" -eq 1 ]; then
  post '{"action":"toggle","params":{"target":"theme","value":"dark"},"issuer":"'"$ISSUER"'"}'
fi

# Stop 7 — Theme back to dark, ribbon restored to full detail
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
toggle theme dark
pause 1
toggle ribbon.compact off
stop \
  "Back to dark, and back to the roomier ribbon. Your view, your call." \
  "Back to dark, and back to the fuller ribbon layout. The point isn't which one is right, it's that the human at the keyboard always has the last word over how this looks, even when an agent is the one driving."

# ---------------------------------------------------------------------------
fi
# Stop 8 — Story: narrative sentences, sorted by token cost
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
open_view story "$SID"
pause 1
toggle story.sort tokens
stop \
  "Story turns a session into plain sentences, sorted here by token cost." \
  "This is Story. Instead of raw events, it reads back like a narrative, plain sentences describing what happened and when. Sorting by token cost is a fast way to spot the sessions quietly burning through your budget."

# ---------------------------------------------------------------------------
fi
# Stop 9 — Canvas: the whole fleet as a zoomable board
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
open_view canvas
stop \
  "Canvas: a zoomable board, drilling from group down to a single run." \
  "Canvas lays your whole fleet out as a zoomable board. It starts grouped by day or user or project so it never feels overwhelming, and you pan and click your way deeper, group to project to a single session, with the same detail pane we just saw waiting underneath."

# ---------------------------------------------------------------------------
fi
# Stop 10 — Ask: real questions, computed live, no model in the loop
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
open_view ask
stop \
  "Ask answers real questions about your history, computed live, no model in the loop." \
  "Ask lets you pick a real question about your history and get an answer computed straight from the data, no language model in the loop, nothing sent anywhere. Every answer links back into Explore, so you're never more than a click from the actual sessions behind it."

# ---------------------------------------------------------------------------
fi
# Stop 11 — Zoom all the way out: Users, then the federation in Admin
if [ "$SHORT" -eq 0 ]; then
# ---------------------------------------------------------------------------
open_view users
pause 1.5
open_view admin
stop \
  "Zooming all the way out: every person in the fleet, then the federation tying it together." \
  "Users rolls everything up by person, who's working, from where, and how much they're spending. Admin goes one level further and shows the federation topology itself, how every device and account link up into a single shared store."

# ---------------------------------------------------------------------------
fi
# Stop 12 — The meta moment: this tour is an agent driving the seam
# ---------------------------------------------------------------------------
open_view live
banner "One last thing: this whole tour was an agent, posting to the same control API you just watched."
pause 1
talk "Here's the last thing worth saying out loud. Nothing about this tour used a mouse. Every view change, every toggle, every one of these banners came from a script posting plain JSON to the same control endpoint you've been watching this whole time. The dashboard was the observed, and for the last few minutes, it was also the observer. That's the whole idea behind OpenStory, the tool that watches your agents work can be driven by an agent too. Thanks for watching."
pause 1

banner "Tour complete. Take the wheel back any time, just click or navigate."
echo "ui-tour: done."
