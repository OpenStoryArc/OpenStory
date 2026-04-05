#!/usr/bin/env python3
"""Data quality tests for StructuralTurns against the live API.

Validates invariants across ALL sessions, not just fixtures.
Run against a running OpenStory server.

Usage:
    python3 scripts/test_turn_quality.py
    python3 scripts/test_turn_quality.py --verbose
"""

import json
import sys
import urllib.request
from collections import Counter

API = "http://localhost:3002"
VERBOSE = "--verbose" in sys.argv

def fetch(url):
    return json.loads(urllib.request.urlopen(url).read())

def test(name, passed, msg=""):
    status = "PASS" if passed else "FAIL"
    print(f"  {status}: {name}" + (f" — {msg}" if msg else ""))
    return passed

def main():
    sessions = fetch(f"{API}/api/sessions")
    print(f"Testing {len(sessions)} sessions\n")

    total_tests = 0
    total_passed = 0
    total_turns = 0
    total_events_in_turns = 0
    total_duplicates = 0
    total_gaps = 0

    for s in sessions:
        sid = s["session_id"]
        try:
            turns_data = fetch(f"{API}/api/sessions/{sid}/turns")
            turns = turns_data.get("turns", [])
        except:
            continue

        if not turns:
            continue

        short = sid[:8]
        if VERBOSE:
            print(f"\n  Session {short}: {len(turns)} turns")

        total_turns += len(turns)

        # Invariant 1: No duplicate event IDs within this session's turns
        all_eids = []
        for t in turns:
            all_eids.extend(t.get("event_ids", []))
        total_events_in_turns += len(all_eids)
        unique = len(set(all_eids))
        dupes = len(all_eids) - unique
        total_duplicates += dupes

        total_tests += 1
        total_passed += test(
            f"{short}: no duplicate event IDs",
            dupes == 0,
            f"{dupes} duplicates across {len(turns)} turns" if dupes > 0 else "",
        ) if VERBOSE or dupes > 0 else (1 if dupes == 0 else 0)

        # Invariant 2: Turn numbers sequential
        numbers = [t["turn_number"] for t in turns]
        gaps = sum(1 for i in range(1, len(numbers)) if numbers[i] - numbers[i-1] > 1)
        total_gaps += gaps

        total_tests += 1
        total_passed += test(
            f"{short}: sequential turn numbers",
            gaps == 0,
            f"{gaps} gaps in {numbers}" if gaps > 0 else "",
        ) if VERBOSE or gaps > 0 else (1 if gaps == 0 else 0)

        # Invariant 3: Valid session_ids (not unknown)
        unknowns = sum(1 for t in turns if t.get("session_id") in ("", "unknown"))
        total_tests += 1
        passed = unknowns == 0
        total_passed += 1 if passed else 0
        if not passed:
            test(f"{short}: valid session_ids", False, f"{unknowns} unknown")

        # Invariant 4: Valid stop_reason
        bad_stops = [t for t in turns if t.get("stop_reason") not in ("end_turn", "tool_use")]
        total_tests += 1
        passed = len(bad_stops) == 0
        total_passed += 1 if passed else 0
        if not passed:
            test(f"{short}: valid stop_reasons", False, f"{len(bad_stops)} invalid")

        # Invariant 5: Every turn has event_ids
        empty_turns = [t for t in turns if not t.get("event_ids")]
        total_tests += 1
        passed = len(empty_turns) == 0
        total_passed += 1 if passed else 0
        if not passed:
            test(f"{short}: turns have event_ids", False, f"{len(empty_turns)} empty")

        # Invariant 6: scope_depth not inflated
        inflated = [t for t in turns if t.get("scope_depth", 0) > 5]
        total_tests += 1
        passed = len(inflated) == 0
        total_passed += 1 if passed else 0
        if not passed:
            test(f"{short}: scope_depth not inflated", False,
                 f"{len(inflated)} turns with depth > 5")

    # Summary
    print(f"\n{'='*60}")
    print(f"Sessions: {len(sessions)}")
    print(f"Total turns: {total_turns}")
    print(f"Total events in turns: {total_events_in_turns}")
    print(f"Duplicate event IDs: {total_duplicates}")
    print(f"Turn number gaps: {total_gaps}")
    print(f"Tests: {total_passed}/{total_tests} passed")

    if total_duplicates > 0:
        print(f"\n⚠ {total_duplicates} duplicate event IDs — accumulator reset bug")
    if total_gaps > 0:
        print(f"\n⚠ {total_gaps} turn number gaps — turn counting issue")
    if total_passed == total_tests:
        print("\n✓ All data quality checks passed")

    sys.exit(0 if total_passed == total_tests else 1)

if __name__ == "__main__":
    main()
