#!/usr/bin/env bash
# demo-inference-club.sh — narrated, agent-driven USER-STORY demo of the
# OpenStory dashboard: the true, cross-device story of everything Max did
# with inference.club, reconstructed entirely from real recorded sessions.
# Modeled directly on demo-katie-loop.sh's structure and control seam.
#
# Provenance (read before editing): inference.club is an open-source
# distributed inference network (Django + Nuxt central service, a Go
# "inference-club-agent" that runs inside a Kubernetes cluster and joins
# inference.club's tailnet via tsnet, plus a browser extension and two
# GPU-backed services, ltx2-server and dia), built by Brian Caffey, MIT
# licensed. It lets you expose home-lab GPU inference servers to the
# internet as an OpenAI-compatible /v1 endpoint, without port forwarding.
#
# The real arc, mined read-only from GET /api/sessions and
# /api/sessions/{id}/conversation, spans two of Max's machines on
# 2026-06-27 and 2026-06-28:
#   - Maxs-Air, session 2acb1869-a2dc-493a-b500-4d49a7488cad: first contact
#     with inference-club-agent (01:40 UTC), a from-scratch sandbox stand-up
#     (kind + Helm + a mock vLLM Service, cluster discovery proven live,
#     stopped honestly at the real-API-key wall), a live teach-back on the
#     label/annotate model (19:25), and a teardown the next day (06-28
#     19:39).
#   - a1 (Max's home box), session f1688f0d-3d88-4406-814d-eac34d99ffaf:
#     ~16 hours after the laptop exploration, Max pasted the org's GitHub
#     URL and asked for every repo runnable without sudo; hit a real sudo/
#     TTY blocker on GPU passthrough; then asked how inference.club could
#     serve models for his separate `agent-harness` coding-agent lab, and
#     the next day (06-28 11:43) proved a real request from the `pi`
#     harness routed through inference.club to LM Studio on his RTX 5090.
#   - Maxs-Air again, session f09d25fa-6c15-4df6-9523-9e3b6e78582a: later
#     on 06-27, Max asked which of inference.club's own architectural
#     patterns (visibility tiers, encrypt-at-rest, tailnet trust) OpenStory
#     itself was missing — which led, by 06-28 21:11, to a committed spec
#     for OpenStory's own encrypted-isolation design.
#
# Honesty note: today's ask (session 917baaad...) that produced this demo
# is excluded as evidence — it is the ask, not the story. Every quote
# below is verbatim from the three sessions above.
#
# Usage:
#   ./scripts/demo-inference-club.sh              # full narrated demo
#   ./scripts/demo-inference-club.sh --fast       # skip pacing sleeps (for testing)
#   ./scripts/demo-inference-club.sh --silent     # skip narration (banners/toggles only)
#   OS_TOUR_VOICE=Ava ./scripts/demo-inference-club.sh
#
# Env overrides:
#   OS_TOUR_VOICE          — `say` voice name                (default: Serena (Premium))
#   OS_TOUR_API            — control endpoint                (default: local :3002)
#   OS_TOUR_SID_LAPTOP     — Maxs-Air / inference-club-agent session
#   OS_TOUR_SID_A1         — a1 / get-repos-running + lab session
#   OS_TOUR_SID_ARCH       — Maxs-Air / OpenStory-architecture session
#   OS_TOUR_EVT_ASK_LAPTOP — event id: "hey tell me about this repo :)"
#   OS_TOUR_EVT_STANDUP    — event id: sandbox stood up, hit the API-key wall
#   OS_TOUR_EVT_ASK_A1     — event id: "Let's get these repos and get them running"
#   OS_TOUR_EVT_SCORECARD  — event id: 5-repo setup scorecard on a1
#   OS_TOUR_EVT_LAB        — event id: what the agent-harness lab actually is
#   OS_TOUR_EVT_PROOF      — event id: pi harness request proven through the 5090
#   OS_TOUR_EVT_TEACH      — event id: "labels select, annotations describe"
#   OS_TOUR_EVT_ARCH       — event id: which patterns OpenStory itself is missing
#   OS_TOUR_EVT_TEARDOWN   — event id: sandbox torn down, repo untouched

set -uo pipefail

VOICE="${OS_TOUR_VOICE:-Serena (Premium)}"
B="${OS_TOUR_API:-http://127.0.0.1:3002/api/control}"
ISSUER="inference-club-story"

# Real session ids, found via GET /api/sessions filtered on "inference" /
# "club" — see the provenance note above for how each was verified.
SID_LAPTOP="${OS_TOUR_SID_LAPTOP:-2acb1869-a2dc-493a-b500-4d49a7488cad}"
SID_A1="${OS_TOUR_SID_A1:-f1688f0d-3d88-4406-814d-eac34d99ffaf}"
SID_ARCH="${OS_TOUR_SID_ARCH:-f09d25fa-6c15-4df6-9523-9e3b6e78582a}"

# Real event ids inside those sessions, found by paging each conversation
# and grepping for the quoted text — see the provenance note above.
E_ASK_LAPTOP="${OS_TOUR_EVT_ASK_LAPTOP:-178229f0-2ad7-4773-97fb-a1136aabcd99}"
E_STANDUP="${OS_TOUR_EVT_STANDUP:-1b52fb58-845e-4f24-a228-284a4e3d3ca6}"
E_ASK_A1="${OS_TOUR_EVT_ASK_A1:-6853aba5-4598-4aec-8a6e-48c9d7f593ff}"
E_SCORECARD="${OS_TOUR_EVT_SCORECARD:-b815b057-1e9f-4a64-bb47-bbdb4d5222b9}"
E_LAB="${OS_TOUR_EVT_LAB:-22c244d1-a4c6-4ddb-a042-2fa33a7d373e}"
E_PROOF="${OS_TOUR_EVT_PROOF:-c7d451e4-4496-445f-8c53-01b12e795365}"
E_TEACH="${OS_TOUR_EVT_TEACH:-39eddfed-569a-4f92-a63e-1287fe4088d5}"
E_ARCH="${OS_TOUR_EVT_ARCH:-eb77e258-87d0-457a-a455-cf788c6eb4b8}"
E_TEARDOWN="${OS_TOUR_EVT_TEARDOWN:-8f75ec6a-3711-4db3-9627-9e68b829333e}"

FAST=0
SILENT=0
for arg in "$@"; do
  case "$arg" in
    --fast) FAST=1 ;;
    --silent) SILENT=1 ;;
    -h|--help)
      sed -n '2,55p' "$0"
      exit 0
      ;;
    *)
      echo "demo-inference-club.sh: unknown flag '$arg' (use --fast / --silent)" >&2
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
    echo "demo-inference-club: post failed (is the dashboard/server up at $B ?)" >&2
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

echo "demo-inference-club: driving ${B} as issuer '${ISSUER}'"
echo "demo-inference-club: sessions laptop=${SID_LAPTOP} a1=${SID_A1} arch=${SID_ARCH}"
[ "$FAST" -eq 1 ] && echo "demo-inference-club: --fast (no pacing sleeps)"
[ "$SILENT" -eq 1 ] && echo "demo-inference-club: --silent (no narration)"

# ---------------------------------------------------------------------------
# Stop 1 — Cold open: what inference.club actually is, per the data
# ---------------------------------------------------------------------------
open_view explore
stop \
  "This is a true story about inference.club, told from real recorded sessions across two of Max's machines." \
  "This next one is a real story, not a feature tour. Inference dot club is an open source network that lets you expose the inference servers running on your own GPUs to the internet, without port forwarding or a public IP, using a small agent and a private tailnet. Let's watch Max meet it, stand it up, connect it to his own coding agent lab, and feed what he learned straight back into OpenStory."

# ---------------------------------------------------------------------------
# Stop 2 — First contact, on the laptop
# ---------------------------------------------------------------------------
open_view explore "$SID_LAPTOP" "$E_ASK_LAPTOP"
stop \
  "First contact. On his laptop, Max asks his assistant to explain a repo he just cloned." \
  "Here is the real first moment, on Max's Air. He typed, word for word, hey tell me about this repo, with a smiley. His assistant read the code and explained that this Go agent runs inside a Kubernetes cluster, registers with inference dot club, and joins a private tailnet so the outside world can reach GPUs that were never port forwarded at all."

# ---------------------------------------------------------------------------
# Stop 3 — Trace lens: standing up a real sandbox, honestly, to a real wall
# ---------------------------------------------------------------------------
toggle session.lens trace
stop \
  "Flip to the trace lens: Max says stand it up as much as possible, and the agent actually does." \
  "Switch to the trace lens and watch the real work. It built the Docker image from source, installed Helm and kind with Homebrew, stood up a local cluster, deployed a mock inference service, and installed the real Helm chart. Cluster discovery worked completely end to end, and it stopped honestly at the one wall it could not cross, a genuine inference dot club API key, refusing to fake one just to look finished."

# ---------------------------------------------------------------------------
# Stop 4 — Device switch: the story moves to a1
# ---------------------------------------------------------------------------
toggle session.lens conversation
pause 1
open_view explore "$SID_A1" "$E_ASK_A1"
stop \
  "This next part happened on a different machine entirely, Max's home box a-one, about sixteen hours later." \
  "Same day, a different computer. On a-one, Max pasted the org's GitHub URL and said, let's get these repos and get them running, get as far as you can without sudo, and gather any sudo commands into a script so I can remove blockers myself. Five separate inference dot club repositories, on a machine that had never seen any of this before."

# ---------------------------------------------------------------------------
# Stop 5 — Subagents lens: five repos surveyed in parallel, then a real wall
# ---------------------------------------------------------------------------
toggle session.lens subagents
stop \
  "The subagents lens shows five repos explored in parallel, then a real sudo wall, exactly as promised." \
  "You can see it fan out into five parallel explorations, one per repo, then get four of them fully running without touching sudo at all, node, python, go, and poetry installed straight into Max's own account. The one real blocker was GPU passthrough into Docker, and true to the ask, it got bundled into a single setup sudo script instead of being papered over."

# ---------------------------------------------------------------------------
# Stop 6 — Purpose surfaces: why Max wanted this running at all
# ---------------------------------------------------------------------------
toggle session.lens conversation
pause 1
open_view explore "$SID_A1" "$E_LAB"
stop \
  "Then the real reason surfaces. Max has his own coding agent lab, and he wants inference.club to feed it." \
  "Max asked his assistant to look at a separate project of his, called agent harness, and figure out how inference dot club could run models for it. The answer was clean, his lab already speaks the OpenAI v1 protocol, and inference dot club is exactly that protocol on the other end, backed by his own graphics card instead of somebody else's cloud."

# ---------------------------------------------------------------------------
# Stop 7 — The proof: a real request, end to end, onto Max's own GPU
# ---------------------------------------------------------------------------
toggle session.lens trace
open_view explore "$SID_A1" "$E_PROOF"
stop \
  "The next day, the proof. A real coding agent request travels through inference.club onto Max's own graphics card." \
  "This is the payoff. A baseline run from the pi coding harness went through the lab's logging proxy, into inference dot club's v1 endpoint, through the Go agent in direct mode, into LM Studio, and onto Max's own RTX 50 90, and back. The model itself replied, quote, I'm an expert coding assistant running inside the pi coding agent harness. Two side projects, wired together, proven with one real token."

# ---------------------------------------------------------------------------
# Stop 8 — Back to the laptop: teaching the mental model live
# ---------------------------------------------------------------------------
toggle session.lens trace
open_view explore "$SID_LAPTOP" "$E_TEACH"
stop \
  "Back on the laptop, hours later, Max asks to actually be taught how the thing works." \
  "Max wrote, teach me how to use this please, with a smiley, and his assistant taught it live against the still running sandbox cluster. The whole mental model came down to one sentence: using this tool means labelling Kubernetes services, because labels select and annotations describe, and that is genuinely the entire interface."

# ---------------------------------------------------------------------------
# Stop 9 — The feedback loop: inference.club reshapes OpenStory itself
# ---------------------------------------------------------------------------
toggle session.lens conversation
open_view explore "$SID_ARCH" "$E_ARCH"
stop \
  "One more turn. Studying inference.club's own architecture fed straight back into OpenStory's roadmap." \
  "Later that same day, Max asked, curious how OpenStory might benefit from the architectural patterns here. His assistant found that inference dot club had already solved a gap OpenStory had, per user visibility tiers and encryption at rest derived from an app secret. By the next evening that turned into a real committed design spec for OpenStory's own encrypted isolation, sitting in a feature branch right now."

# ---------------------------------------------------------------------------
# Stop 10 — Teardown, and the cross-device memory takeaway
# ---------------------------------------------------------------------------
open_view explore "$SID_LAPTOP" "$E_TEARDOWN"
stop \
  "The next day, Max says tear down the sandbox now, and the record shows it actually happening, cleanly." \
  "Tear down the sandbox now, with a smiley, and the kind cluster, the local image, and the docker context all came down, with the actual repo left untouched. That is the whole physical story, start to finish, nothing left running that Max did not choose to leave running."

banner "Three sessions, two machines, three days: a laptop sandbox, a home GPU box, and a fed-back design spec."
pause 1
talk "Here is the honest shape of it. It started on Max's laptop meeting a new repo, jumped to his home box a-one sixteen hours later to actually run five repos and wire a real GPU into his own coding agent lab, and came back around to reshape OpenStory's own design. No one wrote any of that down as one story anywhere. It was scattered across two machines and three days, and OpenStory is what made it possible to pull it back out and tell it straight."
pause 1

banner "Demo complete. Take the wheel back any time, just click or navigate."
echo "demo-inference-club: done."
