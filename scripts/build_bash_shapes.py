#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Build the bash-shape semantic layer.

For every `tool_call` with `name == "Bash"`, tokenize the shell command into
program / subcommand / flags / args, plus pipeline/redirect flags.

Idempotent on `event_id`. Re-running only inserts new commands.

Usage:
    uv run scripts/build_bash_shapes.py
    uv run scripts/build_bash_shapes.py --rebuild
    uv run scripts/build_bash_shapes.py --limit 200

Design: docs/research/semantic-layer.md ("shape layers" family)
"""

from __future__ import annotations

import argparse
import json
import shlex
import sqlite3
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import Path

API = "http://localhost:3002/api"
DEFAULT_DB = "data/bash_shapes.db"

# Programs that take a subcommand as their second token (think `git status`,
# `cargo test`). Anything outside this set has empty `subcommand`.
SUBCOMMAND_PROGRAMS = {
    "git", "cargo", "npm", "just", "gh", "docker", "kubectl",
    "brew", "uv", "pip", "pip3", "poetry", "rustup", "nats", "rg",
}

SCHEMA = """
CREATE TABLE IF NOT EXISTS bash_shapes (
    event_id      TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL,
    timestamp     TEXT NOT NULL,
    seq           INTEGER NOT NULL,
    program       TEXT NOT NULL,
    subcommand    TEXT NOT NULL,
    flags         TEXT NOT NULL,
    args          TEXT NOT NULL,
    is_pipeline   INTEGER NOT NULL,
    is_redirect   INTEGER NOT NULL,
    raw_command   TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_bash_session ON bash_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_bash_program ON bash_shapes(program);
"""


@dataclass
class BashEvent:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    command: str


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def iter_bash_events(session_id: str):
    try:
        data = fetch_json(f"/sessions/{session_id}/records")
    except Exception as e:
        print(f"  warn: failed to fetch {session_id}: {e}", file=sys.stderr)
        return
    records = data if isinstance(data, list) else data.get("records", [])
    for rec in records:
        if rec.get("record_type") != "tool_call":
            continue
        payload = rec.get("payload") or {}
        if payload.get("name") != "Bash":
            continue
        inp = payload.get("input") or {}
        cmd = inp.get("command") if isinstance(inp, dict) else None
        if not isinstance(cmd, str) or not cmd.strip():
            continue
        yield BashEvent(
            event_id=rec.get("id", ""),
            session_id=session_id,
            timestamp=rec.get("timestamp", ""),
            seq=int(rec.get("seq") or 0),
            command=cmd.strip(),
        )


def _outside_quotes(command: str, char: str) -> bool:
    """True if `char` appears in `command` outside single/double quotes."""
    in_single = False
    in_double = False
    i = 0
    while i < len(command):
        c = command[i]
        if c == "\\" and i + 1 < len(command):
            i += 2
            continue
        if c == "'" and not in_double:
            in_single = not in_single
        elif c == '"' and not in_single:
            in_double = not in_double
        elif c == char and not in_single and not in_double:
            return True
        i += 1
    return False


def decompose(command: str) -> dict:
    try:
        tokens = shlex.split(command, posix=True)
    except ValueError:
        tokens = command.split()
    program = tokens[0] if tokens else ""
    # strip absolute-path prefix on `program` so `/usr/bin/git` → `git`
    if "/" in program:
        program = program.rsplit("/", 1)[-1]
    rest = tokens[1:]

    subcommand = ""
    if program in SUBCOMMAND_PROGRAMS and rest:
        # first non-flag token after the program
        for t in rest:
            if not t.startswith("-"):
                subcommand = t
                break

    flags = [t for t in rest if t.startswith("-")]
    args = [t for t in rest if not t.startswith("-") and t != subcommand]

    return {
        "program": program,
        "subcommand": subcommand,
        "flags": flags,
        "args": args,
        "is_pipeline": 1 if _outside_quotes(command, "|") else 0,
        "is_redirect": 1 if _outside_quotes(command, ">") else 0,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--limit", type=int, default=0, help="cap number of sessions (0 = all)")
    ap.add_argument("--rebuild", action="store_true", help="drop the table first")
    args = ap.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    if args.rebuild:
        conn.executescript("DROP TABLE IF EXISTS bash_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {r[0] for r in conn.execute("SELECT event_id FROM bash_shapes")}
    print(f"existing bash shapes: {len(existing)}", file=sys.stderr)

    print("fetching sessions...", file=sys.stderr)
    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for bash commands...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        for ev in iter_bash_events(sid):
            if ev.event_id in existing or not ev.event_id:
                continue
            d = decompose(ev.command)
            batch.append((
                ev.event_id,
                ev.session_id,
                ev.timestamp,
                ev.seq,
                d["program"],
                d["subcommand"],
                json.dumps(d["flags"]),
                json.dumps(d["args"]),
                d["is_pipeline"],
                d["is_redirect"],
                ev.command[:300],
            ))
            existing.add(ev.event_id)
            if len(batch) >= 500:
                conn.executemany(
                    """INSERT OR IGNORE INTO bash_shapes
                       (event_id, session_id, timestamp, seq, program, subcommand,
                        flags, args, is_pipeline, is_redirect, raw_command)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO bash_shapes
               (event_id, session_id, timestamp, seq, program, subcommand,
                flags, args, is_pipeline, is_redirect, raw_command)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} bash shapes in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
