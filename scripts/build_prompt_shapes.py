#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# dependencies = [
#     "spacy==3.7.5",
#     "en-core-web-sm @ https://github.com/explosion/spacy-models/releases/download/en_core_web_sm-3.7.1/en_core_web_sm-3.7.1-py3-none-any.whl",
# ]
# ///
"""Build the prompt-shape semantic layer.

Walks every session via the OpenStory REST API, parses every (non-sidechain)
user_message with spaCy, and upserts the resulting grammatical shape into
`data/prompt_shapes.db`.

Idempotent on `event_id` — re-running only parses new prompts.

Usage:
    uv run scripts/build_prompt_shapes.py
    uv run scripts/build_prompt_shapes.py --db data/prompt_shapes.db
    uv run scripts/build_prompt_shapes.py --limit 50      # cap session count
    uv run scripts/build_prompt_shapes.py --rebuild       # drop table first

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
from dataclasses import dataclass
from pathlib import Path

import spacy


# Claude Code injects synthetic "user_message" records that are actually
# harness machinery: system reminders, task notifications, local command I/O.
# Strip these before parsing so the semantic layer reflects user intent, not
# the wrapping shell.
SYNTHETIC_TAG_RE = re.compile(
    r"<(system-reminder|task-notification|task-id|tool-use-id|local-command-[a-z]+|"
    r"command-(?:name|message|args|stdout|stderr)|summary)>.*?</\1>",
    re.DOTALL | re.IGNORECASE,
)
# Standalone <command-name>... blocks without a close tag also appear.
LOOSE_TAG_RE = re.compile(
    r"<(system-reminder|task-notification|task-id|tool-use-id|local-command-[a-z]+|"
    r"command-(?:name|message|args|stdout|stderr)|summary)>",
    re.IGNORECASE,
)


def clean_prompt(text: str) -> str:
    """Strip synthetic harness tags so we parse only the user's actual words."""
    cleaned = SYNTHETIC_TAG_RE.sub("", text)
    # if any unmatched opening tag survives, drop the whole prompt as synthetic
    if LOOSE_TAG_RE.search(cleaned):
        return ""
    return cleaned.strip()

API = "http://localhost:3002/api"
DEFAULT_DB = "data/prompt_shapes.db"
PROMPT_CHAR_CAP = 4000     # cap input length to spaCy
EXCERPT_LEN = 200          # how much of the prompt to store for grep-ability


SCHEMA = """
CREATE TABLE IF NOT EXISTS prompt_shapes (
    event_id        TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    timestamp       TEXT NOT NULL,
    seq             INTEGER NOT NULL,
    char_count      INTEGER NOT NULL,
    root_verbs      TEXT NOT NULL,
    subjects        TEXT NOT NULL,
    direct_objects  TEXT NOT NULL,
    noun_chunks     TEXT NOT NULL,
    adjectives      TEXT NOT NULL,
    adverbs         TEXT NOT NULL,
    prompt_excerpt  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_session   ON prompt_shapes(session_id);
CREATE INDEX IF NOT EXISTS idx_timestamp ON prompt_shapes(timestamp);
"""


@dataclass
class PromptRow:
    event_id: str
    session_id: str
    timestamp: str
    seq: int
    prompt: str


def fetch_json(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def iter_user_prompts(session_id: str):
    try:
        data = fetch_json(f"/sessions/{session_id}/records")
    except Exception as e:
        print(f"  warn: failed to fetch session {session_id}: {e}", file=sys.stderr)
        return
    records = data if isinstance(data, list) else data.get("records", [])
    for rec in records:
        if rec.get("record_type") != "user_message":
            continue
        if rec.get("is_sidechain"):
            continue
        payload = rec.get("payload") or {}
        content = payload.get("content", "")
        if not isinstance(content, str) or not content.strip():
            continue
        cleaned = clean_prompt(content)
        if not cleaned:
            continue
        yield PromptRow(
            event_id=rec.get("id", ""),
            session_id=session_id,
            timestamp=rec.get("timestamp", ""),
            seq=int(rec.get("seq") or 0),
            prompt=cleaned,
        )


def parse_shape(doc) -> dict[str, list[str]]:
    out: dict[str, list[str]] = {
        "root_verbs": [],
        "subjects": [],
        "direct_objects": [],
        "noun_chunks": [],
        "adjectives": [],
        "adverbs": [],
    }
    for sent in doc.sents:
        for tok in sent:
            if tok.dep_ == "ROOT" and tok.pos_ in {"VERB", "AUX"}:
                out["root_verbs"].append(tok.lemma_.lower())
                for c in tok.children:
                    if c.dep_ in {"nsubj", "nsubjpass"}:
                        out["subjects"].append(c.lemma_.lower())
                    elif c.dep_ in {"dobj", "obj"}:
                        out["direct_objects"].append(c.lemma_.lower())
    for chunk in doc.noun_chunks:
        text = chunk.lemma_.lower().strip()
        if len(text) > 1 and chunk.root.pos_ != "PRON":
            out["noun_chunks"].append(text)
    for tok in doc:
        if not tok.lemma_.isalpha() or tok.is_stop:
            continue
        if tok.pos_ == "ADJ":
            out["adjectives"].append(tok.lemma_.lower())
        elif tok.pos_ == "ADV":
            out["adverbs"].append(tok.lemma_.lower())
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--db", default=DEFAULT_DB, help=f"output db path (default {DEFAULT_DB})")
    ap.add_argument("--limit", type=int, default=0, help="cap number of sessions (0 = all)")
    ap.add_argument("--rebuild", action="store_true", help="drop the prompt_shapes table first")
    ap.add_argument("--batch-size", type=int, default=64, help="spaCy pipe batch size")
    args = ap.parse_args()

    db_path = Path(args.db)
    db_path.parent.mkdir(parents=True, exist_ok=True)

    conn = sqlite3.connect(db_path)
    if args.rebuild:
        conn.executescript("DROP TABLE IF EXISTS prompt_shapes;")
    conn.executescript(SCHEMA)
    conn.commit()

    existing = {row[0] for row in conn.execute("SELECT event_id FROM prompt_shapes")}
    print(f"existing parsed prompts: {len(existing)}", file=sys.stderr)

    print("loading spaCy en_core_web_sm...", file=sys.stderr)
    nlp = spacy.load("en_core_web_sm", disable=["ner"])

    print("fetching sessions...", file=sys.stderr)
    sessions_data = fetch_json("/sessions")
    sessions = sessions_data if isinstance(sessions_data, list) else sessions_data.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    if args.limit:
        sessions = sessions[: args.limit]

    rows_to_parse: list[PromptRow] = []
    print(f"scanning {len(sessions)} sessions for user prompts...", file=sys.stderr)
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, queued {len(rows_to_parse)} new prompts", file=sys.stderr)
        for row in iter_user_prompts(sid):
            if row.event_id in existing or not row.event_id:
                continue
            rows_to_parse.append(row)

    print(f"prompts to parse: {len(rows_to_parse)}", file=sys.stderr)
    if not rows_to_parse:
        print("nothing new to parse — exiting", file=sys.stderr)
        return 0

    texts = (r.prompt[:PROMPT_CHAR_CAP] for r in rows_to_parse)
    t0 = time.monotonic()
    inserted = 0
    batch: list[tuple] = []
    for row, doc in zip(rows_to_parse, nlp.pipe(texts, batch_size=args.batch_size)):
        shape = parse_shape(doc)
        batch.append((
            row.event_id,
            row.session_id,
            row.timestamp,
            row.seq,
            len(row.prompt),
            json.dumps(shape["root_verbs"]),
            json.dumps(shape["subjects"]),
            json.dumps(shape["direct_objects"]),
            json.dumps(shape["noun_chunks"]),
            json.dumps(shape["adjectives"]),
            json.dumps(shape["adverbs"]),
            row.prompt[:EXCERPT_LEN],
        ))
        if len(batch) >= 200:
            conn.executemany(
                """INSERT OR IGNORE INTO prompt_shapes
                   (event_id, session_id, timestamp, seq, char_count,
                    root_verbs, subjects, direct_objects, noun_chunks,
                    adjectives, adverbs, prompt_excerpt)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                batch,
            )
            conn.commit()
            inserted += len(batch)
            batch.clear()
    if batch:
        conn.executemany(
            """INSERT OR IGNORE INTO prompt_shapes
               (event_id, session_id, timestamp, seq, char_count,
                root_verbs, subjects, direct_objects, noun_chunks,
                adjectives, adverbs, prompt_excerpt)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            batch,
        )
        conn.commit()
        inserted += len(batch)

    elapsed = time.monotonic() - t0
    print(f"inserted {inserted} prompt shapes in {elapsed:.1f}s "
          f"({inserted / max(elapsed, 0.001):.0f}/s)", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
