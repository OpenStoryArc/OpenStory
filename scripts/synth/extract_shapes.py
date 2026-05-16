#!/usr/bin/env python3
"""
Extract a statistical "shapes" profile from a live OpenStory instance.

Hits the OpenStory REST API (default http://localhost:3002), samples
sessions, and computes distributions that downstream synth tools can
draw from:

  - record_type histogram (user_message, assistant_message, tool_use, ...)
  - tool name histogram (Bash, Read, Edit, ...)
  - per-session event_count distribution
  - project distribution
  - hour-of-day activity histogram

The output is a JSON file compatible with synth_transcripts.py's Profile
when fed through generate_corpus.py.

Usage:
    python3 scripts/synth/extract_shapes.py --out shapes.json
    python3 scripts/synth/extract_shapes.py --sample 50 --out shapes.json
    python3 scripts/synth/extract_shapes.py --test
"""
from __future__ import annotations

import argparse
import json
import math
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Optional


DEFAULT_API = "http://localhost:3002"


def _get(url: str, timeout: float = 10.0) -> dict | list:
    with urllib.request.urlopen(url, timeout=timeout) as resp:
        return json.loads(resp.read().decode("utf-8"))


def list_sessions(api: str) -> list[dict]:
    """GET /api/sessions → list of session metadata."""
    payload = _get(f"{api}/api/sessions")
    if isinstance(payload, dict) and "sessions" in payload:
        return payload["sessions"]
    if isinstance(payload, list):
        return payload
    raise ValueError(f"unexpected /api/sessions payload shape: {type(payload).__name__}")


def get_records(api: str, session_id: str) -> list[dict]:
    """GET /api/sessions/{id}/records → list of records."""
    payload = _get(f"{api}/api/sessions/{session_id}/records")
    if isinstance(payload, dict) and "records" in payload:
        return payload["records"]
    if isinstance(payload, list):
        return payload
    return []


def _log_params(values: list[float]) -> tuple[float, float]:
    """Fit a log-normal: return (mu, sigma) of log(values)."""
    if not values:
        return (0.0, 0.0)
    logs = [math.log(max(1.0, v)) for v in values]
    mu = sum(logs) / len(logs)
    var = sum((x - mu) ** 2 for x in logs) / max(1, len(logs) - 1)
    return (mu, math.sqrt(var))


def _percentiles(values: list[float], qs: list[float]) -> dict[str, float]:
    if not values:
        return {f"p{int(q*100)}": 0.0 for q in qs}
    s = sorted(values)
    out = {}
    for q in qs:
        idx = min(len(s) - 1, int(q * (len(s) - 1) + 0.5))
        out[f"p{int(q*100)}"] = float(s[idx])
    return out


def _extract_tools(record: dict) -> list[str]:
    """Pull tool names from a record's payload (best-effort)."""
    payload = record.get("payload") or {}
    rtype = record.get("record_type", "")
    names: list[str] = []
    # OpenStory record types: `tool_call` is the canonical name; older
    # shapes used `tool_use` / `assistant_tool_use`.
    if rtype in ("tool_call", "tool_use", "assistant_tool_use"):
        n = payload.get("name") or payload.get("tool_name")
        if n:
            names.append(n)
    # Some assistant_message records carry content blocks
    content = payload.get("content")
    if isinstance(content, list):
        for block in content:
            if isinstance(block, dict) and block.get("type") == "tool_use":
                n = block.get("name")
                if n:
                    names.append(n)
    return names


def extract_shapes(api: str, sample: Optional[int] = None,
                   verbose: bool = False) -> dict:
    """Sample sessions from the API and compute a shapes profile."""
    sessions = list_sessions(api)
    if verbose:
        print(f"Found {len(sessions)} sessions", file=sys.stderr)

    # --- Session-level distributions (cheap, from list endpoint) ---
    event_counts = [s.get("event_count", 0) for s in sessions if s.get("event_count")]
    project_counts: Counter = Counter(
        (s.get("project_id") or "unknown") for s in sessions
    )
    hour_counts: Counter = Counter()
    for s in sessions:
        ts = s.get("start_time") or s.get("first_event")
        if isinstance(ts, str) and len(ts) >= 13:
            hour = ts[11:13]
            hour_counts[hour] += 1

    # --- Record-level distributions (sampled, expensive) ---
    record_type_counts: Counter = Counter()
    tool_counts: Counter = Counter()
    sampled_session_ids: list[str] = []

    if sample is not None and sample > 0:
        # Pick sessions with the most events first — they're the most informative
        ranked = sorted(sessions, key=lambda s: -(s.get("event_count") or 0))
        for s in ranked[:sample]:
            sid = s.get("session_id")
            if not sid:
                continue
            try:
                records = get_records(api, sid)
            except (urllib.error.URLError, urllib.error.HTTPError, ValueError):
                continue
            sampled_session_ids.append(sid)
            for r in records:
                rtype = r.get("record_type", "unknown")
                record_type_counts[rtype] += 1
                for n in _extract_tools(r):
                    tool_counts[n] += 1
            if verbose:
                print(f"  sampled {sid}: {len(records)} records",
                      file=sys.stderr)

    # --- Normalize ---
    def normalize(c: Counter) -> dict[str, float]:
        total = sum(c.values()) or 1
        return {k: v / total for k, v in c.most_common()}

    ec_mu, ec_sigma = _log_params([float(x) for x in event_counts])

    profile = {
        "schema_version": 1,
        "source": api,
        "session_count": len(sessions),
        "sampled_sessions": len(sampled_session_ids),
        "event_count_lognormal": {"mu": ec_mu, "sigma": ec_sigma},
        "event_count_percentiles": _percentiles(
            [float(x) for x in event_counts], [0.5, 0.9, 0.99]
        ),
        "record_type_dist": normalize(record_type_counts),
        "tool_dist": normalize(tool_counts),
        "project_dist": normalize(project_counts),
        "hour_of_day_dist": dict(sorted(hour_counts.items())),
        "totals": {
            "records_observed": sum(record_type_counts.values()),
            "tool_calls_observed": sum(tool_counts.values()),
        },
    }
    return profile


# =====================================================================
# BDD Tests
# =====================================================================

def _fake_sessions() -> list[dict]:
    return [
        {"session_id": "s1", "event_count": 100, "project_id": "P1",
         "start_time": "2026-04-01T09:00:00Z"},
        {"session_id": "s2", "event_count": 50, "project_id": "P1",
         "start_time": "2026-04-01T14:30:00Z"},
        {"session_id": "s3", "event_count": 200, "project_id": "P2",
         "start_time": "2026-04-02T09:15:00Z"},
    ]


def _fake_records(sid: str) -> list[dict]:
    return {
        "s1": [
            {"record_type": "user_message", "payload": {"content": "hi"}},
            {"record_type": "assistant_message", "payload": {
                "content": [
                    {"type": "tool_use", "name": "Bash"},
                    {"type": "tool_use", "name": "Read"},
                ]
            }},
            {"record_type": "tool_use", "payload": {"name": "Edit"}},
        ],
        "s2": [
            {"record_type": "user_message", "payload": {"content": "go"}},
            {"record_type": "tool_use", "payload": {"name": "Bash"}},
        ],
        "s3": [
            {"record_type": "tool_use", "payload": {"name": "Read"}},
            {"record_type": "tool_use", "payload": {"name": "Read"}},
        ],
    }.get(sid, [])


def run_tests() -> bool:
    """BDD tests using a stubbed API client."""
    passed = failed = 0

    def it(name: str, cond: bool, detail: str = ""):
        nonlocal passed, failed
        if cond:
            passed += 1
            print(f"  ok  {name}")
        else:
            failed += 1
            print(f"  FAIL  {name}" + (f" — {detail}" if detail else ""))

    # Monkey-patch the network calls
    global list_sessions, get_records
    real_list = list_sessions
    real_get = get_records
    list_sessions = lambda api: _fake_sessions()  # noqa: E731
    get_records = lambda api, sid: _fake_records(sid)  # noqa: E731

    try:
        print("\nGiven a stubbed API with 3 sessions across 2 projects")
        prof = extract_shapes("stub://", sample=3)
        it("counts all sessions", prof["session_count"] == 3,
           f"got {prof['session_count']}")
        it("samples all 3", prof["sampled_sessions"] == 3)
        it("has 2 projects", len(prof["project_dist"]) == 2)
        it("project P1 is most common",
           list(prof["project_dist"].keys())[0] == "P1")

        print("\nWhen tool_use records are present")
        td = prof["tool_dist"]
        it("tool_dist sums to ~1.0", abs(sum(td.values()) - 1.0) < 1e-6,
           f"sum={sum(td.values())}")
        it("Bash is in tool_dist", "Bash" in td)
        it("Read is in tool_dist", "Read" in td)
        it("Edit is in tool_dist", "Edit" in td)

        print("\nThen record_type_dist reflects observed types")
        rtd = prof["record_type_dist"]
        it("user_message present", "user_message" in rtd)
        it("tool_use present", "tool_use" in rtd)

        print("\nAnd event_count distribution is computed")
        it("p50 is sane", prof["event_count_percentiles"]["p50"] > 0)
        it("lognormal mu is finite",
           math.isfinite(prof["event_count_lognormal"]["mu"]))

        print("\nAnd hour_of_day_dist tags both 09 and 14")
        it("hour 09 seen", "09" in prof["hour_of_day_dist"])
        it("hour 14 seen", "14" in prof["hour_of_day_dist"])
    finally:
        list_sessions = real_list
        get_records = real_get

    print(f"\n{passed} passed, {failed} failed")
    return failed == 0


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--api", default=DEFAULT_API,
                    help=f"OpenStory API base (default: {DEFAULT_API})")
    ap.add_argument("--sample", type=int, default=20,
                    help="Number of sessions to sample for record-level "
                         "distributions (0 = list-only, default: 20)")
    ap.add_argument("--out", "-o", type=Path,
                    help="Write profile to this path (default: stdout)")
    ap.add_argument("--test", action="store_true",
                    help="Run BDD tests with a stubbed API")
    ap.add_argument("--verbose", "-v", action="store_true")
    args = ap.parse_args()

    if args.test:
        sys.exit(0 if run_tests() else 1)

    try:
        profile = extract_shapes(args.api, sample=args.sample,
                                 verbose=args.verbose)
    except (urllib.error.URLError, urllib.error.HTTPError) as e:
        print(f"error: could not reach OpenStory at {args.api}: {e}",
              file=sys.stderr)
        sys.exit(2)

    blob = json.dumps(profile, indent=2, sort_keys=True)
    if args.out:
        args.out.parent.mkdir(parents=True, exist_ok=True)
        args.out.write_text(blob)
        print(f"wrote {args.out} "
              f"({profile['session_count']} sessions, "
              f"{profile['sampled_sessions']} sampled)")
    else:
        print(blob)


if __name__ == "__main__":
    main()
