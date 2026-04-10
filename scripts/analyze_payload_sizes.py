"""Analyze payload sizes across sessions to understand truncation impact.

Measures the actual size difference between truncated and untruncated payloads
by looking at the payload_bytes field (original content size) vs current content.

Usage:
    uv run python scripts/analyze_payload_sizes.py [--url URL]
"""

import argparse
import json
import urllib.request
from collections import Counter, defaultdict


def fetch(base_url: str, path: str):
    return json.loads(urllib.request.urlopen(f"{base_url}{path}").read())


def analyze_payloads(base_url: str) -> None:
    sessions = fetch(base_url, "/api/sessions")["sessions"]
    main_sessions = [s for s in sessions if not s["session_id"].startswith("agent-")]

    print(f"Analyzing {len(main_sessions)} main sessions")
    print()

    total_records = 0
    truncated_count = 0
    truncated_original_total = 0  # sum of original bytes for truncated records
    truncated_current_total = 0   # sum of current output bytes for truncated records (approx 2000 each)

    size_buckets = Counter()
    sizes_by_type = defaultdict(list)
    largest = []

    for session in main_sessions:
        sid = session["session_id"]
        try:
            records = fetch(base_url, f"/api/sessions/{sid}/records")
        except Exception:
            continue

        for r in records:
            total_records += 1
            pb = r.get("payload_bytes", 0)
            truncated = r.get("truncated", False)
            rt = r.get("record_type", "?")

            if truncated and pb > 0:
                truncated_count += 1
                truncated_original_total += pb
                truncated_current_total += 2000  # threshold
                largest.append((pb, rt, sid[:12]))

            # Use payload_bytes for size distribution (original size)
            size = pb if pb > 0 else len(json.dumps(r.get("payload", {})))
            sizes_by_type[rt].append(size)

            if size < 100:
                size_buckets["< 100B"] += 1
            elif size < 500:
                size_buckets["100B-500B"] += 1
            elif size < 1000:
                size_buckets["500B-1KB"] += 1
            elif size < 2000:
                size_buckets["1KB-2KB"] += 1
            elif size < 5000:
                size_buckets["2KB-5KB"] += 1
            elif size < 10000:
                size_buckets["5KB-10KB"] += 1
            elif size < 50000:
                size_buckets["10KB-50KB"] += 1
            else:
                size_buckets["50KB+"] += 1

    saved_bytes = truncated_original_total - truncated_current_total

    print(f"Total records: {total_records}")
    print(f"Truncated: {truncated_count} ({truncated_count/max(total_records,1)*100:.1f}%)")
    print()
    print(f"Truncated records original size: {truncated_original_total/1024:.0f} KB")
    print(f"Truncated records current size:  {truncated_current_total/1024:.0f} KB (capped at 2KB each)")
    print(f"Bandwidth saved by truncation:   {saved_bytes/1024:.0f} KB")
    print(f"Per-session avg savings:         {saved_bytes/max(len(main_sessions),1)/1024:.0f} KB")
    print()

    print("=== Payload Size Distribution (original sizes) ===")
    bucket_order = ["< 100B", "100B-500B", "500B-1KB", "1KB-2KB",
                    "2KB-5KB", "5KB-10KB", "10KB-50KB", "50KB+"]
    for bucket in bucket_order:
        count = size_buckets.get(bucket, 0)
        pct = count / max(total_records, 1) * 100
        bar = "#" * int(pct)
        print(f"  {bucket:12s}  {count:5d} ({pct:5.1f}%)  {bar}")
    print()

    print("=== Average/Max by Record Type ===")
    for rt in sorted(sizes_by_type.keys()):
        sizes = sizes_by_type[rt]
        avg = sum(sizes) / len(sizes)
        mx = max(sizes)
        mn = min(sizes)
        print(f"  {rt:25s}  min={mn:6d}B  avg={avg:8.0f}B  max={mx:8d}B  n={len(sizes)}")
    print()

    if largest:
        largest.sort(reverse=True)
        print("=== Top 10 Largest Truncated Payloads ===")
        for pb, rt, sid in largest[:10]:
            print(f"  {pb:8d}B ({pb/1024:.1f}KB)  {rt}  session {sid}")
        print()

    # Threshold analysis
    would_truncate_at = {}
    for threshold in [2000, 5000, 10000, 20000, 50000]:
        count = sum(1 for pb, _, _ in largest if pb > threshold)
        saved = sum(pb - threshold for pb, _, _ in largest if pb > threshold)
        would_truncate_at[threshold] = (count, saved)

    print("=== Threshold Analysis ===")
    print(f"  {'Threshold':>12s}  {'Truncated':>10s}  {'Saved':>10s}")
    for threshold, (count, saved) in would_truncate_at.items():
        print(f"  {threshold:>10d}B  {count:>10d}  {saved/1024:>8.0f} KB")
    print(f"  {'No limit':>12s}  {'0':>10s}  {'0':>10s} KB")


def _test():
    """Self-tests for size analysis logic."""
    # Simulate records
    records = [
        {"payload_bytes": 100, "truncated": False, "record_type": "tool_result", "payload": {"output": "x" * 100}},
        {"payload_bytes": 500, "truncated": False, "record_type": "tool_result", "payload": {"output": "x" * 500}},
        {"payload_bytes": 5000, "truncated": True, "record_type": "tool_result", "payload": {"output": "x" * 2000}},
        {"payload_bytes": 40000, "truncated": True, "record_type": "tool_result", "payload": {"output": "x" * 2000}},
        {"payload_bytes": 200, "truncated": False, "record_type": "tool_call", "payload": {"name": "Read"}},
    ]

    truncated = [r for r in records if r.get("truncated")]
    assert len(truncated) == 2

    original_total = sum(r["payload_bytes"] for r in truncated)
    assert original_total == 45000  # 5000 + 40000

    current_total = len(truncated) * 2000  # each capped at threshold
    assert current_total == 4000

    saved = original_total - current_total
    assert saved == 41000  # 45KB original - 4KB truncated = 41KB saved

    # Threshold analysis
    at_5k = sum(1 for r in truncated if r["payload_bytes"] > 5000)
    assert at_5k == 1  # only the 40KB one

    at_50k = sum(1 for r in truncated if r["payload_bytes"] > 50000)
    assert at_50k == 0  # none

    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Analyze payload sizes")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    args = parser.parse_args()

    if args.test:
        _test()
    else:
        analyze_payloads(args.url)
