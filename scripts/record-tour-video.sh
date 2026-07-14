#!/usr/bin/env bash
# record-tour-video.sh — produce a narrated video of the OpenStory UI tour.
#
# Records the MAIN DISPLAY with `screencapture -v` while ui-tour.sh drives the
# dashboard, rendering each narration line to an audio file (studio-clean, no
# microphone) with its start offset. Afterwards ffmpeg rebuilds the narration
# track at those offsets and muxes it onto the screen recording.
#
# Prereqs:
#   • Screen Recording permission for your terminal app
#     (System Settings → Privacy & Security → Screen Recording)
#   • ffmpeg (brew install ffmpeg)
#   • The dashboard visible on the main display (put the browser front &
#     center — everything on that display is recorded)
#
# Usage:
#   ./scripts/record-tour-video.sh [output.mov]
#   OS_TOUR_VOICE="Serena (Premium)" ./scripts/record-tour-video.sh

set -uo pipefail

OUT="${1:-$HOME/Desktop/openstory-tour-$(date +%Y%m%d-%H%M%S).mov}"
DIR="$(mktemp -d /tmp/os-tour-video.XXXXXX)"
HERE="$(cd "$(dirname "$0")" && pwd)"
export OS_TOUR_VOICE="${OS_TOUR_VOICE:-Serena (Premium)}"

echo "▶ recording main display → $DIR/screen.mov"
echo "  (if this is the first run, macOS may ask for Screen Recording permission)"
screencapture -v -x "$DIR/screen.mov" &
REC=$!
sleep 2 # capture startup latency

export OS_TOUR_T0
OS_TOUR_T0="$(python3 -c 'import time; print(time.time())')"
export OS_TOUR_AUDIO_DIR="$DIR"

"$HERE/ui-tour.sh"

sleep 2
kill -INT "$REC" 2>/dev/null
wait "$REC" 2>/dev/null || true

if [ ! -s "$DIR/screen.mov" ]; then
  echo "✗ screen recording is empty — grant Screen Recording permission to your terminal and retry." >&2
  exit 1
fi

echo "▶ muxing narration onto video…"
python3 - "$DIR" "$OUT" <<'PY'
import subprocess, sys, os
d, out = sys.argv[1], sys.argv[2]
offsets = [float(x) for x in open(os.path.join(d, "offsets.log"))]
files = [x.strip() for x in open(os.path.join(d, "files.log"))]
cmd = ["ffmpeg", "-y", "-i", os.path.join(d, "screen.mov")]
for f in files:
    cmd += ["-i", f]
parts, mix = [], []
for i, off in enumerate(offsets):
    ms = max(0, int(off * 1000))
    parts.append(f"[{i+1}:a]adelay={ms}:all=1[a{i}]")
    mix.append(f"[a{i}]")
graph = ";".join(parts) + f";{''.join(mix)}amix=inputs={len(mix)}:normalize=0[aout]"
cmd += ["-filter_complex", graph, "-map", "0:v", "-map", "[aout]",
        "-c:v", "copy", "-c:a", "aac", "-b:a", "160k", out]
subprocess.run(cmd, check=True, capture_output=True)
print(f"✓ wrote {out}")
PY

echo "▶ done: $OUT"
echo "  (scratch kept at $DIR — delete when happy with the video)"
