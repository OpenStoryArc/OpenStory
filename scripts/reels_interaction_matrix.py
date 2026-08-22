#!/usr/bin/env python3
"""
reels_interaction_matrix.py — live exploration loop for reel × ink UX.

TDD pure core lives in ui/tests/lib/reel-annotate.test.ts (permutations).
This script exercises the *running* stack:

  1. Create a multi-slide reel (title / diagram / title)
  2. Open it in the UI (control)
  3. Agent pen → each slide (target:slide parity with human Annotate)
  4. Review via GET /api/ui-state/journey
  5. Print a usability matrix report

Usage:
  python3 scripts/reels_interaction_matrix.py           # run live loop
  python3 scripts/reels_interaction_matrix.py --test    # offline self-check
  python3 scripts/reels_interaction_matrix.py --dry-run # no control posts
"""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request
from typing import Any

API = "http://127.0.0.1:3002"


def http_json(method: str, url: str, body: dict | None = None) -> Any:
    data = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        url,
        data=data,
        method=method,
        headers={"Content-Type": "application/json"} if body is not None else {},
    )
    with urllib.request.urlopen(req, timeout=20) as r:
        return json.loads(r.read().decode())


def control(action: str, params: dict, issuer: str = "matrix") -> dict:
    return http_json(
        "POST",
        f"{API}/api/control",
        {"action": action, "params": params, "issuer": issuer},
    )


def mark_stroke(cx: float, cy: float, r: float = 0.06) -> list[dict]:
    """Simple circle — same primitives as human pen."""
    return [
        {
            "type": "circle",
            "cx": cx,
            "cy": cy,
            "r": r,
            "stroke": "#0f172a",
            "strokeWidth": 8,
            "fill": "none",
        },
        {
            "type": "circle",
            "cx": cx,
            "cy": cy,
            "r": r,
            "stroke": "#facc15",
            "strokeWidth": 4,
            "fill": "none",
        },
    ]


def create_matrix_reel() -> str:
    body = {
        "title": "Interaction matrix — explore slides",
        "author": "matrix-bot",
        "opener": "BLUF: One format — slides, tools, ink. Explore every combo.",
        "closer": "Matrix complete. Review beatInk on the journey.",
        "stops": [
            {
                "kind": "title",
                "line": "Slide A — title. Agent marks top-left.",
            },
            {
                "kind": "diagram",
                "line": "Slide B — diagram. Agent marks center.",
                "visual": {
                    "kind": "labels",
                    "title": "Journey",
                    "labels": ["Read", "Edit ×2", "Bash", "Commit"],
                },
            },
            {
                "kind": "title",
                "line": "Slide C — title. Agent marks bottom-right.",
            },
        ],
    }
    res = http_json("POST", f"{API}/api/reels", body)
    if not res.get("ok"):
        raise RuntimeError(f"save reel failed: {res}")
    return res["id"]


def agent_ink_slide(reel_id: str, beat_index: int, cx: float, cy: float) -> dict:
    """Agent pen → beat ink (parity path)."""
    return control(
        "draw",
        {
            "target": "slide",
            "reelId": reel_id,
            "beatIndex": beat_index,
            "mode": "append",
            "label": f"agent-slide-{beat_index}",
            "strokes": mark_stroke(cx, cy),
        },
    )


def open_reel(reel_id: str) -> dict:
    return control(
        "open_view",
        {"view": "reels", "reelId": reel_id, "reelAutoplay": True},
    )


def review_journey(n: int = 25) -> list[dict]:
    data = http_json("GET", f"{API}/api/ui-state/journey?n={n}")
    return data.get("journey") or []


def usability_checklist() -> list[tuple[str, str]]:
    """Design/usability tasks — human or agent should be able to answer each."""
    return [
        ("U1", "Create multi-kind reel (title + diagram)"),
        ("U2", "Open reel and see unified slide chrome (play/pause, dots)"),
        ("U3", "Pause and click-through without auto-advance racing"),
        ("U4", "Annotate slide 0 — ink only on slide 0"),
        ("U5", "Advance — slide 1 empty; back — slide 0 ink restored"),
        ("U6", "Agent draw target=slide matches human annotate store"),
        ("U7", "Clear slide ink does not clear other slides"),
        ("U8", "Journey shows reelId + beatIndex + beatInk counts"),
        ("U9", "Opener/closer are slides in progress bar (unified list)"),
        ("U10", "Done annotating restores click-to-advance"),
    ]


def run_live(*, dry_run: bool) -> int:
    print("=== reels interaction matrix (live) ===\n")
    print("Usability tasks (design space):\n")
    for code, text in usability_checklist():
        print(f"  [{code}] {text}")
    print()

    if dry_run:
        print("(dry-run — no API calls)")
        return 0

    try:
        http_json("GET", f"{API}/api/sessions")
    except urllib.error.URLError as e:
        print(f"API down at {API}: {e}", file=sys.stderr)
        return 1

    reel_id = create_matrix_reel()
    print(f"created reel {reel_id}")

    open_reel(reel_id)
    print("opened reel (autoplay)")
    time.sleep(0.6)

    # Permutations: agent ink on each body-oriented index.
    # Unified slides: 0=opener, 1=title A, 2=diagram B, 3=title C, 4=closer
    # BeatInkLayer uses slide index from playerToSlideIndex — body stop 0 → slide 1 if opener exists.
    # Agent target uses the same key the player uses when on that slide.
    # For matrix we paint indices 0,1,2 as stop indices AND also unified 1,2,3.
    combos = [
        (0, 0.25, 0.3, "title-A / stop0"),
        (1, 0.5, 0.5, "diagram-B / stop1"),
        (2, 0.75, 0.7, "title-C / stop2"),
    ]
    results = []
    for beat, cx, cy, label in combos:
        # Map stop index → unified slide index (opener shifts +1)
        slide_idx = beat + 1  # with opener present
        r = agent_ink_slide(reel_id, slide_idx, cx, cy)
        results.append((slide_idx, label, r.get("ok"), r.get("delivered")))
        print(f"  agent ink slide={slide_idx} ({label}) → ok={r.get('ok')} delivered={r.get('delivered')}")
        time.sleep(0.35)

    # Clear middle slide only
    control(
        "draw",
        {
            "target": "slide",
            "reelId": reel_id,
            "beatIndex": 2,
            "clear": True,
            "strokes": [],
        },
    )
    print("  cleared slide 2 ink only")
    time.sleep(0.3)
    # re-ink slide 2
    agent_ink_slide(reel_id, 2, 0.55, 0.55)
    print("  re-annotated slide 2")
    time.sleep(0.5)

    journey = review_journey(40)
    beat_rows = [e for e in journey if isinstance(e, dict) and e.get("beatInk")]
    print(f"\nJourney: {len(journey)} rows, {len(beat_rows)} with beatInk")
    for e in beat_rows[-8:]:
        bi = e.get("beatInk") or {}
        print(
            f"  beat={bi.get('beatIndex')} n={bi.get('stroke_count')} "
            f"annotate={e.get('annotate')} reel={str(e.get('reelId') or '')[:18]}"
        )

    print("\n=== matrix report ===")
    print(f"reel_id: {reel_id}")
    print("agent_draw_results:", results)
    print(
        "PASS heuristic: all agent draws delivered and at least one beatInk row with n>0"
    )
    ok = all(r[2] for r in results) and any(
        (e.get("beatInk") or {}).get("stroke_count", 0) > 0 for e in beat_rows
    )
    # Note: beatInk only appears when UI has BeatInkLayer mounted & reporting —
    # control commitBeatInkIntent updates localStorage but journey needs UI tab open.
    print("live_loop:", "PASS" if ok else "PARTIAL (open UI on reel for full beatInk reports)")
    print(f"\nOpen: http://127.0.0.1:5173/#/reels/{reel_id}?autoplay=1")
    return 0 if all(r[2] for r in results) else 1


def run_tests() -> int:
    # Offline: ensure mark geometry and checklist exist
    assert len(mark_stroke(0.5, 0.5)) == 2
    assert len(usability_checklist()) >= 8
    print("reels_interaction_matrix offline tests ok")
    return 0


def main() -> int:
    global API
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--test", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--api", default=API)
    args = p.parse_args()
    API = args.api.rstrip("/")
    if args.test:
        return run_tests()
    return run_live(dry_run=args.dry_run)


if __name__ == "__main__":
    raise SystemExit(main())
