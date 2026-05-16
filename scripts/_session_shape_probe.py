#!/usr/bin/env python3
"""Probe one session's records to confirm shape signals are extractable.

For a chosen session_id, prints:
  - distinct record_types and counts
  - a sample of tool_call payloads (tool name, input keys, raw fragment)
  - whether file paths are extractable from Write/Edit/Read
  - whether Agent tool calls (subagent spawns) carry identifiable info

Throwaway probe. Output drives the feature design of analyze_session_shape.py.
"""

import argparse
import json
import sys
import urllib.request
from collections import Counter

API = "http://localhost:3002/api"


def fetch(sid: str) -> list:
    return json.loads(urllib.request.urlopen(f"{API}/sessions/{sid}/records").read())


def main() -> None:
    p = argparse.ArgumentParser()
    p.add_argument("--session", required=True)
    p.add_argument("--limit", type=int, default=200)
    args = p.parse_args()

    records = fetch(args.session)
    print(f"session={args.session}  total_records={len(records)}\n")

    rt_counts = Counter(r.get("record_type", "?") for r in records)
    print("record_type counts:")
    for rt, n in rt_counts.most_common():
        print(f"  {rt:<20} {n:>5}")

    tool_names = Counter()
    file_path_samples = {}      # tool_name -> sample input
    agent_call_samples = []     # subagent spawn payloads
    parallel_widths = []        # tools per assistant_message turn

    current_burst = 0
    for r in records:
        rt = r.get("record_type", "?")
        if rt == "tool_call":
            payload = r.get("payload") or {}
            tname = payload.get("name", "?")
            tool_names[tname] += 1
            current_burst += 1
            inp = payload.get("input") or {}
            if tname not in file_path_samples and isinstance(inp, dict):
                # Record one example for each tool to inspect shape
                file_path_samples[tname] = {k: (str(v)[:80] if not isinstance(v, dict) else "<dict>") for k, v in list(inp.items())[:6]}
            if tname in ("Agent", "Task"):
                agent_call_samples.append({k: (str(v)[:80] if not isinstance(v, dict) else "<dict>") for k, v in (inp.items() if isinstance(inp, dict) else [])})
        else:
            if current_burst > 0:
                parallel_widths.append(current_burst)
                current_burst = 0
    if current_burst > 0:
        parallel_widths.append(current_burst)

    print(f"\ndistinct tools: {len(tool_names)}  total tool_calls: {sum(tool_names.values())}")
    for t, n in tool_names.most_common(20):
        print(f"  {t:<25} {n:>5}")

    print(f"\ntool input shapes (first sample of each):")
    for t in sorted(file_path_samples):
        print(f"  {t}: {file_path_samples[t]}")

    print(f"\nAgent/Task spawn samples (count={len(agent_call_samples)}):")
    for s in agent_call_samples[:5]:
        print(f"  {s}")

    if parallel_widths:
        print(f"\ntool-burst widths (tools per assistant message, approx):")
        print(f"  count={len(parallel_widths)} max={max(parallel_widths)} avg={sum(parallel_widths)/len(parallel_widths):.2f}")
        wide = sum(1 for w in parallel_widths if w >= 3)
        print(f"  bursts with >=3 tools: {wide}")


if __name__ == "__main__":
    main()
