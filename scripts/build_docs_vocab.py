#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "spacy==3.7.5",
#     "en-core-web-sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.7.1/en_core_web_sm-3.7.1-py3-none-any.whl",
# ]
# ///
"""Build the docs-vocab semantic layer.

For every Edit / Write / MultiEdit tool_call against a prose file (.md /
.markdown / .rst / .adoc / .txt), extract the named concepts that appear:

  - headers       (lines starting with #, with the marker stripped)
  - bold_terms    (text inside ** ** markers)
  - link_labels   (the visible text in [label](url) links)
  - noun_chunks   (spaCy-extracted noun phrases from the body prose)

The body is preprocessed to strip code fences (we don't want code identifiers
showing up as "noun chunks") and inline markdown syntax before parsing.

Idempotent on event_id.

Usage:
    uv run scripts/build_docs_vocab.py
    uv run scripts/build_docs_vocab.py --limit 200 --rebuild

Design: docs/research/semantic-layer.md (the "shape layers" family)
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

import spacy

API = "http://localhost:3002/api"
DEFAULT_DB = "data/docs_vocab_shapes.db"

CHANGE_TOOLS = {"Edit", "Write", "MultiEdit"}
PROSE_EXTENSIONS = (".md", ".markdown", ".rst", ".adoc", ".txt")

# Markdown-aware regex patterns (used pre-spaCy)
HEADER_RE      = re.compile(r"^\s{0,3}(#{1,6})\s+(.+?)\s*#*\s*$", re.MULTILINE)
RST_HEADER_RE  = re.compile(r"^(.+)\n[=\-~^\"'*+]{3,}\s*$", re.MULTILINE)
ADOC_HEADER_RE = re.compile(r"^={1,6}\s+(.+?)\s*$", re.MULTILINE)
BOLD_RE        = re.compile(r"\*\*([^*\n]{2,80})\*\*")
LINK_RE        = re.compile(r"\[([^\]\n]{2,80})\]\(([^)\s]+)\)")
FENCE_RE       = re.compile(r"```.*?```", re.DOTALL)
INLINE_CODE_RE = re.compile(r"`[^`\n]+`")
HTML_TAG_RE    = re.compile(r"<[^>\n]+>")
MD_MARKER_RE   = re.compile(r"[*_~]{1,3}([^*_~\n]+)[*_~]{1,3}")

# Noun-chunk stop-list: too generic to count as a project-level concept
CHUNK_STOPWORDS = {
    "this", "that", "these", "those", "it", "you", "we", "i",
    "the", "a", "an", "some", "any", "one", "two", "three",
}


SCHEMA = """
CREATE TABLE IF NOT EXISTS docs_vocab_shapes (
    event_id        TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    timestamp       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    tool            TEXT NOT NULL,
    path            TEXT NOT NULL,
    extension       TEXT NOT NULL,
    headers         TEXT NOT NULL,    -- JSON list of strings
    bold_terms      TEXT NOT NULL,    -- JSON dict {term: count}
    link_labels     TEXT NOT NULL,    -- JSON dict {label: count}
    noun_chunks     TEXT NOT NULL,    -- JSON dict {chunk: count}
    char_count      INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_docs_session ON docs_vocab_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_docs_ext     ON docs_vocab_shapes(extension);
"""


@dataclass
class DocsEvent:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    tool: str
    path: str
    extension: str
    text: str


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def extension_of(path: str) -> str:
    p = path.lower()
    for ext in PROSE_EXTENSIONS:
        if p.endswith(ext):
            return ext
    return ""


def extract_headers(text: str, ext: str) -> list[str]:
    """Pull headers as plain strings, marker-stripped."""
    out: list[str] = []
    if ext in (".md", ".markdown"):
        for m in HEADER_RE.finditer(text):
            t = m.group(2).strip()
            # strip trailing closing #'s, bold markers, links
            t = re.sub(r"\*\*([^*]+)\*\*", r"\1", t)
            t = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", t)
            t = re.sub(r"`([^`]+)`", r"\1", t)
            if t:
                out.append(t)
    elif ext == ".rst":
        for m in RST_HEADER_RE.finditer(text):
            t = m.group(1).strip()
            if t and not t.startswith((".", "..")):
                out.append(t)
    elif ext == ".adoc":
        for m in ADOC_HEADER_RE.finditer(text):
            t = m.group(1).strip()
            if t:
                out.append(t)
    return out


def extract_bold(text: str) -> Counter:
    c: Counter = Counter()
    for m in BOLD_RE.finditer(text):
        term = m.group(1).strip()
        if 2 <= len(term) <= 80:
            c[term] += 1
    return c


def extract_links(text: str) -> Counter:
    c: Counter = Counter()
    for m in LINK_RE.finditer(text):
        label = m.group(1).strip()
        # skip pure-URL labels and very short labels
        if label and not label.startswith(("http", "/")):
            c[label] += 1
    return c


def preprocess_for_prose(text: str) -> str:
    """Strip code fences + inline code + markdown markers + HTML tags for spaCy."""
    s = FENCE_RE.sub(" ", text)
    s = INLINE_CODE_RE.sub(" ", s)
    s = HTML_TAG_RE.sub(" ", s)
    # collapse markdown emphasis markers (keep the text inside)
    s = re.sub(r"\*\*([^*\n]+)\*\*", r"\1", s)
    s = re.sub(r"\*([^*\n]+)\*", r"\1", s)
    s = re.sub(r"__([^_\n]+)__", r"\1", s)
    # link [label](url) → label
    s = re.sub(r"\[([^\]\n]+)\]\([^)\s]+\)", r"\1", s)
    # strip markdown header markers from line starts
    s = re.sub(r"^\s{0,3}#{1,6}\s+", "", s, flags=re.MULTILINE)
    # strip table borders and list bullets at line start
    s = re.sub(r"^\s*[\|\-+\*•]\s*", "", s, flags=re.MULTILINE)
    return s


def extract_noun_chunks(nlp, text: str) -> Counter:
    c: Counter = Counter()
    # cap to keep spaCy fast on large docs
    doc = nlp(text[:50000])
    for chunk in doc.noun_chunks:
        lemma = chunk.lemma_.lower().strip()
        if len(lemma) < 3 or len(lemma) > 80:
            continue
        if lemma in CHUNK_STOPWORDS:
            continue
        if chunk.root.pos_ == "PRON":
            continue
        # skip chunks that are entirely stopwords
        if all(t.is_stop for t in chunk):
            continue
        c[lemma] += 1
    return c


def iter_docs_events(session_id: str):
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
        ext = extension_of(path)
        if not ext:
            continue

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
            else:
                text = ""
        else:
            text = ""

        if not text.strip():
            continue

        yield DocsEvent(
            event_id=rec.get("id", ""),
            session_id=session_id,
            timestamp=rec.get("timestamp", ""),
            seq=int(rec.get("seq") or 0),
            tool=tool,
            path=path.strip(),
            extension=ext,
            text=text,
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
        conn.executescript("DROP TABLE IF EXISTS docs_vocab_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {r[0] for r in conn.execute("SELECT event_id FROM docs_vocab_shapes")}
    print(f"existing docs-vocab events: {len(existing)}", file=sys.stderr)

    print("loading spaCy en_core_web_sm...", file=sys.stderr)
    nlp = spacy.load("en_core_web_sm", disable=["ner"])

    sd = fetch_json("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    print(f"scanning {len(sessions)} sessions for prose edits...", file=sys.stderr)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []

    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, inserted {inserted}", file=sys.stderr)
        for ev in iter_docs_events(sid):
            if ev.event_id in existing or not ev.event_id:
                continue
            headers = extract_headers(ev.text, ev.extension)
            bold = extract_bold(ev.text)
            links = extract_links(ev.text)
            prose = preprocess_for_prose(ev.text)
            chunks = extract_noun_chunks(nlp, prose)
            batch.append((
                ev.event_id, ev.session_id, ev.timestamp, ev.seq, ev.tool,
                ev.path, ev.extension,
                json.dumps(headers),
                json.dumps(dict(bold)),
                json.dumps(dict(links)),
                json.dumps(dict(chunks)),
                len(ev.text),
            ))
            existing.add(ev.event_id)
            if len(batch) >= 200:
                conn.executemany(
                    """INSERT OR IGNORE INTO docs_vocab_shapes
                       (event_id, session_id, timestamp, seq, tool, path, extension,
                        headers, bold_terms, link_labels, noun_chunks, char_count)
                       VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                    batch,
                )
                conn.commit()
                inserted += len(batch)
                batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO docs_vocab_shapes
               (event_id, session_id, timestamp, seq, tool, path, extension,
                headers, bold_terms, link_labels, noun_chunks, char_count)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} docs-vocab events in {elapsed:.1f}s", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
