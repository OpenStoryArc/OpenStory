#!/usr/bin/env python3
"""Probe the full tool-name vocabulary across all sessions.

Helps the analyzer normalize agent-specific naming (Claude Code's CamelCase
vs pi-mono's snake_case) without missing a tool.
"""

import json
import urllib.request
from collections import Counter

API = "http://localhost:3002/api"


def main() -> None:
    sessions = json.loads(urllib.request.urlopen(f"{API}/sessions").read())["sessions"]
    sessions.sort(key=lambda s: -s["event_count"])

    upper: Counter[str] = Counter()
    lower: Counter[str] = Counter()
    other: Counter[str] = Counter()

    # Sample top 50 sessions for vocabulary
    for s in sessions[:80]:
        try:
            records = json.loads(urllib.request.urlopen(f"{API}/sessions/{s['session_id']}/records").read())
        except Exception:
            continue
        for r in records:
            if r.get("record_type") != "tool_call":
                continue
            name = (r.get("payload") or {}).get("name", "?")
            if not isinstance(name, str):
                continue
            if name and name[0].isupper():
                upper[name] += 1
            elif name and name[0].islower():
                lower[name] += 1
            else:
                other[name] += 1

    print("=== UPPERCASE (Claude Code style) — top 25 ===")
    for n, c in upper.most_common(25):
        print(f"  {n:<30} {c}")
    print(f"\n=== lowercase (pi-mono / other) — top 25 ===")
    for n, c in lower.most_common(25):
        print(f"  {n:<30} {c}")
    print(f"\n=== other ===")
    for n, c in other.most_common():
        print(f"  {repr(n):<30} {c}")


if __name__ == "__main__":
    main()
