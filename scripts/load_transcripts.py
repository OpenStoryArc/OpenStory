#!/usr/bin/env python3
"""
Load Claude transcript JSONL files into flat records for analysis.

Reads raw .jsonl transcript files from ~/.claude/projects/,
parses the native Claude transcript format (not CloudEvents),
and returns flat dicts ready for pandas DataFrames.

Usage:
    from load_transcripts import load_all, load_session

    records = load_all()               # all sessions from last 24h
    records = load_all(hours=168)      # last 7 days

    # Then:
    import pandas as pd
    df = pd.DataFrame(records)
"""

import json
import os
import sys
import time
from pathlib import Path
from typing import Optional

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------

CLAUDE_PROJECTS = Path(os.path.expanduser("~")) / ".claude" / "projects"


# ---------------------------------------------------------------------------
# Parse a single JSONL line into flat record(s)
# ---------------------------------------------------------------------------

def parse_line(line: str) -> list[dict]:
    """Parse a raw Claude transcript line into one or more flat records.

    A single transcript line can produce multiple records because
    assistant messages may contain multiple tool_use blocks, and
    user messages may contain multiple tool_result blocks.
    """
    try:
        obj = json.loads(line)
    except json.JSONDecodeError:
        return []

    line_type = obj.get("type", "")
    session_id = obj.get("sessionId", "")
    timestamp = obj.get("timestamp", "")
    uuid = obj.get("uuid", "")
    cwd = obj.get("cwd", "")
    version = obj.get("version", "")
    git_branch = obj.get("gitBranch", "")

    base = {
        "uuid": uuid,
        "timestamp": timestamp,
        "session_id": session_id,
        "line_type": line_type,
        "cwd": cwd,
        "version": version,
        "git_branch": git_branch,
        # fields filled per record type
        "record_type": "",
        "tool_name": "",
        "tool_id": "",
        "tool_input_summary": "",
        "text": "",
        "is_error": False,
        "duration_ms": None,
        "input_tokens": None,
        "output_tokens": None,
        "subtype": "",
    }

    msg = obj.get("message", {})
    content = []
    if isinstance(msg, dict):
        c = msg.get("content", [])
        if isinstance(c, list):
            content = c
        elif isinstance(c, str):
            content = [{"type": "text", "text": c}]

    records = []

    # ----- user messages -----
    if line_type == "user":
        # Check content blocks
        has_tool_result = any(
            isinstance(b, dict) and b.get("type") == "tool_result"
            for b in content
        )

        if has_tool_result:
            # Each tool_result block -> separate record
            for block in content:
                if isinstance(block, dict) and block.get("type") == "tool_result":
                    r = {**base}
                    r["record_type"] = "tool_result"
                    r["tool_id"] = block.get("tool_use_id", "")
                    output = block.get("content", "")
                    if isinstance(output, list):
                        output = " ".join(
                            b.get("text", "") for b in output
                            if isinstance(b, dict) and b.get("type") == "text"
                        )
                    r["text"] = str(output)[:500]
                    r["is_error"] = block.get("is_error", False)
                    records.append(r)
        else:
            # Regular user prompt
            r = {**base}
            r["record_type"] = "user_message"
            text_parts = []
            for block in content:
                if isinstance(block, dict) and block.get("type") == "text":
                    text_parts.append(block.get("text", ""))
            r["text"] = " ".join(text_parts)[:500]
            records.append(r)

    # ----- assistant messages -----
    elif line_type == "assistant":
        for block in content:
            if not isinstance(block, dict):
                continue
            btype = block.get("type", "")

            if btype == "thinking":
                r = {**base}
                r["record_type"] = "reasoning"
                r["text"] = (block.get("thinking") or "")[:500]
                records.append(r)

            elif btype == "text":
                r = {**base}
                r["record_type"] = "assistant_message"
                r["text"] = (block.get("text") or "")[:500]
                records.append(r)

            elif btype == "tool_use":
                r = {**base}
                r["record_type"] = "tool_call"
                r["tool_name"] = block.get("name", "")
                r["tool_id"] = block.get("id", "")
                inp = block.get("input", {})
                if isinstance(inp, dict):
                    r["tool_input_summary"] = (
                        inp.get("command", "")
                        or inp.get("file_path", "")
                        or inp.get("pattern", "")
                        or inp.get("query", "")
                        or inp.get("url", "")
                        or (inp.get("prompt", "") or "")[:80]
                        or (inp.get("description", "") or "")[:80]
                        or ""
                    )[:200]
                records.append(r)

    # ----- progress events -----
    elif line_type == "progress":
        r = {**base}
        r["record_type"] = "progress"
        data = obj.get("data", {})
        r["subtype"] = data.get("type", "") if isinstance(data, dict) else ""
        records.append(r)

    # ----- system events -----
    elif line_type == "system":
        r = {**base}
        r["record_type"] = "system_event"
        r["subtype"] = obj.get("subtype", "")
        r["duration_ms"] = obj.get("durationMs")
        records.append(r)

    # ----- file snapshots -----
    elif line_type == "file-history-snapshot":
        r = {**base}
        r["record_type"] = "file_snapshot"
        records.append(r)

    # ----- queue operations -----
    elif line_type == "queue-operation":
        r = {**base}
        r["record_type"] = "queue_operation"
        r["subtype"] = obj.get("operation", "")
        records.append(r)

    # ----- last-prompt / summary / other -----
    else:
        r = {**base}
        r["record_type"] = line_type or "unknown"
        records.append(r)

    return records


# ---------------------------------------------------------------------------
# File discovery
# ---------------------------------------------------------------------------

def find_transcript_files(hours: float = 24) -> list[Path]:
    """Find .jsonl transcript files modified within the last `hours` hours."""
    cutoff = time.time() - (hours * 3600)
    files = []

    if not CLAUDE_PROJECTS.exists():
        print(f"Warning: {CLAUDE_PROJECTS} does not exist", file=sys.stderr)
        return files

    for jsonl in CLAUDE_PROJECTS.rglob("*.jsonl"):
        try:
            if jsonl.stat().st_mtime >= cutoff:
                files.append(jsonl)
        except OSError:
            continue

    files.sort(key=lambda p: p.stat().st_mtime, reverse=True)
    return files


# ---------------------------------------------------------------------------
# Loading
# ---------------------------------------------------------------------------

def load_file(path: Path) -> list[dict]:
    """Load a single JSONL file into a list of flat dicts."""
    records = []
    with open(path, "r", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            records.extend(parse_line(line))
    return records


def load_all(hours: float = 24) -> list[dict]:
    """Load all recent transcript files into a flat list of records."""
    files = find_transcript_files(hours)
    print(f"Found {len(files)} transcript files from last {hours}h")

    all_records = []
    for f in files:
        recs = load_file(f)
        all_records.extend(recs)

    print(f"Loaded {len(all_records)} records from {len(files)} files")
    return all_records


def load_session(session_id: str) -> list[dict]:
    """Load records for a specific session by finding its transcript file."""
    for jsonl in CLAUDE_PROJECTS.rglob("*.jsonl"):
        if session_id in jsonl.name:
            return load_file(jsonl)
    # Fall back: scan files for matching session_id in content
    print(f"No file named {session_id}, scanning...", file=sys.stderr)
    for jsonl in CLAUDE_PROJECTS.rglob("*.jsonl"):
        recs = load_file(jsonl)
        if recs and recs[0].get("session_id") == session_id:
            return recs
    return []


# ---------------------------------------------------------------------------
# CLI: dump to CSV
# ---------------------------------------------------------------------------

def main():
    import argparse
    parser = argparse.ArgumentParser(description="Load Claude transcripts to CSV")
    parser.add_argument("--hours", type=float, default=24, help="Hours of history (default: 24)")
    parser.add_argument("--session", help="Specific session ID")
    parser.add_argument("--output", "-o", default="events.csv", help="Output CSV path")
    parser.add_argument("--list-files", action="store_true", help="Just list transcript files")
    parser.add_argument("--stats", action="store_true", help="Print quick stats")
    args = parser.parse_args()

    if args.list_files:
        files = find_transcript_files(args.hours)
        for f in files:
            mtime = time.strftime("%Y-%m-%d %H:%M", time.localtime(f.stat().st_mtime))
            size_kb = f.stat().st_size / 1024
            print(f"  {mtime}  {size_kb:>8.0f}KB  {f.name[:60]}")
        print(f"\n  {len(files)} files")
        return

    if args.session:
        records = load_session(args.session)
    else:
        records = load_all(args.hours)

    if not records:
        print("No records found.")
        return

    if args.stats:
        from collections import Counter
        types = Counter(r["record_type"] for r in records)
        tools = Counter(r["tool_name"] for r in records if r["record_type"] == "tool_call")
        subtypes = Counter(r["subtype"] for r in records if r["subtype"])
        sessions = Counter(r["session_id"] for r in records)

        print(f"\n  Records: {len(records):,}")
        print(f"  Sessions: {len(sessions):,}")

        print("\n  --- Record Types ---")
        for t, c in types.most_common():
            print(f"  {t:25s} {c:>6} ({c/len(records)*100:.1f}%)")

        print("\n  --- Tools ---")
        for t, c in tools.most_common(15):
            print(f"  {t:20s} {c:>6}")

        print("\n  --- Subtypes ---")
        for t, c in subtypes.most_common(10):
            print(f"  {t:25s} {c:>6}")

        print(f"\n  --- Top sessions ---")
        for sid, c in sessions.most_common(10):
            print(f"  {sid[:16]:18s} {c:>6} records")
        return

    # Write CSV
    import csv
    fields = list(records[0].keys())
    with open(args.output, "w", newline="", encoding="utf-8") as f:
        writer = csv.DictWriter(f, fieldnames=fields)
        writer.writeheader()
        writer.writerows(records)

    print(f"Wrote {len(records)} records to {args.output}")


if __name__ == "__main__":
    main()
