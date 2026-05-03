#!/usr/bin/env python3
"""Step 1 — gather every session active in a local-day window.

Methodology:
  1. Resolve the local calendar date in the user's timezone to a UTC window.
  2. List all sessions from /api/sessions.
  3. Filter to sessions whose start_time falls in the window. (We use start_time,
     not last_event, to define "today's sessions" — a session that started
     yesterday and ran past midnight is yesterday's, not today's.)
  4. For each survivor, fetch the canonical synopsis. The synopsis is the
     single source of truth for event_count, tool_count, error_count, label,
     first_event, last_event, duration_secs, project_id. The list endpoint
     is a snapshot and lies as it ages.
  5. Emit a flat array of canonical session records, sorted by first_event.

Output schema (per record):
  {
    "session_id", "project_id", "label", "event_count", "tool_count",
    "error_count", "first_event", "last_event", "duration_secs",
    "top_tools": [{"tool", "count"}, ...]
  }

Plus a top-level envelope with the resolved window and roster:
  {
    "window": {"date", "tz", "utc_start", "utc_end"},
    "sessions": [...]
  }

Usage:
  python3 scripts/team_day/gather.py --date 2026-05-02
  python3 scripts/team_day/gather.py --date 2026-05-02 --tz America/New_York
"""

from __future__ import annotations

import argparse
import sys
from datetime import datetime
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import (  # noqa: E402
    DEFAULT_URL,
    add_io_args,
    fetch,
    in_window,
    load_roster,
    resolve_window,
    write_json,
)


def gather(
    date_str: str,
    tz_name: str,
    base_url: str,
    limit: int = 5000,
    mode: str = "started",
) -> dict:
    """Modes:
      started  — sessions whose start_time falls in the window (default).
      active   — sessions whose [start, last_event] intersects the window.
                 Catches resumed-old sessions; noisier (re-ingestion can bump
                 last_event without real new work).
    """
    utc_start, utc_end, tz = resolve_window(date_str, tz_name)
    envelope = fetch(f"/api/sessions?limit={limit}", base_url)
    raw = envelope.get("sessions", []) if isinstance(envelope, dict) else envelope
    in_today = []
    for s in raw:
        start = s.get("start_time")
        last = s.get("last_event") or start
        if not start:
            continue
        if mode == "started":
            if utc_start <= start < utc_end:
                in_today.append(s)
        elif mode == "active":
            if start < utc_end and (last or start) >= utc_start:
                in_today.append(s)
        else:
            raise ValueError(f"unknown mode: {mode}")
    canonical = []
    for s in in_today:
        sid = s.get("session_id")
        if not sid:
            continue
        syn = fetch(f"/api/sessions/{sid}/synopsis", base_url)
        canonical.append(
            {
                "session_id": syn.get("session_id"),
                "project_id": syn.get("project_id") or s.get("project_id"),
                "project_name": syn.get("project_name") or s.get("project_name"),
                "label": syn.get("label") or s.get("label"),
                "branch": s.get("branch"),
                "host": s.get("host"),
                "user": s.get("user"),
                "event_count": syn.get("event_count", s.get("event_count", 0)),
                "tool_count": syn.get("tool_count", 0),
                "error_count": syn.get("error_count", 0),
                "first_event": syn.get("first_event") or s.get("start_time"),
                "last_event": syn.get("last_event") or s.get("last_event"),
                "duration_secs": syn.get("duration_secs"),
                "top_tools": syn.get("top_tools", []),
            }
        )
    canonical.sort(key=lambda r: r.get("first_event") or "")
    return {
        "window": {"date": date_str, "tz": tz, "utc_start": utc_start, "utc_end": utc_end},
        "sessions": canonical,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description="Gather sessions active in a local day window.")
    parser.add_argument(
        "--date",
        default=datetime.now().strftime("%Y-%m-%d"),
        help="Local date YYYY-MM-DD (default: today)",
    )
    parser.add_argument(
        "--tz",
        default=load_roster()["default_tz"],
        help="IANA tz name (default: from roster.json)",
    )
    parser.add_argument("--out", dest="output", default="-", help="output file (default stdout)")
    parser.add_argument(
        "--url", default=DEFAULT_URL, help=f"OpenStory base URL (default {DEFAULT_URL})"
    )
    parser.add_argument(
        "--mode",
        default="started",
        choices=["started", "active"],
        help="started: start_time in window (default). active: any overlap.",
    )
    args = parser.parse_args()
    result = gather(args.date, args.tz, args.url, mode=args.mode)
    write_json(result, args.output)
    sys.stderr.write(
        f"gathered {len(result['sessions'])} sessions for {args.date} ({args.tz})\n"
    )


if __name__ == "__main__":
    main()
