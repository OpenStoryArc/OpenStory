#!/usr/bin/env python3
"""Find duplicate events in the store — same session content stored under
different CloudEvent ids.

Background: every server restart's JSONL backfill re-translates recent
transcript lines and re-publishes them to NATS. Events whose translation
mints a fresh UUID (instead of deriving a deterministic id from the source
line) escape both dedup layers and get stored again. Observed on session
0375729d: 16,712 stored events -> 17,483 -> 18,254 across two restarts
(~771 duplicates per boot).

This script groups a session's stored events by their content identity
(seq, subtype, time) and reports groups stored under >1 distinct id —
including which subtypes duplicate, so the fix can target the exact
translation paths that lack stable ids.

Usage:
    python3 scripts/find_duplicate_events.py SESSION_ID [--data-dir data]
    python3 scripts/find_duplicate_events.py --test
"""

import argparse
import json
import sqlite3
import sys
from collections import Counter, defaultdict
from pathlib import Path


def duplicate_groups(events: list[dict]) -> dict[tuple, list[str]]:
    """Group events by content identity -> list of distinct stored ids.

    Content identity = (seq, subtype, time): what a re-translation of the
    same transcript line reproduces. Returns only groups with >1 id.
    """
    groups: dict[tuple, list[str]] = defaultdict(list)
    for e in events:
        data = e.get("data") or {}
        key = (data.get("seq"), e.get("subtype"), e.get("time"))
        eid = e.get("id")
        if eid not in groups[key]:
            groups[key].append(eid)
    return {k: ids for k, ids in groups.items() if len(ids) > 1}


def report(session_id: str, data_dir: Path) -> int:
    db = data_dir / "open-story.db"
    if not db.exists():
        print(f"no database at {db}", file=sys.stderr)
        return 2
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    rows = con.execute(
        "SELECT payload FROM events WHERE session_id = ?", (session_id,)
    ).fetchall()
    con.close()
    events = [json.loads(r[0]) for r in rows]

    dupes = duplicate_groups(events)
    n_extra = sum(len(ids) - 1 for ids in dupes.values())
    print(f"session {session_id[:8]}: {len(events)} stored events")
    print(f"duplicate groups: {len(dupes)} (≈{n_extra} redundant rows)")

    by_subtype = Counter(k[1] for k in dupes)
    for subtype, count in by_subtype.most_common():
        print(f"  {subtype or '(none)':40s} {count}")

    if dupes:
        key, ids = next(iter(sorted(dupes.items(), key=lambda kv: str(kv[0]))))
        print(f"\nexample group (seq={key[0]}, subtype={key[1]}, time={key[2]}):")
        for i in ids[:4]:
            print(f"  id={i}")
    return 0 if not dupes else 1


def _test() -> None:
    ev = lambda eid, seq, sub, t: {  # noqa: E731
        "id": eid,
        "subtype": sub,
        "time": t,
        "data": {"seq": seq},
    }
    # same content, two ids -> one duplicate group
    dup = duplicate_groups(
        [
            ev("a", 1, "message.user.prompt", "t1"),
            ev("b", 1, "message.user.prompt", "t1"),
            ev("c", 2, "message.assistant.text", "t2"),
        ]
    )
    assert list(dup.keys()) == [(1, "message.user.prompt", "t1")], dup
    assert dup[(1, "message.user.prompt", "t1")] == ["a", "b"]
    # same id twice (store-level dupe read) is NOT a duplicate group
    assert duplicate_groups([ev("a", 1, "s", "t"), ev("a", 1, "s", "t")]) == {}
    print("ok")


if __name__ == "__main__":
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("session_id", nargs="?")
    p.add_argument("--data-dir", default="data", type=Path)
    p.add_argument("--test", action="store_true")
    args = p.parse_args()
    if args.test:
        _test()
    elif args.session_id:
        sys.exit(report(args.session_id, args.data_dir))
    else:
        p.print_help()