"""Shared helpers for the team_day pipeline.

Every script in this directory is a pure function from JSON in to JSON out.
This module provides:
  - HTTP fetch against the OpenStory REST API
  - Timezone / window resolution (local date -> UTC bounds)
  - Stdin/stdout JSON I/O wrappers
  - Roster loader

Methodology: deterministic everywhere. No retries with sleeps, no defaults
that hide failure. If the API is down, we fail loudly so the report never
narrates over missing data.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import urllib.error
import urllib.request
from datetime import datetime, time, timedelta, timezone
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo


DEFAULT_URL = os.environ.get("OPEN_STORY_URL", "http://localhost:3002")
ROSTER_PATH = Path(__file__).parent / "roster.json"


# -- HTTP -------------------------------------------------------------

def fetch(path: str, base_url: str = DEFAULT_URL) -> Any:
    """GET {base_url}{path} and return parsed JSON. Exit 2 on any failure."""
    url = f"{base_url}{path}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {url}: {e}\n")
        sys.exit(2)
    except json.JSONDecodeError as e:
        sys.stderr.write(f"error: invalid JSON from {url}: {e}\n")
        sys.exit(2)


# -- Time windows -----------------------------------------------------

def resolve_window(date_str: str, tz_name: str) -> tuple[str, str, str]:
    """A local calendar date in tz -> (utc_start, utc_end, tz_name).

    Returns ISO-8601 strings with trailing 'Z'.
    The window is [00:00 local, 24:00 local), converted to UTC.
    """
    tz = ZoneInfo(tz_name)
    d = datetime.strptime(date_str, "%Y-%m-%d").date()
    local_start = datetime.combine(d, time.min, tzinfo=tz)
    local_end = local_start + timedelta(days=1)
    utc_start = local_start.astimezone(timezone.utc)
    utc_end = local_end.astimezone(timezone.utc)
    return (
        utc_start.strftime("%Y-%m-%dT%H:%M:%SZ"),
        utc_end.strftime("%Y-%m-%dT%H:%M:%SZ"),
        tz_name,
    )


def utc_to_local(utc_iso: str, tz_name: str) -> str:
    """ISO UTC string -> 'HH:MM' local. Returns '?' on failure."""
    if not utc_iso:
        return "?"
    try:
        dt = datetime.strptime(utc_iso[:19], "%Y-%m-%dT%H:%M:%S").replace(tzinfo=timezone.utc)
        return dt.astimezone(ZoneInfo(tz_name)).strftime("%H:%M")
    except (ValueError, TypeError):
        return "?"


def in_window(utc_iso: str, utc_start: str, utc_end: str) -> bool:
    """True if utc_iso lies in [utc_start, utc_end)."""
    if not utc_iso:
        return False
    return utc_start <= utc_iso < utc_end


# -- Roster -----------------------------------------------------------

def load_roster(path: Path = ROSTER_PATH) -> dict:
    return json.loads(path.read_text())


def author_for(
    project_id: str | None,
    files_touched: list | None,
    roster: dict,
    user: str | None = None,
) -> str:
    """Resolve an author name from session metadata.

    Priority: explicit user (OS username on the session record) > project_id
    path > files_touched paths. Returns 'unknown' if nothing matches.
    """
    if user:
        for member in roster["members"]:
            if user in member.get("users", []):
                return member["name"]
    pid = project_id or ""
    for member in roster["members"]:
        for prefix in member["path_prefixes"]:
            if prefix in pid:
                return member["name"]
    if files_touched:
        for f in files_touched:
            p = f.get("path", "") if isinstance(f, dict) else str(f)
            for member in roster["members"]:
                for prefix in member["path_prefixes"]:
                    if prefix in p:
                        return member["name"]
    return "unknown"


# -- IO ---------------------------------------------------------------

def read_json(path: str | None) -> Any:
    """Read JSON from path, or stdin if path is None or '-'."""
    if path is None or path == "-":
        return json.load(sys.stdin)
    return json.loads(Path(path).read_text())


def write_json(data: Any, path: str | None) -> None:
    """Write JSON to path, or stdout if path is None or '-'."""
    payload = json.dumps(data, indent=2, default=str)
    if path is None or path == "-":
        sys.stdout.write(payload + "\n")
    else:
        Path(path).write_text(payload + "\n")


def add_io_args(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--in", dest="input", default="-", help="input file (default stdin)")
    parser.add_argument("--out", dest="output", default="-", help="output file (default stdout)")
    parser.add_argument(
        "--url", default=DEFAULT_URL, help=f"OpenStory base URL (default {DEFAULT_URL})"
    )
