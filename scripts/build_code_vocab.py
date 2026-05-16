#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Build the code-vocab semantic layer.

For every Edit / Write / MultiEdit tool_call against a code file, extract
identifiers (function names, variable names, type names) from the new code.

This is a regex-based extractor (per-language refinement is the upgrade
path; tree-sitter would be the rigorous form). For each event, store a
JSON dict mapping `identifier -> count`. Idempotent on event_id.

Languages handled: .rs .py .ts .tsx .js .jsx .go .java .c .cpp .h .hpp.
Other extensions are stored with an empty identifier map.

Usage:
    uv run scripts/build_code_vocab.py
    uv run scripts/build_code_vocab.py --limit 200 --rebuild

Design: docs/research/semantic-layer.md
"""

from __future__ import annotations

import argparse
import json
import re
import sqlite3
import sys
import time
import urllib.request
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

API = "http://localhost:3002/api"
DEFAULT_DB = "data/code_vocab_shapes.db"

CHANGE_TOOLS = {"Edit", "Write", "MultiEdit"}
EXT_TO_LANG = {
    ".rs": "rust",
    ".py": "python",
    ".ts": "typescript",
    ".tsx": "typescript",
    ".js": "javascript",
    ".jsx": "javascript",
    ".go": "go",
    ".java": "java",
    ".c": "c", ".h": "c",
    ".cpp": "cpp", ".hpp": "cpp",
}

# Keywords + common stopwords to exclude from identifier vocabulary.
# Per-language refinement is possible but a unified list catches 95%.
STOPWORDS = {
    # universal
    "if", "else", "for", "while", "do", "return", "true", "false", "null", "none",
    "this", "self", "super", "new", "delete", "let", "const", "var", "type",
    "function", "fn", "def", "class", "struct", "enum", "trait", "impl", "interface",
    "public", "private", "protected", "static", "final", "abstract", "override",
    "import", "export", "from", "as", "in", "of", "and", "or", "not", "is",
    "try", "catch", "finally", "throw", "raise", "with", "yield", "await", "async",
    "break", "continue", "match", "case", "switch", "default", "void", "extends",
    "implements", "package", "use", "mod", "pub", "crate", "ref", "mut", "where",
    # very common short identifiers
    "i", "j", "k", "x", "y", "z", "n", "m", "a", "b", "c", "e",
    "id", "ok", "err", "val", "key", "out", "fn", "args", "kwargs",
    # python builtins (frequent)
    "print", "len", "str", "int", "float", "list", "dict", "set", "tuple",
    "bool", "range", "enumerate", "zip", "map", "filter",
    # JS/TS/JSX noise
    "div", "span", "className", "style", "props", "children", "useState", "useEffect",
    "import", "default", "from", "export",
    # Rust noise
    "Self", "Some", "None", "Ok", "Err", "Result", "Option", "Vec", "String",
}

IDENT_RE = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\b")

SCHEMA = """
CREATE TABLE IF NOT EXISTS code_vocab_shapes (
    event_id      TEXT PRIMARY KEY,
    session_id    TEXT NOT NULL,
    timestamp     TEXT NOT NULL,
    seq           INTEGER NOT NULL,
    tool          TEXT NOT NULL,
    path          TEXT NOT NULL,
    language      TEXT NOT NULL,
    identifiers   TEXT NOT NULL,    -- JSON {identifier: count}
    unique_idents INTEGER NOT NULL,
    total_idents  INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_vocab_session ON code_vocab_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_vocab_lang    ON code_vocab_shapes(language);
"""


@dataclass
class VocabEvent:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    tool: str
    path: str
    language: str
    identifiers: dict[str, int]


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def language_of(path: str) -> str:
    for ext, lang in EXT_TO_LANG.items():
        if path.endswith(ext):
            return lang
    return ""


def extract_identifiers(text: str) -> Counter:
    if not text:
        return Counter()
    out: Counter = Counter()
    for m in IDENT_RE.finditer(text):
        tok = m.group(0)
        if tok in STOPWORDS:
            continue
        # skip pure-digit and very short
        if len(tok) < 3:
            continue
        # skip ALL_UPPER constants? — keep them, they're meaningful
        out[tok] += 1
    return out


def iter_vocab_events(session_id: str):
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
        if tool not in CHANGE_TOOLS:
            continue
        inp = payload.get("input") or {}
        if not isinstance(inp, dict):
            continue
        path = inp.get("file_path") or inp.get("notebook_path") or ""
        if not isinstance(path, str) or not path.strip():
            continue
        lang = language_of(path)
        if not lang:
            continue  # skip non-code files for vocab

        text = ""
        if tool == "Edit":
            text = inp.get("new_string") or ""
        elif tool == "Write":
            text = inp.get("content") or ""
        elif tool == "MultiEdit":
            edits = inp.get("edits") or []
            if isinstance(edits, list):
                text = "\n".join(
                    (e.get("new_string") or "") for e in edits if isinstance(e, dict)
                )

        idents = extract_identifiers(text)
        if not idents:
            continue

        yield VocabEvent(
            event_id=rec.get("id", ""),
            session_id=session_id,
            timestamp=rec.get("timestamp", ""),
            seq=int(rec.get("seq") or 0),
            tool=tool,
            path=path.strip(),
            language=lang,
            identifiers=dict(idents),
        )


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB)
    ap.add_argument("--limit", type=int, default=0)
    ap.add_argument("--rebuild", action="store_true")
    args = ap.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(db_path)
    if args.rebuild:
        conn.executescript("DROP TABLE IF EXISTS code_vocab_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {r[0] for r in conn.execute("SELECT event_id FROM code_vocab_shapes")}
    print(f"existing vocab events: {len(existing)}", file=sys.stderr)

    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for code identifiers...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        for ev in iter_vocab_events(sid):
            if ev.event_id in existing or not ev.event_id:
                continue
            total = sum(ev.identifiers.values())
            batch.append((
                ev.event_id, ev.session_id, ev.timestamp, ev.seq, ev.tool,
                ev.path, ev.language, json.dumps(ev.identifiers),
                len(ev.identifiers), total,
            ))
            existing.add(ev.event_id)
            if len(batch) >= 300:
                conn.executemany(
                    """INSERT OR IGNORE INTO code_vocab_shapes
                       (event_id, session_id, timestamp, seq, tool, path, language,
                        identifiers, unique_idents, total_idents)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO code_vocab_shapes
               (event_id, session_id, timestamp, seq, tool, path, language,
                identifiers, unique_idents, total_idents)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} code-vocab events in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
