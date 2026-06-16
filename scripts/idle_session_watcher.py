"""Watch OpenStory sessions and emit when one goes idle — the trigger for a story.

Observe, never interfere: this only reads the live store through the REST API
(`/api/sessions`). It never writes back, never touches transcripts. When a *watched*
session crosses from active → idle (no events for `--threshold` seconds, or the store
marks it `completed`), it prints one JSON line per transition. That line is the signal
to narrate a session story (e.g. via the OpenStory MCP `session_story` tool).

Designed to be the `command` of a Monitor / `tail -f`-style watch: each emitted line
is an event. Baseline on first run is silent — we only fire on transitions we *observe*
(a session that was active under our eyes and then went quiet), never on the backlog of
already-idle sessions. Pass --report-existing to also fire for sessions already idle at
startup.

Filtering: by default we watch every session. Pass --match (repeatable) to scope to a
teammate's machine — a substring of their host/user/project, e.g. `--match teammate`.

Usage:
    python3 scripts/idle_session_watcher.py --watch                 # loop, default 60s
    python3 scripts/idle_session_watcher.py --once                  # single poll
    python3 scripts/idle_session_watcher.py --watch --all           # every session
    python3 scripts/idle_session_watcher.py --watch --match teammate  # custom filter
    python3 scripts/idle_session_watcher.py --threshold 120         # idle after 2m
    python3 scripts/idle_session_watcher.py --test                  # self-tests

Companion to scripts/sessionstory.py (narration) and the OpenStory MCP. This script
finds *which* session to tell a story about and *when*; the story itself is the
narrator's job.
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone


DEFAULT_URL = "http://localhost:3002"
DEFAULT_THRESHOLD = 300  # mirrors OpenStory's stale_threshold_secs default
DEFAULT_MATCH: list[str] = []  # watch all sessions; pass --match to scope to a teammate
DEFAULT_STATE = "/tmp/openstory_idle_watcher_state.json"


# -- HTTP -------------------------------------------------------------

def fetch_sessions(base_url: str) -> list[dict]:
    url = f"{base_url}/api/sessions"
    with urllib.request.urlopen(url, timeout=30) as resp:
        data = json.loads(resp.read())
    return data if isinstance(data, list) else data.get("sessions", [])


# -- pure logic (tested) ----------------------------------------------

def parse_iso(ts: str) -> datetime | None:
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return None


def matches(session: dict, filters: list[str]) -> bool:
    """True if any filter substring appears in host/user/project_id (case-insensitive)."""
    if not filters:  # --all
        return True
    hay = " ".join(
        str(session.get(k, "")) for k in ("host", "user", "project_id", "project_name")
    ).lower()
    return any(f.lower() in hay for f in filters)


def idle_seconds(session: dict, now: datetime) -> float | None:
    last = parse_iso(session.get("last_event", ""))
    if last is None:
        return None
    return (now - last).total_seconds()


def is_idle(session: dict, now: datetime, threshold: int) -> bool:
    """A session is idle if the store calls it completed, or it's been quiet too long."""
    if str(session.get("status", "")).lower() in ("completed", "stale", "ended"):
        return True
    secs = idle_seconds(session, now)
    return secs is not None and secs >= threshold


def detect_transitions(
    prev_state: dict,
    sessions: list[dict],
    now: datetime,
    threshold: int,
    filters: list[str],
    report_existing: bool,
) -> tuple[dict, list[dict]]:
    """Pure core: fold (prev_state, current sessions) → (new_state, sessions to report).

    A session reports exactly once, when it crosses active → idle. First sight of an
    already-idle session is recorded as reported unless report_existing is set, so a
    cold start does not flood. State per session: {"active": bool, "reported": bool}.
    """
    new_state: dict = {}
    to_report: list[dict] = []

    for s in sessions:
        if not matches(s, filters):
            continue
        sid = s.get("session_id") or s.get("id")
        if not sid:
            continue
        idle = is_idle(s, now, threshold)
        prev = prev_state.get(sid)

        if prev is None:
            # First sight. Active → arm it. Idle → record as reported (silent baseline),
            # unless the caller explicitly wants the existing backlog reported.
            if idle and not report_existing:
                new_state[sid] = {"active": False, "reported": True}
            elif idle and report_existing:
                new_state[sid] = {"active": False, "reported": True}
                to_report.append(s)
            else:
                new_state[sid] = {"active": True, "reported": False}
            continue

        # Seen before. Fire only on active→idle, once.
        if idle and prev.get("active") and not prev.get("reported"):
            new_state[sid] = {"active": False, "reported": True}
            to_report.append(s)
        elif not idle:
            # Came back to life — re-arm so a later quiet period reports again.
            new_state[sid] = {"active": True, "reported": False}
        else:
            new_state[sid] = {"active": False, "reported": prev.get("reported", True)}

    # Carry forward sessions not in this poll (e.g. filtered list unchanged).
    for sid, st in prev_state.items():
        new_state.setdefault(sid, st)
    return new_state, to_report


def emit_line(session: dict, now: datetime) -> str:
    secs = idle_seconds(session, now)
    return json.dumps(
        {
            "event": "session_idle",
            "session_id": session.get("session_id") or session.get("id"),
            "user": session.get("user"),
            "host": session.get("host"),
            "branch": session.get("branch"),
            "label": (session.get("label") or "")[:80],
            "idle_secs": round(secs) if secs is not None else None,
            "last_event": session.get("last_event"),
            "detected_at": now.isoformat(),
        }
    )


# -- state I/O (edge) -------------------------------------------------

def load_state(path: str) -> dict:
    try:
        with open(path) as f:
            return json.load(f)
    except (OSError, ValueError):
        return {}


def save_state(path: str, state: dict) -> None:
    try:
        with open(path, "w") as f:
            json.dump(state, f)
    except OSError as e:
        sys.stderr.write(f"warn: could not persist state to {path}: {e}\n")


# -- driver -----------------------------------------------------------

def poll_once(args, state: dict) -> dict:
    try:
        sessions = fetch_sessions(args.url)
    except urllib.error.URLError as e:
        sys.stderr.write(f"warn: fetch failed: {e}\n")
        return state  # transient; keep state, try again next tick
    now = datetime.now(timezone.utc)
    new_state, to_report = detect_transitions(
        state, sessions, now, args.threshold, args.filters, args.report_existing
    )
    for s in to_report:
        print(emit_line(s, now), flush=True)
    return new_state


def run(args) -> int:
    state = load_state(args.state_file)
    if args.once:
        state = poll_once(args, state)
        save_state(args.state_file, state)
        return 0
    # --watch loop
    sys.stderr.write(
        f"idle-watcher: polling {args.url} every {args.interval}s, "
        f"threshold {args.threshold}s, filters={args.filters or 'ALL'}\n"
    )
    while True:
        state = poll_once(args, state)
        save_state(args.state_file, state)
        time.sleep(args.interval)


# -- self-tests -------------------------------------------------------

def selftest() -> int:
    failures = []

    def check(name, cond, detail=""):
        mark = "ok  " if cond else "FAIL"
        print(f"[{mark}] {name}" + (f" — {detail}" if detail and not cond else ""))
        if not cond:
            failures.append(name)

    now = datetime(2026, 6, 10, 0, 10, 0, tzinfo=timezone.utc)
    team = ["teammate"]  # an explicit filter scoping to one teammate's machine
    peer = {
        "session_id": "k1", "user": "teammate", "host": "Teammates-Mac-mini",
        "project_id": "-Users-teammate-workspace-OpenStory",
        "last_event": "2026-06-10T00:09:30+00:00", "status": "active",  # 30s ago → active
    }
    mine = {
        "session_id": "m1", "user": "self", "host": "Self-MBP",
        "project_id": "-Users-self-projects-OpenStory",
        "last_event": "2026-06-10T00:09:50+00:00", "status": "active",
    }

    check("matches teammate when filtered", matches(peer, team))
    check("filters out self when scoped to teammate", not matches(mine, team))
    check("matches all when no filters", matches(mine, []))

    check("fresh event is active", not is_idle(peer, now, 300))
    old = dict(peer, last_event="2026-06-10T00:04:00+00:00")  # 6m ago
    check("stale event is idle", is_idle(old, now, 300))
    check("completed status is idle", is_idle(dict(peer, status="completed"), now, 300))

    # Cold start with an active session: armed, silent.
    st1, rep1 = detect_transitions({}, [peer], now, 300, team, False)
    check("cold start active → no report", rep1 == [], f"got {rep1}")
    check("cold start arms session", st1["k1"] == {"active": True, "reported": False})

    # Cold start already-idle: silent baseline (no flood).
    st_b, rep_b = detect_transitions({}, [old], now, 300, team, False)
    check("cold start idle → silent", rep_b == [] and st_b["k1"]["reported"])

    # Cold start already-idle with --report-existing: fires.
    _, rep_e = detect_transitions({}, [old], now, 300, team, True)
    check("report_existing fires for backlog", len(rep_e) == 1)

    # The real transition: active (armed) → idle fires exactly once.
    later = now.replace(minute=20)  # 10 min later; peer's last_event now 10.5m old
    st2, rep2 = detect_transitions(st1, [peer], later, 300, team, False)
    check("active→idle fires once", len(rep2) == 1 and rep2[0]["session_id"] == "k1")
    check("after firing, marked reported", st2["k1"] == {"active": False, "reported": True})

    # No double-fire on the next poll.
    _, rep3 = detect_transitions(st2, [peer], later, 300, team, False)
    check("no double fire", rep3 == [])

    # Re-activation re-arms.
    revived = dict(peer, last_event=later.isoformat())
    st4, _ = detect_transitions(st2, [revived], later, 300, team, False)
    check("re-activation re-arms", st4["k1"] == {"active": True, "reported": False})

    print()
    print("PASS" if not failures else f"FAIL ({len(failures)})")
    return 0 if not failures else 1


def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("--url", default=os.environ.get("OPENSTORY_URL", DEFAULT_URL))
    p.add_argument("--threshold", type=int, default=DEFAULT_THRESHOLD,
                   help="seconds of quiet before a session is idle (default 300)")
    p.add_argument("--interval", type=int, default=60, help="poll interval for --watch")
    p.add_argument("--match", action="append", default=None,
                   help="substring to match host/user/project (repeatable). "
                        "Default: watch all sessions.")
    p.add_argument("--all", action="store_true", help="watch every session (no filter)")
    p.add_argument("--report-existing", action="store_true",
                   help="also fire for sessions already idle at startup")
    p.add_argument("--state-file", default=DEFAULT_STATE)
    mode = p.add_mutually_exclusive_group()
    mode.add_argument("--once", action="store_true", help="single poll then exit")
    mode.add_argument("--watch", action="store_true", help="loop forever (default)")
    p.add_argument("--test", action="store_true", help="run self-tests and exit")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())
    args.filters = [] if args.all else (args.match or DEFAULT_MATCH)
    if not args.once and not args.watch:
        args.watch = True
    sys.exit(run(args))


if __name__ == "__main__":
    main()
