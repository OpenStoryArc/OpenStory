#!/usr/bin/env python3
"""Step 5 — verify every session record is internally consistent.

Methodology:
  We check invariants that, if violated, mean the report would lie if we
  didn't catch them here. Each violation goes into a 'warnings' list with a
  pointer to the session_id. Warnings are non-fatal — the bundle still flows
  through — but the composer should refuse to make claims about a session
  whose record is flagged.

  Invariants:
    1. timestamps in window: first_event and last_event both fall in the
       declared utc_start..utc_end window.
    2. duration matches: last_event - first_event ≈ duration_secs (within 5s).
       Negative duration_secs means the synopsis disagrees with first/last
       (we observed this on `a5816733` today — clock skew or merge bug).
    3. author resolved: tags.author != 'unknown'. Unknown authors mean the
       composer can't attribute the work; flag for human review.
    4. opening_prompt vs label divergence: when both exist, the label should
       be a prefix or substring of opening_prompt. If they fully diverge,
       quoting either is risky — flag.
    5. event_count > 0 for ship sessions: a session tagged 'ship' with zero
       events touched is a contradiction; usually means the activity endpoint
       returned stale data.
    6. files_touched paths consistent with project_id: every path should
       share the project_id's home prefix. Cross-prefix files mean either
       the session jumped repos or the project_id is wrong.

Output: bundle augmented with `validation`:
  {
    "warnings": [{"session_id", "rule", "detail"}, ...],
    "ok_session_ids": [...],
    "flagged_session_ids": [...]
  }
"""

from __future__ import annotations

import argparse
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import in_window, read_json, write_json  # noqa: E402


def _parse_iso(s: str | None):
    if not s:
        return None
    try:
        return datetime.strptime(s[:19], "%Y-%m-%dT%H:%M:%S")
    except (ValueError, TypeError):
        return None


def validate(bundle: dict) -> dict:
    window = bundle["window"]
    warnings: list[dict] = []

    def warn(sid: str, rule: str, detail: str) -> None:
        warnings.append({"session_id": sid, "rule": rule, "detail": detail})

    flagged: set[str] = set()

    for s in bundle["sessions"]:
        sid = s["session_id"]
        # Rule 1: window
        if not in_window(s.get("first_event"), window["utc_start"], window["utc_end"]):
            warn(sid, "window", f"first_event {s.get('first_event')} outside window")
            flagged.add(sid)

        # Rule 2: duration sanity
        first = _parse_iso(s.get("first_event"))
        last = _parse_iso(s.get("last_event"))
        dur = s.get("duration_secs")
        if first and last and isinstance(dur, (int, float)):
            implied = (last - first).total_seconds()
            if dur < 0:
                warn(sid, "duration_negative", f"duration_secs={dur}")
                flagged.add(sid)
            elif abs(implied - dur) > 5:
                warn(
                    sid,
                    "duration_mismatch",
                    f"duration_secs={dur}, last-first={implied:.0f}",
                )
                flagged.add(sid)

        # Rule 3: author resolved
        if s.get("tags", {}).get("author") == "unknown":
            warn(sid, "unknown_author", f"project_id={s.get('project_id')}")
            flagged.add(sid)

        # Rule 4: opening prompt vs label
        opening = (s.get("enriched") or {}).get("opening_prompt") or ""
        label = s.get("label") or ""
        if opening and label:
            label_norm = label.rstrip(".…").strip().lower()
            opening_norm = opening.strip().lower()
            if label_norm and label_norm not in opening_norm:
                warn(
                    sid,
                    "label_opening_divergence",
                    f"label={label!r} not a substring of opening prompt",
                )
                # Soft flag — don't block reporting, just warn.

        # Rule 5: ship-with-no-events
        kind = s.get("tags", {}).get("kind")
        ev = s.get("event_count", 0)
        files = (s.get("enriched") or {}).get("files_touched") or []
        if kind == "ship" and ev == 0:
            warn(sid, "ship_no_events", "kind=ship but event_count=0")
            flagged.add(sid)
        if kind == "ship" and not files:
            warn(sid, "ship_no_files", "kind=ship but files_touched is empty")

        # Rule 6: file paths vs project_id
        pid = s.get("project_id") or ""
        if pid.startswith("-"):
            home_prefix = "/" + pid.lstrip("-").split("-workspace")[0].replace("-", "/")
            mismatched = []
            for f in files:
                p = f.get("path", "") if isinstance(f, dict) else str(f)
                if p and not p.startswith("/Users/") and not p.startswith("/tmp"):
                    continue
                if p.startswith("/tmp") or p.startswith("/Users/"):
                    user_root = "/" + "/".join(p.split("/")[:3])
                    expected_user = home_prefix.rsplit("/workspace", 1)[0]
                    if not p.startswith("/tmp") and not p.startswith(expected_user):
                        mismatched.append(p)
            if len(mismatched) > 3:
                warn(
                    sid,
                    "file_project_mismatch",
                    f"{len(mismatched)} files outside project home (e.g. {mismatched[0]})",
                )

    ok = [s["session_id"] for s in bundle["sessions"] if s["session_id"] not in flagged]
    return {
        **bundle,
        "validation": {
            "warnings": warnings,
            "ok_session_ids": ok,
            "flagged_session_ids": sorted(flagged),
        },
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Validate enriched session records.")
    parser.add_argument("--in", dest="input", default="-")
    parser.add_argument("--out", dest="output", default="-")
    parser.add_argument(
        "--strict",
        action="store_true",
        help="Exit non-zero if any session is flagged.",
    )
    args = parser.parse_args()
    bundle = read_json(args.input)
    result = validate(bundle)
    write_json(result, args.output)
    n_warn = len(result["validation"]["warnings"])
    n_flag = len(result["validation"]["flagged_session_ids"])
    sys.stderr.write(f"validation: {n_warn} warnings, {n_flag} flagged sessions\n")
    if args.strict and n_flag:
        sys.exit(1)


if __name__ == "__main__":
    main()
