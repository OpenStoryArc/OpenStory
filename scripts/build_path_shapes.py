#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Build the path-shape semantic layer.

For every tool_call that touches a file path (Read/Edit/Write/Grep/Glob/etc.),
decompose the path into directory chain, basename, extension, top project
segment, and naming tokens (camelCase / snake_case / kebab-case split).

Idempotent on (event_id, path) — re-running only inserts new touches.

Usage:
    uv run scripts/build_path_shapes.py
    uv run scripts/build_path_shapes.py --rebuild
    uv run scripts/build_path_shapes.py --limit 200

Design: docs/research/semantic-layer.md ("shape layers" family)
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
import time
import urllib.request
from dataclasses import dataclass
from pathlib import PurePosixPath

API = "http://localhost:3002/api"
DEFAULT_DB = "data/path_shapes.db"

# Tool input keys that carry a single path argument
PATH_KEYS = ("file_path", "notebook_path", "path")

SCHEMA = """
CREATE TABLE IF NOT EXISTS path_shapes (
    event_id        TEXT NOT NULL,
    session_id      TEXT NOT NULL,
    timestamp       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    tool            TEXT NOT NULL,
    path            TEXT NOT NULL,
    directory       TEXT NOT NULL,
    basename        TEXT NOT NULL,
    stem            TEXT NOT NULL,
    extension       TEXT NOT NULL,
    depth           INTEGER NOT NULL,
    top_segment     TEXT NOT NULL,
    dir_segments    TEXT NOT NULL,
    naming_tokens   TEXT NOT NULL,
    absolute        INTEGER NOT NULL,
    PRIMARY KEY (event_id, path)
);
CREATE INDEX IF NOT EXISTS idx_path_session ON path_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_path_top     ON path_shapes(top_segment);
CREATE INDEX IF NOT EXISTS idx_path_ext     ON path_shapes(extension);
"""

# Tokens to drop from the naming-vocab (too common to be signal)
NAMING_STOPWORDS = {"src", "lib", "js", "ts", "tsx", "rs", "py", "md", "test", "tests"}


@dataclass
class PathEvent:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    tool: str
    path: str


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def iter_path_events(session_id: str):
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
        tool = payload.get("name", "")
        inp = payload.get("input") or {}
        if not isinstance(inp, dict):
            continue
        for key in PATH_KEYS:
            val = inp.get(key)
            if not isinstance(val, str) or not val.strip():
                continue
            # skip glob patterns and shell-meta paths
            if "*" in val or "?" in val:
                continue
            yield PathEvent(
                event_id=rec.get("id", ""),
                session_id=session_id,
                timestamp=rec.get("timestamp", ""),
                seq=int(rec.get("seq") or 0),
                tool=tool,
                path=val.strip(),
            )


# Project-relative anchor: anything after this is the "project view" of the path
PROJECT_ANCHORS = ("/OpenStory/", "/openstory/")


def normalize_for_top_segment(path: str) -> str:
    for anchor in PROJECT_ANCHORS:
        i = path.find(anchor)
        if i >= 0:
            return path[i + len(anchor):]
    return path


CAMEL_SPLIT_RE = re.compile(r"(?<=[a-z0-9])(?=[A-Z])|(?<=[A-Z])(?=[A-Z][a-z])")


def tokenize_stem(stem: str) -> list[str]:
    """Split a file stem into naming tokens. Handles snake / kebab / camel."""
    # First split on snake / kebab / dot separators
    parts = re.split(r"[_\-.]+", stem)
    # Then split each part on camelCase boundaries
    tokens: list[str] = []
    for part in parts:
        if not part:
            continue
        for sub in CAMEL_SPLIT_RE.split(part):
            sub = sub.lower().strip()
            if not sub or sub.isdigit() or len(sub) < 2:
                continue
            if sub in NAMING_STOPWORDS:
                continue
            tokens.append(sub)
    return tokens


def decompose(path: str) -> dict:
    is_abs = path.startswith("/")
    rel = normalize_for_top_segment(path)
    posix = PurePosixPath(rel)
    parts = list(posix.parts)
    basename = parts[-1] if parts else ""
    dir_segments = parts[:-1]
    directory = "/".join(dir_segments) if dir_segments else ""
    depth = len(parts) - 1
    top_segment = dir_segments[0] if dir_segments else ""
    extension = posix.suffix.lower()
    stem = posix.stem
    # for compound extensions like `.spec.ts` recover the inner stem
    while "." in stem and PurePosixPath(stem).suffix:
        stem = PurePosixPath(stem).stem
    naming_tokens = tokenize_stem(stem)
    return {
        "directory": directory,
        "basename": basename,
        "stem": stem,
        "extension": extension,
        "depth": depth,
        "top_segment": top_segment,
        "dir_segments": dir_segments,
        "naming_tokens": naming_tokens,
        "absolute": 1 if is_abs else 0,
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--limit", type=int, default=0, help="cap number of sessions (0 = all)")
    ap.add_argument("--rebuild", action="store_true", help="drop the table first")
    args = ap.parse_args()

    from pathlib import Path as Pp
    db_path = Pp(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    if args.rebuild:
        conn.executescript("DROP TABLE IF EXISTS path_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {(r[0], r[1]) for r in conn.execute("SELECT event_id, path FROM path_shapes")}
    print(f"existing path touches: {len(existing)}", file=sys.stderr)

    print("fetching sessions...", file=sys.stderr)
    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for path touches...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        for ev in iter_path_events(sid):
            if (ev.event_id, ev.path) in existing or not ev.event_id:
                continue
            d = decompose(ev.path)
            batch.append((
                ev.event_id,
                ev.session_id,
                ev.timestamp,
                ev.seq,
                ev.tool,
                ev.path,
                d["directory"],
                d["basename"],
                d["stem"],
                d["extension"],
                d["depth"],
                d["top_segment"],
                json.dumps(d["dir_segments"]),
                json.dumps(d["naming_tokens"]),
                d["absolute"],
            ))
            existing.add((ev.event_id, ev.path))
            if len(batch) >= 500:
                conn.executemany(
                    """INSERT OR IGNORE INTO path_shapes
                       (event_id, session_id, timestamp, seq, tool, path,
                        directory, basename, stem, extension, depth, top_segment,
                        dir_segments, naming_tokens, absolute)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO path_shapes
               (event_id, session_id, timestamp, seq, tool, path,
                directory, basename, stem, extension, depth, top_segment,
                dir_segments, naming_tokens, absolute)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} path touches in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
