#!/usr/bin/env python3
# /// script
# requires-python = ">=3.10"
# ///
"""Inventory the layers of signal in the OpenStory event stream.

The point: before adding more shape-layer extractors, enumerate what's
actually in the data. Each row of the output is a *candidate layer* —
something with enough volume and internal structure to mine.

Walks recent sessions via the REST API and reports:
  - record_type histogram
  - assistant_message content-block types
  - tool_call tool-name histogram + which input-keys each tool carries
  - tool_result content shape
  - system_event subtypes
  - user_message texture (plain / image-bearing / synthetic-tagged)
  - turn_end reasons
  - per-record-type sample payload keys

Usage:
    uv run scripts/_probe_data_layers.py
    uv run scripts/_probe_data_layers.py --limit 100
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.request
from collections import Counter, defaultdict

API = "http://localhost:3002/api"

SYNTHETIC_TAG_RE = re.compile(
    r"<(system-reminder|task-notification|local-command-[a-z]+|"
    r"command-(?:name|message|args|stdout|stderr))",
    re.IGNORECASE,
)


def fetch(path: str) -> object:
    with urllib.request.urlopen(f"{API}{path}", timeout=20) as r:
        return json.loads(r.read())


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--limit", type=int, default=200,
                    help="number of most recent sessions to scan")
    args = ap.parse_args()

    sd = fetch("/sessions")
    sessions = sd if isinstance(sd, list) else sd.get("sessions", [])
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    sessions = sessions[: args.limit]

    record_types: Counter = Counter()
    record_keys: dict[str, Counter] = defaultdict(Counter)

    asst_block_types: Counter = Counter()
    asst_block_keys: dict[str, Counter] = defaultdict(Counter)

    tool_names: Counter = Counter()
    tool_input_keys: dict[str, Counter] = defaultdict(Counter)

    tool_result_kinds: Counter = Counter()

    sys_subtypes: Counter = Counter()
    sys_keys: Counter = Counter()

    user_textures: Counter = Counter()

    turn_reasons: Counter = Counter()

    total_records = 0
    for i, s in enumerate(sessions, 1):
        sid = s["session_id"]
        if i % 25 == 0:
            print(f"  scanned {i}/{len(sessions)} sessions, records so far: {total_records}",
                  file=sys.stderr)
        try:
            data = fetch(f"/sessions/{sid}/records")
        except Exception as e:
            print(f"  warn: {sid}: {e}", file=sys.stderr)
            continue
        recs = data if isinstance(data, list) else data.get("records", [])
        for rec in recs:
            total_records += 1
            rt = rec.get("record_type", "?")
            record_types[rt] += 1
            payload = rec.get("payload") or {}
            if isinstance(payload, dict):
                record_keys[rt].update(payload.keys())

            if rt == "assistant_message":
                content = payload.get("content", [])
                if isinstance(content, list):
                    for b in content:
                        if isinstance(b, dict):
                            bt = b.get("type", "?")
                            asst_block_types[bt] += 1
                            asst_block_keys[bt].update(b.keys())
            elif rt == "tool_call":
                tname = payload.get("name", "?")
                tool_names[tname] += 1
                inp = payload.get("input") or {}
                if isinstance(inp, dict):
                    tool_input_keys[tname].update(inp.keys())
            elif rt == "tool_result":
                content = payload.get("content", payload.get("output", ""))
                if isinstance(content, list):
                    tool_result_kinds["list"] += 1
                    for c in content:
                        if isinstance(c, dict):
                            tool_result_kinds[f"list.{c.get('type','?')}"] += 1
                elif isinstance(content, str):
                    tool_result_kinds["string"] += 1
                else:
                    tool_result_kinds[type(content).__name__] += 1
            elif rt == "system_event":
                sys_subtypes[payload.get("subtype", "?")] += 1
                sys_keys.update(payload.keys())
            elif rt == "user_message":
                c = payload.get("content", "")
                imgs = payload.get("images") or []
                if imgs:
                    user_textures["with_images"] += 1
                if isinstance(c, str):
                    if not c.strip():
                        user_textures["empty"] += 1
                    elif SYNTHETIC_TAG_RE.search(c):
                        user_textures["synthetic_tagged"] += 1
                    else:
                        user_textures["plain_text"] += 1
            elif rt == "turn_end":
                turn_reasons[payload.get("reason", "?")] += 1

    def print_section(title: str, counter: Counter, top: int = 20) -> None:
        print(f"\n## {title}  (n={sum(counter.values())})")
        if not counter:
            print("  (none)")
            return
        for k, v in counter.most_common(top):
            print(f"  {k:<32} {v}")

    print(f"\n# DATA-LAYER INVENTORY")
    print(f"sessions scanned: {len(sessions)}")
    print(f"records seen:     {total_records}")

    print_section("record_type histogram", record_types)

    print("\n## payload keys by record_type")
    for rt, kc in record_keys.items():
        keys = ", ".join(f"{k}({n})" for k, n in kc.most_common(10))
        print(f"  {rt}: {keys}")

    print_section("assistant_message content-block types", asst_block_types)
    print("\n## assistant content-block payload keys")
    for bt, kc in asst_block_keys.items():
        keys = ", ".join(f"{k}({n})" for k, n in kc.most_common(8))
        print(f"  {bt}: {keys}")

    print_section("tool_call.name (top tools)", tool_names, top=30)

    print("\n## tool_call.input keys per tool (top 15 tools)")
    for tname, _ in tool_names.most_common(15):
        keys = ", ".join(f"{k}({n})" for k, n in tool_input_keys[tname].most_common(6))
        print(f"  {tname:<28} {keys}")

    print_section("tool_result content shape", tool_result_kinds)
    print_section("system_event subtypes", sys_subtypes)
    print("\n## system_event payload keys (overall)")
    print(f"  {', '.join(f'{k}({n})' for k, n in sys_keys.most_common(15))}")

    print_section("user_message texture", user_textures)
    print_section("turn_end reasons", turn_reasons)

    return 0


if __name__ == "__main__":
    sys.exit(main())
