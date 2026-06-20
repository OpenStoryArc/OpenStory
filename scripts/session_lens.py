#!/usr/bin/env python3
"""session_lens.py — slice the OpenStory session store by device, user, or topic.

Dogfoods the OpenStory REST API (never greps transcript files). Answers the
recurring reflection questions:

  - "Is there data from device X?"        -> --by host        / --host a1
  - "What work touched topic Y?"          -> --match codex
  - "Who/what machines are streaming?"    -> --by host (default)

Reads /api/sessions once, then groups/filters in memory. Pure functions for the
aggregation logic (importable + tested via --test); side effects (HTTP, print)
live at the edges.

Usage:
  python3 scripts/session_lens.py                      # host x user inventory
  python3 scripts/session_lens.py --host a1            # detail one device
  python3 scripts/session_lens.py --match codex        # sessions mentioning a topic
  python3 scripts/session_lens.py --match codex --host a1
  python3 scripts/session_lens.py --test               # run self-tests

Env:
  OPENSTORY_API_URL   base URL (default http://localhost:3002)
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.request
from collections import defaultdict
from typing import Iterable


DEFAULT_API = os.environ.get("OPENSTORY_API_URL", "http://localhost:3002")


# ---------------------------------------------------------------------------
# Pure functions — data in, data out. No I/O.
# ---------------------------------------------------------------------------

def host_key(session: dict) -> tuple[str, str]:
    """(host, user) identity for a session, with stable placeholders."""
    return (session.get("host") or "(none)", session.get("user") or "?")


def group_by_host(sessions: Iterable[dict]) -> list[dict]:
    """Aggregate sessions into per-(host,user) rows, sorted by event volume desc."""
    agg: dict[tuple[str, str], dict] = defaultdict(
        lambda: {"sessions": 0, "events": 0, "first": "", "last": ""}
    )
    for s in sessions:
        row = agg[host_key(s)]
        row["sessions"] += 1
        row["events"] += s.get("event_count", 0) or 0
        st = s.get("start_time") or ""
        le = s.get("last_event") or ""
        if st and (not row["first"] or st < row["first"]):
            row["first"] = st
        if le > row["last"]:
            row["last"] = le
    out = [{"host": h, "user": u, **v} for (h, u), v in agg.items()]
    out.sort(key=lambda r: r["events"], reverse=True)
    return out


def matches(session: dict, needle: str) -> bool:
    """True if needle (case-insensitive) appears in label, branch, or project."""
    n = needle.lower()
    hay = " ".join(
        str(session.get(k) or "")
        for k in ("label", "branch", "project_id", "project_name")
    ).lower()
    return n in hay


def filter_sessions(
    sessions: Iterable[dict], *, host: str | None = None, match: str | None = None
) -> list[dict]:
    """Filter by host (exact) and/or topic substring, newest activity first."""
    out = []
    for s in sessions:
        if host is not None and (s.get("host") or "(none)") != host:
            continue
        if match is not None and not matches(s, match):
            continue
        out.append(s)
    out.sort(key=lambda s: s.get("last_event") or "", reverse=True)
    return out


# ---------------------------------------------------------------------------
# Edges — I/O.
# ---------------------------------------------------------------------------

def fetch_sessions(api_url: str = DEFAULT_API) -> list[dict]:
    with urllib.request.urlopen(f"{api_url}/api/sessions", timeout=15) as resp:
        return json.load(resp)["sessions"]


def print_host_table(rows: list[dict], total: int) -> None:
    print(f"{total} sessions total\n")
    print(f'{"host":22} {"user":13} {"sess":>5} {"events":>7}  {"first":10}  last')
    for r in rows:
        last = r["last"][:16].replace("T", " ")
        print(
            f'{r["host"]:22} {r["user"]:13} {r["sessions"]:5} '
            f'{r["events"]:7}  {r["first"][:10]:10}  {last}'
        )


def print_session_list(sessions: list[dict], title: str) -> None:
    print(f"=== {title} — {len(sessions)} session(s) ===\n")
    for s in sessions:
        le = (s.get("last_event") or "")[:16].replace("T", " ")
        host = s.get("host") or "(none)"
        br = (s.get("branch") or "?")[:32]
        ev = s.get("event_count", 0)
        lbl = (s.get("label") or "")[:54]
        print(f"  {le}  {host:16} ev={ev:<4} {br:34} {lbl}")


# ---------------------------------------------------------------------------
# Self-tests (no network).
# ---------------------------------------------------------------------------

def _run_tests() -> int:
    fixtures = [
        {"host": "a1", "user": "max", "event_count": 100, "label": "codex host stamping",
         "branch": "feat/codex-host-stamping", "start_time": "2026-05-09", "last_event": "2026-05-31"},
        {"host": "a1", "user": "max", "event_count": 50, "label": "unrelated",
         "branch": "master", "start_time": "2026-05-10", "last_event": "2026-05-12"},
        {"host": "Maxs-Air", "user": "maxglassie", "event_count": 9, "label": "ask codex review",
         "branch": "main", "start_time": "2026-06-01", "last_event": "2026-06-01"},
        {"host": None, "user": None, "event_count": 1, "label": None,
         "branch": None, "start_time": "", "last_event": ""},
    ]
    failures = 0

    def check(name, cond):
        nonlocal failures
        if cond:
            print(f"  ok   {name}")
        else:
            print(f"  FAIL {name}")
            failures += 1

    rows = group_by_host(fixtures)
    a1 = next(r for r in rows if r["host"] == "a1")
    check("a1 aggregates two sessions", a1["sessions"] == 2)
    check("a1 sums events", a1["events"] == 150)
    check("a1 first is earliest", a1["first"] == "2026-05-09")
    check("a1 last is latest", a1["last"] == "2026-05-31")
    check("rows sorted by events desc", rows[0]["events"] >= rows[-1]["events"])
    check("None host -> (none)", any(r["host"] == "(none)" for r in rows))

    codex = filter_sessions(fixtures, match="codex")
    check("match codex finds 2 (label + branch)", len(codex) == 2)
    check("match is case-insensitive", len(filter_sessions(fixtures, match="CODEX")) == 2)
    check("match newest first", codex[0]["last_event"] >= codex[-1]["last_event"])

    a1_only = filter_sessions(fixtures, host="a1")
    check("host filter exact", len(a1_only) == 2)
    check("host+match combine", len(filter_sessions(fixtures, host="a1", match="codex")) == 1)
    check("None-safe matches()", matches(fixtures[3], "codex") is False)

    print(f"\n{'PASS' if failures == 0 else 'FAIL'}: {len(fixtures)} fixtures, {failures} failures")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--host", help="filter to one device/host (exact match)")
    p.add_argument("--match", help="filter to sessions whose label/branch/project contains this substring")
    p.add_argument("--by", choices=["host"], default="host", help="grouping for the inventory view")
    p.add_argument("--api-url", default=DEFAULT_API, help=f"OpenStory API base (default {DEFAULT_API})")
    p.add_argument("--test", action="store_true", help="run self-tests and exit")
    args = p.parse_args(argv)

    if args.test:
        return _run_tests()

    sessions = fetch_sessions(args.api_url)

    if args.host or args.match:
        filtered = filter_sessions(sessions, host=args.host, match=args.match)
        scope = []
        if args.match:
            scope.append(f'match="{args.match}"')
        if args.host:
            scope.append(f'host={args.host}')
        print_session_list(filtered, " ".join(scope))
    else:
        print_host_table(group_by_host(sessions), len(sessions))
    return 0


if __name__ == "__main__":
    sys.exit(main())
