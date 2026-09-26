#!/usr/bin/env python3
"""boot_assess — decide what a node's boot must read, without reading it.

Prototype for the boot planner (docs/research/openstory-as-node/
2026-09-26-boot-planner.md, rows BP-01..BP-04). Read-only against the node:
it opens the store with `mode=ro`, only stats and tail-reads JSONL, and writes
its manifest to a path you choose (never the node's data dir by default).

Three passes, each timed:

  cold    no manifest yet: every file is New. Counts lines and hashes each
          file's tail, which is what today's reconcile pays on every boot.
          Writes the manifest.
  warm    with a manifest: stat every file; a file with the same size and
          mtime is Unchanged without reading a byte; a grown file whose old
          tail still hashes the same is Appended (read from the old size); a
          shrunk or re-tailed file is Rewritten; a file the manifest knows
          but the disk lacks is Missing (a finding, never a delete).
  verify  optional, the background check: extract every event id from the
          JSONL (fixed position at the start of each line) and compare with
          the store's ids per session.

Then a pure plan: given the classes, the store's session summary, a window
and a policy, which work runs before serving and which becomes debt.

Usage:
    python3 scripts/boot_assess.py --data-dir ~/projects/OpenStory/data \\
        --manifest /tmp/manifest.jsonl --window-days 1 [--verify] [--json out.json]
    python3 scripts/boot_assess.py --test
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import sqlite3
import sys
import time
from dataclasses import dataclass, asdict
from pathlib import Path

TAIL = 4096
ID_AT_START = re.compile(rb'^\{"specversion":"[^"]*","id":"([^"]+)"')


@dataclass(frozen=True)
class Stat:
    session_id: str
    size: int
    mtime_ns: int


@dataclass(frozen=True)
class Entry:
    session_id: str
    size: int
    mtime_ns: int
    lines: int
    tail_hash: str  # hash of the last TAIL bytes ending at `size`


# ── pure core ────────────────────────────────────────────────────────────────

FRACTION = re.compile(r"\.(\d+)")


def parse_ts(s: str) -> dt.datetime:
    """RFC 3339 with any fractional precision (the store writes nanoseconds)."""
    s = FRACTION.sub(lambda m: "." + m.group(1)[:6].ljust(6, "0"), s.replace("Z", "+00:00"), count=1)
    return dt.datetime.fromisoformat(s)


def classify(stat: Stat | None, entry: Entry | None, tail_at_old_size: str | None) -> tuple[str, int]:
    """(class, offset to read from). `tail_at_old_size` is the hash of the
    TAIL bytes ending at entry.size in the current file, supplied only when the
    caller had to look (size or mtime moved)."""
    if stat is None and entry is None:
        raise ValueError("nothing to classify")
    if stat is None:
        return "missing", 0
    if entry is None:
        return "new", 0
    if stat.size == entry.size and stat.mtime_ns == entry.mtime_ns:
        return "unchanged", stat.size
    if stat.size < entry.size:
        return "rewritten", 0
    if tail_at_old_size == entry.tail_hash:
        return ("unchanged", stat.size) if stat.size == entry.size else ("appended", entry.size)
    return "rewritten", 0


def plan(classes: dict[str, tuple[str, int]], sizes: dict[str, int], store: dict[str, tuple[int, str]],
         now: dt.datetime, window_days: float, policy: str) -> dict:
    """Pure. classes: session -> (class, offset); sizes: session -> bytes on disk;
    store: session -> (event_count, last_event iso). Returns the plan."""
    cutoff = now - dt.timedelta(days=window_days)

    def in_window(sid: str) -> bool:
        last = store.get(sid, (0, ""))[1]
        if not last:
            return True  # unknown to the store: treat as recent, read it now
        return parse_ts(last) >= cutoff

    before, debt, findings = [], [], []
    for sid in sorted(classes):
        cls, off = classes[sid]
        if cls == "unchanged":
            continue
        if cls == "missing":
            findings.append(("transcript_missing", sid))
            continue
        step = ("reconcile", sid, off, sizes.get(sid, 0) - off)
        (before if policy == "thorough" or in_window(sid) else debt).append(step)
    window_sessions = sorted(s for s in store if in_window(s))
    for sid in window_sessions:
        before.append(("replay", sid, 0, store[sid][0]))
    for sid in sorted(set(store) - set(window_sessions)):
        (before if policy == "thorough" else debt).append(("replay", sid, 0, store[sid][0]))
    store_only = sorted(set(store) - set(classes))
    return {
        "before_serving": before,
        "debt": debt,
        "findings": findings,
        "store_only_sessions": len(store_only),
        "bytes_before_serving": sum(s[3] for s in before if s[0] == "reconcile"),
        "events_before_serving": sum(s[3] for s in before if s[0] == "replay"),
        "bytes_debt": sum(s[3] for s in debt if s[0] == "reconcile"),
        "events_debt": sum(s[3] for s in debt if s[0] == "replay"),
    }


# ── edges: disk and store ────────────────────────────────────────────────────

def stat_dir(data_dir: Path) -> dict[str, Stat]:
    out = {}
    with os.scandir(data_dir) as it:
        for e in it:
            if e.name.endswith(".jsonl") and e.is_file() and e.name not in ("events.jsonl", "presence.jsonl"):
                st = e.stat()
                sid = e.name[:-6]
                out[sid] = Stat(sid, st.st_size, st.st_mtime_ns)
    return out


def tail_hash(path: Path, end: int) -> str:
    start = max(0, end - TAIL)
    with open(path, "rb") as f:
        f.seek(start)
        return hashlib.blake2b(f.read(end - start), digest_size=16).hexdigest()


def count_lines(path: Path) -> int:
    n = 0
    with open(path, "rb") as f:
        while chunk := f.read(1 << 20):
            n += chunk.count(b"\n")
    return n


def store_summary(db: Path) -> dict[str, tuple[int, str]]:
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        rows = con.execute("SELECT id, event_count, COALESCE(last_event,'') FROM sessions").fetchall()
    finally:
        con.close()
    return {r[0]: (r[1] or 0, r[2]) for r in rows}


def store_ids(db: Path) -> dict[str, set[str]]:
    con = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    out: dict[str, set[str]] = {}
    try:
        for sid, eid in con.execute("SELECT session_id, id FROM events"):
            out.setdefault(sid, set()).add(eid)
    finally:
        con.close()
    return out


def file_ids(path: Path) -> set[str]:
    ids = set()
    with open(path, "rb") as f:
        for line in f:
            m = ID_AT_START.match(line)
            if m:
                ids.add(m.group(1).decode())
    return ids


def load_manifest(p: Path) -> dict[str, Entry]:
    if not p.exists():
        return {}
    out = {}
    for line in p.read_text().splitlines():
        d = json.loads(line)
        out[d["session_id"]] = Entry(**d)
    return out


def write_manifest(p: Path, entries: dict[str, Entry]) -> None:
    p.parent.mkdir(parents=True, exist_ok=True)
    with open(p, "w") as f:
        for sid in sorted(entries):
            f.write(json.dumps(asdict(entries[sid])) + "\n")


# ── passes ───────────────────────────────────────────────────────────────────

def cold(data_dir: Path, stats: dict[str, Stat]) -> dict[str, Entry]:
    return {sid: Entry(sid, s.size, s.mtime_ns, count_lines(data_dir / f"{sid}.jsonl"),
                       tail_hash(data_dir / f"{sid}.jsonl", s.size)) for sid, s in stats.items()}


def warm(data_dir: Path, stats: dict[str, Stat], manifest: dict[str, Entry]) -> dict[str, tuple[str, int]]:
    out = {}
    for sid in set(stats) | set(manifest):
        s, e = stats.get(sid), manifest.get(sid)
        look = None
        if s and e and (s.size != e.size or s.mtime_ns != e.mtime_ns) and s.size >= e.size:
            look = tail_hash(data_dir / f"{sid}.jsonl", e.size)
        out[sid] = classify(s, e, look)
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--data-dir", type=Path)
    ap.add_argument("--manifest", type=Path, help="where to read/write the manifest (not the node's data dir)")
    ap.add_argument("--window-days", type=float, default=1.0)
    ap.add_argument("--policy", default="fast", choices=["fast", "standard", "thorough"])
    ap.add_argument("--verify", action="store_true", help="also compare every JSONL id with the store")
    ap.add_argument("--json", type=Path)
    ap.add_argument("--test", action="store_true")
    a = ap.parse_args()
    if a.test:
        return _test()
    if not a.data_dir or not a.manifest:
        ap.error("--data-dir and --manifest are required")
    if a.manifest.resolve().parent == a.data_dir.resolve():
        ap.error("refusing to write the manifest into the node's data dir")
    report: dict = {"timings_s": {}}
    t = time.perf_counter(); stats = stat_dir(a.data_dir); report["timings_s"]["stat"] = time.perf_counter() - t
    t = time.perf_counter(); store = store_summary(a.data_dir / "open-story.db"); report["timings_s"]["store_summary"] = time.perf_counter() - t
    manifest = load_manifest(a.manifest)
    if not manifest:
        t = time.perf_counter(); manifest = cold(a.data_dir, stats); report["timings_s"]["cold_manifest_build"] = time.perf_counter() - t
        write_manifest(a.manifest, manifest)
        classes = {sid: ("new", 0) for sid in stats}
    else:
        t = time.perf_counter(); classes = warm(a.data_dir, stats, manifest); report["timings_s"]["warm_classify"] = time.perf_counter() - t
    sizes = {sid: s.size for sid, s in stats.items()}
    p = plan(classes, sizes, store, dt.datetime.now(dt.timezone.utc), a.window_days, a.policy)
    counts: dict[str, int] = {}
    for c, _ in classes.values():
        counts[c] = counts.get(c, 0) + 1
    report.update(files=len(stats), bytes_on_disk=sum(sizes.values()), store_sessions=len(store), classes=counts,
                  window_days=a.window_days, policy=a.policy,
                  **{k: v for k, v in p.items() if k not in ("before_serving", "debt")},
                  steps_before_serving=len(p["before_serving"]), steps_debt=len(p["debt"]))
    if a.verify:
        t = time.perf_counter(); sids = store_ids(a.data_dir / "open-story.db"); t_db = time.perf_counter() - t
        t = time.perf_counter()
        missing_in_store, sessions_short = 0, 0
        for sid in stats:
            fids = file_ids(a.data_dir / f"{sid}.jsonl")
            gap = len(fids - sids.get(sid, set()))
            missing_in_store += gap
            sessions_short += gap > 0
        report["timings_s"]["verify_store_ids"] = t_db
        report["timings_s"]["verify_file_ids"] = time.perf_counter() - t
        report["verify"] = {"events_in_jsonl_not_in_store": missing_in_store, "sessions_short": sessions_short}
    for k, v in report["timings_s"].items():
        report["timings_s"][k] = round(v, 3)
    print(json.dumps(report, indent=2))
    if a.json:
        a.json.write_text(json.dumps(report, indent=2))
    return 0


def _test() -> int:
    s = lambda size, m=1: Stat("s", size, m)
    e = Entry("s", 100, 1, 10, "h")
    assert classify(s(100), e, None) == ("unchanged", 100)
    assert classify(s(150, 2), e, "h") == ("appended", 100)
    assert classify(s(150, 2), e, "x") == ("rewritten", 0)
    assert classify(s(80, 2), e, None) == ("rewritten", 0)
    assert classify(s(100, 2), e, "h") == ("unchanged", 100)   # touched, same bytes
    assert classify(s(100, 2), e, "x") == ("rewritten", 0)
    assert classify(None, e, None) == ("missing", 0)
    assert classify(s(10), None, None) == ("new", 0)
    now = dt.datetime(2026, 9, 26, tzinfo=dt.timezone.utc)
    store = {"a": (5, "2026-09-25T12:00:00Z"), "b": (7, "2026-09-01T00:00:00Z"), "c": (3, "2026-09-25T23:00:00Z")}
    classes = {"a": ("appended", 40), "b": ("new", 0), "c": ("unchanged", 90), "d": ("missing", 0)}
    sizes = {"a": 100, "b": 50, "c": 90}
    p = plan(classes, sizes, store, now, 1, "fast")
    assert ("reconcile", "a", 40, 60) in p["before_serving"], p
    assert ("reconcile", "b", 0, 50) in p["debt"], p
    assert ("transcript_missing", "d") in p["findings"]
    assert p["events_before_serving"] == 8 and p["events_debt"] == 7, p
    t = plan(classes, sizes, store, now, 1, "thorough")
    assert t["debt"] == [] and t["events_before_serving"] == 15, t
    # completeness: every non-unchanged, non-missing file and every store session appears exactly once
    for q in (p, t):
        steps = q["before_serving"] + q["debt"]
        assert sorted(x[1] for x in steps if x[0] == "reconcile") == ["a", "b"]
        assert sorted(x[1] for x in steps if x[0] == "replay") == ["a", "b", "c"]
    assert plan(classes, sizes, store, now, 1, "fast") == p   # determinism
    assert parse_ts("2026-06-24T23:26:55.236775402+00:00") == dt.datetime(2026, 6, 24, 23, 26, 55, 236775, tzinfo=dt.timezone.utc)
    assert parse_ts("2026-06-24T23:26:55Z").second == 55
    print("ok: 18 checks")
    return 0


if __name__ == "__main__":
    sys.exit(main())
