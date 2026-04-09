#!/usr/bin/env python3
"""
Investigate sentence pattern duplication for an OpenStory session.

Phase 0 of the "deterministic pattern _id" fix. This script answers the
empirical question: when the same logical turn appears multiple times
in the patterns collection, what's actually different between the rows?

Specifically, it tests three competing hypotheses for why
`turn.sentence` patterns get persisted multiple times across server
reprocessing runs:

  H1. The same source events are re-detected on each boot, and the
      patterns consumer stamps a fresh wall-clock onto `started_at`
      at detection time (started_at = "when did I notice this").
      → Same event_ids, different started_at, same underlying source.

  H2. The watcher / translator restamps event.time on synthetic events
      (session_start, hooks) at translation time, and turn boundaries
      that begin with such an event inherit a fresh start_ts.
      → Same logical turn, different event_ids depending on which
      synthetic boundary event got included.

  H3. The patterns consumer is being fed the same events more than once
      within a single session because of NATS replay or watcher rescan,
      and emitting one sentence row per pass.
      → Same event_ids, started_at values clustered into batches.

Usage:
    python3 scripts/inspect_sentence_dedup.py SESSION_ID
    python3 scripts/inspect_sentence_dedup.py SESSION_ID --json
    python3 scripts/inspect_sentence_dedup.py --list      # show recent sessions
    python3 scripts/inspect_sentence_dedup.py --test      # self-tests
"""

import argparse
import json
import sys
import urllib.request
from collections import defaultdict
from typing import Any

API_BASE = "http://localhost:3002/api"


def fetch_json(path: str) -> Any:
    """GET an OpenStory API endpoint, return parsed JSON."""
    url = f"{API_BASE}{path}"
    with urllib.request.urlopen(url, timeout=10) as resp:
        return json.loads(resp.read())


def list_sessions(limit: int = 15) -> list[dict]:
    """Recent sessions from the live server."""
    data = fetch_json("/sessions")
    return data[:limit]


def fetch_patterns(session_id: str) -> list[dict]:
    """All patterns for a session, normalized to a flat list."""
    data = fetch_json(f"/sessions/{session_id}/patterns")
    if isinstance(data, dict) and "patterns" in data:
        return data["patterns"]
    return data


def fetch_records(session_id: str) -> list[dict]:
    """All ViewRecords for a session — used to look up actual event timestamps."""
    data = fetch_json(f"/sessions/{session_id}/records")
    if isinstance(data, dict) and "records" in data:
        return data["records"]
    return data


def group_sentences_by_structural_identity(
    sentences: list[dict],
) -> dict[tuple[int, int], list[dict]]:
    """
    Group sentence patterns by (turn_number, scope_depth) — the structural
    identity that *should* uniquely identify a logical sentence regardless of
    when it was detected.
    """
    groups: dict[tuple[int, int], list[dict]] = defaultdict(list)
    for s in sentences:
        md = s.get("metadata") or {}
        key = (md.get("turn", -1), md.get("scope_depth", -1))
        groups[key].append(s)
    return groups


def event_ids_set(sentence: dict) -> frozenset[str]:
    """The set of event_ids referenced by a sentence pattern."""
    return frozenset(sentence.get("event_ids") or [])


def classify_duplication_pattern(group: list[dict]) -> str:
    """
    For a group of sentences with the same (turn, scope_depth), classify
    what kind of duplication is happening.

    Returns one of:
      'unique'              — only one row, no duplication
      'identical-event-ids' — multiple rows, all share the same event_ids
                              → H1 (wall-clock at detection time)
      'overlapping'         — multiple rows, event_ids overlap but not identical
                              → H2 (synthetic boundary events differ across runs)
      'disjoint'            — multiple rows, no shared event_ids
                              → not duplication, different content under same key
    """
    if len(group) <= 1:
        return "unique"

    sets = [event_ids_set(s) for s in group]
    first = sets[0]
    if all(s == first for s in sets[1:]):
        return "identical-event-ids"

    intersection = first
    for s in sets[1:]:
        intersection = intersection & s
    if intersection:
        return "overlapping"
    return "disjoint"


def stable_and_unstable_event_ids(group: list[dict]) -> tuple[set[str], set[str]]:
    """
    Split the union of event_ids in a duplicate group into:
      - stable:   ids present in EVERY row (the rock-solid backbone)
      - unstable: ids present in some rows but not others (the drift)
    """
    sets = [event_ids_set(s) for s in group]
    if not sets:
        return set(), set()
    union: set[str] = set()
    for s in sets:
        union |= s
    intersection = sets[0]
    for s in sets[1:]:
        intersection &= s
    return set(intersection), union - intersection


def classify_unstable_events_by_type(
    unstable_ids: set[str],
    record_type_by_id: dict[str, str],
) -> dict[str, int]:
    """
    For a set of 'unstable' event_ids (ones that appear in some duplicate
    rows but not others), count how many have each record_type. This tells
    us whether instability is concentrated in synthetic events
    (file_snapshot, system_event, hooks) or scattered across user/assistant
    messages.
    """
    counts: dict[str, int] = defaultdict(int)
    for eid in unstable_ids:
        rt = record_type_by_id.get(eid, "<not in records>")
        counts[rt] += 1
    return dict(counts)


def analyze(session_id: str) -> dict:
    """Run the full analysis for a session, return a structured report."""
    patterns = fetch_patterns(session_id)
    sentences = [p for p in patterns if p.get("pattern_type") == "turn.sentence"]

    # Build event_id → record_type lookup from the records endpoint, so we
    # can characterize WHICH events are unstable across duplicate runs.
    records = fetch_records(session_id)
    record_type_by_id: dict[str, str] = {
        r["id"]: r.get("record_type", "<unknown>") for r in records
    }

    by_identity = group_sentences_by_structural_identity(sentences)

    classes: dict[str, int] = defaultdict(int)
    # Aggregate instability across ALL duplicate groups: which record_types
    # most often drift in/out of duplicates?
    global_unstable_subtypes: dict[str, int] = defaultdict(int)
    duplicated_groups = []

    for key, group in sorted(by_identity.items()):
        cls = classify_duplication_pattern(group)
        classes[cls] += 1
        if cls == "unique":
            continue

        stable, unstable = stable_and_unstable_event_ids(group)
        unstable_by_type = classify_unstable_events_by_type(unstable, record_type_by_id)
        for rt, n in unstable_by_type.items():
            global_unstable_subtypes[rt] += n

        duplicated_groups.append({
            "turn": key[0],
            "scope_depth": key[1],
            "duplicate_count": len(group),
            "class": cls,
            "started_at_values": sorted(s.get("started_at") for s in group),
            "summaries": [s.get("summary", "")[:100] for s in group],
            "event_id_sets": [sorted(s.get("event_ids") or []) for s in group],
            "stable_count": len(stable),
            "unstable_count": len(unstable),
            "unstable_by_record_type": unstable_by_type,
        })

    return {
        "session_id": session_id,
        "total_patterns": len(patterns),
        "total_sentences": len(sentences),
        "distinct_logical_sentences": len(by_identity),
        "total_records": len(records),
        "classification": dict(classes),
        "global_unstable_record_types": dict(global_unstable_subtypes),
        "duplicated_groups": duplicated_groups,
    }


def hypothesis_verdict(report: dict) -> str:
    """Map the classification counts to a hypothesis verdict."""
    cls = report["classification"]
    has_identical = cls.get("identical-event-ids", 0) > 0
    has_overlapping = cls.get("overlapping", 0) > 0
    has_disjoint = cls.get("disjoint", 0) > 0

    if has_identical and not has_overlapping and not has_disjoint:
        return (
            "H1 confirmed: every duplicate group shares identical event_ids. "
            "The source events are the same — only `started_at` varies. "
            "Root cause is in the sentence detector / patterns consumer "
            "stamping fresh detection time onto a structurally-stable sentence."
        )
    if has_overlapping and not has_disjoint:
        return (
            "H2 likely: duplicate groups share *some* event_ids but not all. "
            "Suggests synthetic boundary events (session_start, hooks) are "
            "being re-translated with new IDs across runs, so the patterns "
            "consumer assembles structurally-identical turns from slightly "
            "different event sets each time."
        )
    if has_disjoint:
        return (
            "H3 or worse: duplicate groups have completely disjoint event_ids. "
            "Same (turn, scope_depth) is being applied to genuinely different "
            "content. This is not just an _id problem — there's something "
            "wrong with how turn numbers get assigned."
        )
    if not (has_identical or has_overlapping or has_disjoint):
        return "no duplication detected — patterns are clean"
    return "mixed signal — manual inspection needed"


def render_human(report: dict) -> str:
    """Format the analysis report as readable markdown."""
    lines = []
    lines.append(f"# Sentence Dedup Analysis: {report['session_id']}\n")
    lines.append(f"- **Total patterns**: {report['total_patterns']}")
    lines.append(f"- **Total `turn.sentence` rows**: {report['total_sentences']}")
    lines.append(f"- **Distinct logical sentences (by turn+depth)**: {report['distinct_logical_sentences']}")

    n_dup_rows = report["total_sentences"] - report["distinct_logical_sentences"]
    if report["distinct_logical_sentences"]:
        ratio = report["total_sentences"] / report["distinct_logical_sentences"]
        lines.append(f"- **Duplication ratio**: {ratio:.2f}× (expected 1.00×)")
    lines.append(f"- **Excess rows from duplication**: {n_dup_rows}")
    lines.append("")

    lines.append("## Classification of duplicate groups\n")
    for cls, count in sorted(report["classification"].items()):
        lines.append(f"- `{cls}`: {count}")
    lines.append("")

    lines.append("## Hypothesis verdict\n")
    lines.append(hypothesis_verdict(report))
    lines.append("")

    if report.get("global_unstable_record_types"):
        lines.append("## Global instability by record_type\n")
        lines.append("(Across ALL duplicate groups: which record_types are most often drifting in/out?)\n")
        sorted_types = sorted(
            report["global_unstable_record_types"].items(),
            key=lambda kv: -kv[1],
        )
        for rt, n in sorted_types:
            lines.append(f"- `{rt}`: {n} unstable references")
        lines.append("")

    if report["duplicated_groups"]:
        lines.append("## Duplicated groups (first 5)\n")
        for g in report["duplicated_groups"][:5]:
            lines.append(f"### turn={g['turn']}  scope_depth={g['scope_depth']}  ({g['class']})")
            lines.append(f"- {g['duplicate_count']} rows")
            lines.append("- started_at:")
            for ts in g["started_at_values"]:
                lines.append(f"    - {ts}")
            ids = g["event_id_sets"]
            if g["class"] == "identical-event-ids":
                lines.append(f"- event_ids: {len(ids[0])} ids, identical across all rows")
            elif g["class"] == "overlapping":
                lines.append(
                    f"- event_ids: {g['stable_count']} stable, "
                    f"{g['unstable_count']} unstable, "
                    f"{[len(s) for s in ids]} per-row counts"
                )
            else:
                lines.append(
                    f"- event_ids: 0 stable, {g['unstable_count']} unstable, "
                    f"{[len(s) for s in ids]} per-row counts"
                )
            if g.get("unstable_by_record_type"):
                rts = sorted(g["unstable_by_record_type"].items(), key=lambda kv: -kv[1])
                lines.append("- unstable record_types: " + ", ".join(f"{rt}={n}" for rt, n in rts))
            lines.append(f"- summary[0]: {g['summaries'][0]}")
            lines.append("")

    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Self-tests (--test)
# ---------------------------------------------------------------------------


def _make_sentence(turn: int, depth: int, started_at: str, event_ids: list[str], summary: str = "") -> dict:
    return {
        "pattern_type": "turn.sentence",
        "started_at": started_at,
        "event_ids": event_ids,
        "summary": summary,
        "metadata": {"turn": turn, "scope_depth": depth},
    }


def _test_unique_group():
    s = [_make_sentence(1, 0, "2026-04-08T00:00:00Z", ["a", "b"])]
    groups = group_sentences_by_structural_identity(s)
    assert len(groups) == 1
    assert classify_duplication_pattern(groups[(1, 0)]) == "unique"


def _test_identical_event_ids_h1():
    """H1: duplicates share identical event_ids → wall-clock-on-detect."""
    s = [
        _make_sentence(1, 0, "2026-04-08T00:00:00Z", ["a", "b", "c"]),
        _make_sentence(1, 0, "2026-04-08T05:00:00Z", ["a", "b", "c"]),
        _make_sentence(1, 0, "2026-04-08T10:00:00Z", ["a", "b", "c"]),
    ]
    groups = group_sentences_by_structural_identity(s)
    assert classify_duplication_pattern(groups[(1, 0)]) == "identical-event-ids"


def _test_overlapping_event_ids_h2():
    """H2: duplicates share some event_ids but not all → synthetic boundary drift."""
    s = [
        _make_sentence(1, 0, "2026-04-08T00:00:00Z", ["session_start_v1", "a", "b"]),
        _make_sentence(1, 0, "2026-04-08T05:00:00Z", ["session_start_v2", "a", "b"]),
    ]
    groups = group_sentences_by_structural_identity(s)
    assert classify_duplication_pattern(groups[(1, 0)]) == "overlapping"


def _test_disjoint_event_ids_h3():
    """H3: same key, no shared events → real content collision."""
    s = [
        _make_sentence(1, 0, "2026-04-08T00:00:00Z", ["a", "b"]),
        _make_sentence(1, 0, "2026-04-08T05:00:00Z", ["c", "d"]),
    ]
    groups = group_sentences_by_structural_identity(s)
    assert classify_duplication_pattern(groups[(1, 0)]) == "disjoint"


def _test_stable_unstable_split():
    s = [
        _make_sentence(1, 0, "t1", ["a", "b", "c", "d"]),
        _make_sentence(1, 0, "t2", ["a", "b", "c", "e"]),
        _make_sentence(1, 0, "t3", ["a", "b", "c", "f"]),
    ]
    stable, unstable = stable_and_unstable_event_ids(s)
    assert stable == {"a", "b", "c"}, f"unexpected stable: {stable}"
    assert unstable == {"d", "e", "f"}, f"unexpected unstable: {unstable}"


def _test_classify_unstable_by_type():
    record_lookup = {
        "a": "user_message",
        "b": "assistant_message",
        "d": "file_snapshot",
        "e": "system_event",
        "f": "file_snapshot",
    }
    counts = classify_unstable_events_by_type({"d", "e", "f"}, record_lookup)
    assert counts == {"file_snapshot": 2, "system_event": 1}, f"unexpected: {counts}"


def _test_classify_unstable_handles_missing():
    """Unstable ids that aren't in the records map (deleted? out of window?) get counted under a sentinel."""
    counts = classify_unstable_events_by_type({"ghost"}, {})
    assert counts == {"<not in records>": 1}


def _test_verdicts():
    assert "H1 confirmed" in hypothesis_verdict({
        "classification": {"unique": 5, "identical-event-ids": 3}
    })
    assert "H2 likely" in hypothesis_verdict({
        "classification": {"unique": 5, "overlapping": 2}
    })
    assert "H3 or worse" in hypothesis_verdict({
        "classification": {"unique": 5, "disjoint": 1}
    })
    assert "no duplication" in hypothesis_verdict({"classification": {"unique": 10}})


def run_tests():
    tests = [
        _test_unique_group,
        _test_identical_event_ids_h1,
        _test_overlapping_event_ids_h2,
        _test_disjoint_event_ids_h3,
        _test_stable_unstable_split,
        _test_classify_unstable_by_type,
        _test_classify_unstable_handles_missing,
        _test_verdicts,
    ]
    for t in tests:
        try:
            t()
            print(f"  ok    {t.__name__}")
        except AssertionError as e:
            print(f"  FAIL  {t.__name__}: {e}")
            sys.exit(1)
    print(f"\n{len(tests)}/{len(tests)} tests passed")


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main():
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("session_id", nargs="?", help="Session UUID to analyze")
    parser.add_argument("--json", action="store_true", help="Emit JSON instead of markdown")
    parser.add_argument("--list", action="store_true", help="List recent sessions and exit")
    parser.add_argument("--test", action="store_true", help="Run self-tests and exit")
    args = parser.parse_args()

    if args.test:
        run_tests()
        return

    if args.list:
        for s in list_sessions():
            sid = s["session_id"]
            label = s["label"][:60]
            n = s["event_count"]
            print(f"  {sid}  events={n:5}  {label}")
        return

    if not args.session_id:
        parser.error("session_id required (or use --list / --test)")

    report = analyze(args.session_id)

    if args.json:
        print(json.dumps(report, indent=2))
    else:
        print(render_human(report))


if __name__ == "__main__":
    main()
