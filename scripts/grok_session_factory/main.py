#!/usr/bin/env python3
"""Grok Session Factory — generate / loop / cover synthetic Grok Build sessions.

Usage:
  python3 scripts/grok_session_factory/main.py generate
  python3 scripts/grok_session_factory/main.py loop --target 0.95
  python3 scripts/grok_session_factory/main.py cover
  python3 scripts/grok_session_factory/main.py --test
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
if str(HERE) not in sys.path:
    sys.path.insert(0, str(HERE))

from catalog.actions import CATALOG, STORIES, ids as catalog_ids  # noqa: E402
from compose import (  # noqa: E402
    catalog_act_ids,
    compose_session,
    default_corpus_stories,
    pick_stories_for_gaps,
)

DEFAULT_OUT = HERE / "out"


def measure_coverage(acts_seen: set[str]) -> dict:
    all_ids = catalog_act_ids()
    hit = acts_seen & all_ids
    miss = all_ids - acts_seen
    ratio = len(hit) / len(all_ids) if all_ids else 1.0
    by_kind: dict[str, dict] = {}
    for a in CATALOG:
        k = a.kind
        by_kind.setdefault(k, {"total": 0, "hit": 0})
        by_kind[k]["total"] += 1
        if a.id in hit:
            by_kind[k]["hit"] += 1
    return {
        "catalog_size": len(all_ids),
        "hit": sorted(hit),
        "miss": sorted(miss),
        "coverage": round(ratio, 4),
        "by_kind": {
            k: {
                "hit": v["hit"],
                "total": v["total"],
                "ratio": round(v["hit"] / v["total"], 3) if v["total"] else 0,
            }
            for k, v in sorted(by_kind.items())
        },
    }


def scan_out(out_dir: Path) -> set[str]:
    """Union of acts_used from all MANIFEST.json under out_dir."""
    seen: set[str] = set()
    for m in out_dir.rglob("MANIFEST.json"):
        data = json.loads(m.read_text())
        seen.update(data.get("acts_used") or [])
    return seen


def cmd_generate(args: argparse.Namespace) -> int:
    out = Path(args.out)
    stories = default_corpus_stories() if args.full else pick_stories_for_gaps(catalog_act_ids(), n=args.turns)
    if args.full:
        stories = default_corpus_stories()
    em = compose_session(stories, cwd=args.cwd)
    session_dir = em.write_tree(out)
    cov = measure_coverage(set(em.acts_used))
    report = {
        "session_dir": str(session_dir),
        "stories": [s.id for s in stories],
        "coverage": cov,
    }
    (out / "last_report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    return 0


def cmd_loop(args: argparse.Namespace) -> int:
    """Generate sessions biased toward missing acts until coverage target."""
    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    history = []
    for i in range(args.max_sessions):
        seen = scan_out(out)
        cov = measure_coverage(seen)
        history.append({"i": i, "coverage": cov["coverage"], "miss": cov["miss"]})
        if cov["coverage"] >= args.target and i > 0:
            break
        missing = set(cov["miss"]) if cov["miss"] else catalog_act_ids()
        stories = pick_stories_for_gaps(missing, n=args.turns)
        # First iteration: full corpus for a big jump
        if i == 0 and args.bootstrap_full:
            stories = default_corpus_stories()
        em = compose_session(stories, cwd=args.cwd, t0=1_700_000_000 + i * 10_000)
        em.write_tree(out)

    seen = scan_out(out)
    final = measure_coverage(seen)
    report = {
        "sessions": len(list((out / "sessions").rglob("updates.jsonl"))) if (out / "sessions").exists() else 0,
        "iterations": len(history),
        "target": args.target,
        "final_coverage": final,
        "history": history,
    }
    (out / "loop_report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))
    ok = final["coverage"] >= args.target
    return 0 if ok else 1


def cmd_cover(args: argparse.Namespace) -> int:
    out = Path(args.out)
    seen = scan_out(out)
    report = measure_coverage(seen)
    report["sessions_scanned"] = len(list(out.rglob("MANIFEST.json")))
    print(json.dumps(report, indent=2))
    return 0


def cmd_test() -> int:
    """Lightweight self-test — no OpenStory server required."""
    failures = []
    # 1. catalog ids unique
    ids = catalog_ids()
    if len(ids) != len(set(ids)):
        failures.append("duplicate catalog ids")
    # 2. every story act exists (except speech handled specially)
    known = set(ids)
    for s in STORIES:
        for a in s.act_ids + s.recovery_act_ids:
            if a not in known:
                failures.append(f"story {s.id} unknown act {a}")
    # 3. full compose writes valid JSONL with turn_completed
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        em = compose_session(default_corpus_stories()[:3], cwd="/workspace/demo")
        d = em.write_tree(Path(td))
        updates = (d / "updates.jsonl").read_text().strip().splitlines()
        if len(updates) < 5:
            failures.append("too few updates")
        kinds = []
        for line in updates:
            row = json.loads(line)
            kinds.append(row["params"]["update"]["sessionUpdate"])
        if "user_message_chunk" not in kinds:
            failures.append("missing user_message_chunk")
        if "turn_completed" not in kinds:
            failures.append("missing turn_completed")
        if "tool_call" not in kinds:
            failures.append("missing tool_call in multi-story compose")
        events = (d / "events.jsonl").read_text().strip().splitlines()
        if not any("loop_started" in e for e in events):
            failures.append("events.jsonl missing loop_started")
        cov = measure_coverage(set(em.acts_used))
        if cov["coverage"] <= 0:
            failures.append("zero coverage after compose")

    # 4. loop improves or hits target on empty out
    with tempfile.TemporaryDirectory() as td:
        class A:
            out = td
            target = 0.85
            max_sessions = 10
            turns = 4
            cwd = "/workspace/demo"
            bootstrap_full = True

        rc = cmd_loop(A())  # type: ignore[arg-type]
        report = json.loads((Path(td) / "loop_report.json").read_text())
        if report["final_coverage"]["coverage"] < 0.85:
            failures.append(f"loop coverage low: {report['final_coverage']['coverage']}")
        if rc != 0 and report["final_coverage"]["coverage"] >= 0.85:
            failures.append("loop rc nonzero despite meeting target")

    if failures:
        print("FAIL:", failures)
        return 1
    print(
        json.dumps(
            {
                "ok": True,
                "catalog_size": len(ids),
                "stories": len(STORIES),
            },
            indent=2,
        )
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description="Grok Session Factory")
    p.add_argument("--test", action="store_true", help="run self-test")
    sub = p.add_subparsers(dest="cmd")

    g = sub.add_parser("generate", help="emit one session")
    g.add_argument("--out", default=str(DEFAULT_OUT))
    g.add_argument("--cwd", default="/workspace/demo")
    g.add_argument("--full", action="store_true", help="all stories in one session")
    g.add_argument("--turns", type=int, default=4, help="stories if not --full")

    lp = sub.add_parser("loop", help="emit until coverage target")
    lp.add_argument("--out", default=str(DEFAULT_OUT))
    lp.add_argument("--cwd", default="/workspace/demo")
    lp.add_argument("--target", type=float, default=0.9)
    lp.add_argument("--max-sessions", type=int, default=20)
    lp.add_argument("--turns", type=int, default=4)
    lp.add_argument("--bootstrap-full", action="store_true", default=True)
    lp.add_argument("--no-bootstrap-full", action="store_false", dest="bootstrap_full")

    c = sub.add_parser("cover", help="report coverage of out/")
    c.add_argument("--out", default=str(DEFAULT_OUT))

    args = p.parse_args(argv)
    if args.test or args.cmd is None and getattr(args, "test", False):
        if args.test:
            return cmd_test()
    if args.cmd == "generate":
        return cmd_generate(args)
    if args.cmd == "loop":
        return cmd_loop(args)
    if args.cmd == "cover":
        return cmd_cover(args)
    if args.test:
        return cmd_test()
    p.print_help()
    return 2


if __name__ == "__main__":
    raise SystemExit(main())
