"""Explore agent-specific field shapes in the OpenStory event store.

Answers: what fields are truly universal vs agent-specific?
Informs the monadic EventData design (raw + Option<AgentPayload>).

Usage:
    uv run python scripts/explore_agent_payloads.py
    uv run python scripts/explore_agent_payloads.py --db path/to/open-story.db
    uv run python scripts/explore_agent_payloads.py --json
    uv run python scripts/explore_agent_payloads.py --test
"""

import argparse
import json
import sqlite3
import sys
from collections import defaultdict
from pathlib import Path


DEFAULT_DB = Path(__file__).parent.parent / "data" / "open-story.db"


def connect(db_path: str) -> sqlite3.Connection:
    conn = sqlite3.connect(db_path)
    conn.row_factory = sqlite3.Row
    return conn


def top_level_keys_by_agent(conn: sqlite3.Connection) -> dict:
    """Extract all top-level keys in CloudEvent.data, grouped by agent.

    Returns: {agent: {key: count}}
    """
    # The events table stores the full CloudEvent as `payload` (JSON text).
    # CloudEvent.data is the nested object we care about.
    # CloudEvent.agent is the tag.
    rows = conn.execute("""
        SELECT
            json_extract(payload, '$.agent') as agent,
            j.key as field,
            COUNT(*) as freq
        FROM events, json_each(json_extract(payload, '$.data'), '$') as j
        WHERE json_extract(payload, '$.agent') IS NOT NULL
        GROUP BY agent, field
        ORDER BY agent, freq DESC
    """).fetchall()

    result = defaultdict(dict)
    for row in rows:
        result[row["agent"]][row["field"]] = row["freq"]
    return dict(result)


def classify_fields(keys_by_agent: dict) -> dict:
    """Classify fields as shared, agent-only, or universal.

    Returns: {shared: [...], <agent>_only: [...], universal: [...]}
    """
    all_agents = set(keys_by_agent.keys())
    all_keys = set()
    for agent_keys in keys_by_agent.values():
        all_keys.update(agent_keys.keys())

    shared = []
    universal = []
    agent_only = defaultdict(list)

    for key in sorted(all_keys):
        agents_with_key = {a for a, keys in keys_by_agent.items() if key in keys}
        if agents_with_key == all_agents:
            # Check if it's in >90% of events for ALL agents — truly universal
            total_events = {a: sum(keys.values()) // len(keys) for a, keys in keys_by_agent.items()}
            is_universal = all(
                keys_by_agent[a].get(key, 0) > total_events.get(a, 1) * 0.8
                for a in all_agents
            )
            if is_universal:
                universal.append(key)
            else:
                shared.append(key)
        elif len(agents_with_key) == 1:
            agent = agents_with_key.pop()
            agent_only[agent].append(key)
        else:
            shared.append(key)

    return {
        "universal": universal,
        "shared": shared,
        **{f"{agent}_only": fields for agent, fields in agent_only.items()},
    }


def token_usage_shapes(conn: sqlite3.Connection, limit: int = 5) -> dict:
    """Sample token_usage objects per agent to see internal key differences.

    Returns: {agent: [sample_usage_objects]}
    """
    rows = conn.execute("""
        SELECT
            json_extract(payload, '$.agent') as agent,
            json_extract(payload, '$.data.token_usage') as usage
        FROM events
        WHERE json_extract(payload, '$.data.token_usage') IS NOT NULL
          AND json_extract(payload, '$.agent') IS NOT NULL
        ORDER BY RANDOM()
    """).fetchall()

    result = defaultdict(list)
    seen_shapes = defaultdict(set)
    for row in rows:
        agent = row["agent"]
        usage = row["usage"]
        if usage:
            try:
                obj = json.loads(usage)
                shape = tuple(sorted(obj.keys()))
                if shape not in seen_shapes[agent]:
                    seen_shapes[agent].add(shape)
                    result[agent].append(obj)
                    if len(result[agent]) >= limit:
                        continue
            except (json.JSONDecodeError, TypeError):
                pass

    return dict(result)


def token_usage_key_freq(conn: sqlite3.Connection) -> dict:
    """Count frequency of each key inside token_usage, per agent.

    Returns: {agent: {key: count}}
    """
    rows = conn.execute("""
        SELECT
            json_extract(payload, '$.agent') as agent,
            j.key as field,
            COUNT(*) as freq
        FROM events,
             json_each(json_extract(payload, '$.data.token_usage'), '$') as j
        WHERE json_extract(payload, '$.data.token_usage') IS NOT NULL
          AND json_extract(payload, '$.agent') IS NOT NULL
        GROUP BY agent, field
        ORDER BY agent, freq DESC
    """).fetchall()

    result = defaultdict(dict)
    for row in rows:
        result[row["agent"]][row["field"]] = row["freq"]
    return dict(result)


def tool_block_samples(conn: sqlite3.Connection, limit: int = 3) -> dict:
    """Sample tool_use content blocks per agent to see structural differences.

    Returns: {agent: [sample_blocks]}
    """
    rows = conn.execute("""
        SELECT
            json_extract(payload, '$.agent') as agent,
            json_extract(payload, '$.data.raw.message.content') as content
        FROM events
        WHERE json_extract(payload, '$.subtype') = 'message.assistant.tool_use'
          AND json_extract(payload, '$.agent') IS NOT NULL
        ORDER BY RANDOM()
    """).fetchall()

    result = defaultdict(list)
    for row in rows:
        agent = row["agent"]
        if len(result[agent]) >= limit:
            continue
        content = row["content"]
        if content:
            try:
                blocks = json.loads(content)
                # Find the tool block (not text blocks)
                for block in blocks:
                    if isinstance(block, dict) and block.get("type") in ("tool_use", "toolCall"):
                        # Just keep the structure, truncate large inputs
                        sample = {k: v for k, v in block.items()}
                        if "input" in sample and isinstance(sample["input"], dict):
                            keys = list(sample["input"].keys())
                            sample["input"] = f"<{len(keys)} keys: {', '.join(keys[:5])}>"
                        if "arguments" in sample and isinstance(sample["arguments"], dict):
                            keys = list(sample["arguments"].keys())
                            sample["arguments"] = f"<{len(keys)} keys: {', '.join(keys[:5])}>"
                        result[agent].append(sample)
                        break
            except (json.JSONDecodeError, TypeError):
                pass

    return dict(result)


def extra_field_frequency(conn: sqlite3.Connection) -> dict:
    """For each agent, show frequency of fields that are NOT in the current
    EventData common set (raw, seq, session_id, uuid, parent_uuid, cwd, text,
    model, stop_reason, token_usage, tool, args, content_types).

    These are the fields currently landing in `extra` — candidates for
    agent-specific payload structs.

    Returns: {agent: {field: count}}
    """
    common_fields = {
        "raw", "seq", "session_id", "uuid", "parent_uuid", "cwd", "text",
        "model", "stop_reason", "token_usage", "tool", "args", "content_types",
    }

    rows = conn.execute("""
        SELECT
            json_extract(payload, '$.agent') as agent,
            j.key as field,
            COUNT(*) as freq
        FROM events, json_each(json_extract(payload, '$.data'), '$') as j
        WHERE json_extract(payload, '$.agent') IS NOT NULL
        GROUP BY agent, field
        ORDER BY agent, freq DESC
    """).fetchall()

    result = defaultdict(dict)
    for row in rows:
        if row["field"] not in common_fields:
            result[row["agent"]][row["field"]] = row["freq"]
    return dict(result)


def print_report(db_path: str) -> dict:
    """Generate and print the full analysis report. Returns raw data for --json."""
    conn = connect(db_path)

    total = conn.execute("SELECT COUNT(*) as n FROM events").fetchone()["n"]
    by_agent = conn.execute("""
        SELECT json_extract(payload, '$.agent') as agent, COUNT(*) as n
        FROM events GROUP BY agent ORDER BY n DESC
    """).fetchall()

    print(f"Total events: {total}")
    agent_counts = ", ".join(f"{r['agent'] or 'NULL'}: {r['n']}" for r in by_agent)
    print(f"By agent: {agent_counts}")
    print()

    # 1. Top-level keys
    keys = top_level_keys_by_agent(conn)
    classification = classify_fields(keys)

    print("=" * 70)
    print("FIELD CLASSIFICATION")
    print("=" * 70)
    print(f"\nUniversal (>80% of events for ALL agents):")
    for f in classification.get("universal", []):
        freqs = ", ".join(f"{a}: {keys[a].get(f, 0)}" for a in keys)
        print(f"  {f:20s}  ({freqs})")

    print(f"\nShared (both agents, but not universal):")
    for f in classification.get("shared", []):
        freqs = ", ".join(f"{a}: {keys[a].get(f, 0)}" for a in keys)
        print(f"  {f:20s}  ({freqs})")

    for agent in sorted(keys.keys()):
        only_key = f"{agent}_only"
        fields = classification.get(only_key, [])
        if fields:
            print(f"\n{agent} only:")
            for f in fields:
                print(f"  {f:20s}  (count: {keys[agent].get(f, 0)})")

    # 2. Extra fields (not in current common set)
    extras = extra_field_frequency(conn)
    print(f"\n{'=' * 70}")
    print("FIELDS CURRENTLY IN `extra` (not named in EventData)")
    print("=" * 70)
    for agent in sorted(extras.keys()):
        print(f"\n{agent}:")
        for field, count in sorted(extras[agent].items(), key=lambda x: -x[1]):
            print(f"  {field:25s}  {count:>6d}")

    # 3. Token usage shapes
    usage_keys = token_usage_key_freq(conn)
    print(f"\n{'=' * 70}")
    print("TOKEN USAGE INTERNAL KEYS")
    print("=" * 70)
    for agent in sorted(usage_keys.keys()):
        print(f"\n{agent}:")
        for field, count in sorted(usage_keys[agent].items(), key=lambda x: -x[1]):
            print(f"  {field:25s}  {count:>6d}")

    usage_samples = token_usage_shapes(conn)
    print(f"\n{'=' * 70}")
    print("TOKEN USAGE SAMPLE SHAPES")
    print("=" * 70)
    for agent in sorted(usage_samples.keys()):
        print(f"\n{agent}:")
        for sample in usage_samples[agent]:
            print(f"  keys: {sorted(sample.keys())}")

    # 4. Tool block samples
    tool_blocks = tool_block_samples(conn)
    print(f"\n{'=' * 70}")
    print("TOOL CALL BLOCK STRUCTURES")
    print("=" * 70)
    for agent in sorted(tool_blocks.keys()):
        print(f"\n{agent}:")
        for sample in tool_blocks[agent]:
            print(f"  {json.dumps(sample, indent=4)}")

    conn.close()

    return {
        "total_events": total,
        "by_agent": {r["agent"]: r["n"] for r in by_agent},
        "keys_by_agent": keys,
        "classification": classification,
        "extra_fields": extras,
        "token_usage_keys": usage_keys,
        "token_usage_samples": usage_samples,
        "tool_block_samples": tool_blocks,
    }


def run_tests():
    """Smoke tests using in-memory SQLite."""
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.execute("""CREATE TABLE events (
        id TEXT PRIMARY KEY,
        session_id TEXT NOT NULL,
        subtype TEXT NOT NULL DEFAULT '',
        timestamp TEXT NOT NULL DEFAULT '',
        agent_id TEXT,
        parent_uuid TEXT,
        payload TEXT NOT NULL
    )""")

    # Insert a Claude Code event
    cc_event = json.dumps({
        "agent": "claude-code",
        "subtype": "message.assistant.text",
        "data": {
            "raw": {"type": "assistant"},
            "seq": 1,
            "session_id": "s1",
            "text": "hello",
            "model": "claude-opus-4-6",
            "token_usage": {"input_tokens": 100, "output_tokens": 50},
            "is_sidechain": False,
        }
    })
    conn.execute("INSERT INTO events VALUES (?, ?, ?, ?, ?, ?, ?)",
                 ("e1", "s1", "message.assistant.text", "2026-01-01", None, None, cc_event))

    # Insert a pi-mono event
    pm_event = json.dumps({
        "agent": "pi-mono",
        "subtype": "message.assistant.text",
        "data": {
            "raw": {"type": "message"},
            "seq": 1,
            "session_id": "s2",
            "text": "hi",
            "model": "claude-4",
            "token_usage": {"input": 80, "output": 40, "cacheRead": 10},
            "provider": "anthropic",
        }
    })
    conn.execute("INSERT INTO events VALUES (?, ?, ?, ?, ?, ?, ?)",
                 ("e2", "s2", "message.assistant.text", "2026-01-01", None, None, pm_event))

    conn.commit()

    # Test key extraction
    rows = conn.execute("""
        SELECT json_extract(payload, '$.agent') as agent, j.key as field, COUNT(*) as freq
        FROM events, json_each(json_extract(payload, '$.data'), '$') as j
        WHERE json_extract(payload, '$.agent') IS NOT NULL
        GROUP BY agent, field
    """).fetchall()

    keys_by_agent = defaultdict(dict)
    for row in rows:
        keys_by_agent[row["agent"]][row["field"]] = row["freq"]

    assert "claude-code" in keys_by_agent, "Should find claude-code events"
    assert "pi-mono" in keys_by_agent, "Should find pi-mono events"
    assert "is_sidechain" in keys_by_agent["claude-code"], "is_sidechain is claude-code specific"
    assert "provider" in keys_by_agent["pi-mono"], "provider is pi-mono specific"
    assert "text" in keys_by_agent["claude-code"] and "text" in keys_by_agent["pi-mono"], "text is shared"

    # Test classification
    classification = classify_fields(dict(keys_by_agent))
    assert "provider" in classification.get("pi-mono_only", []), f"provider should be pi-mono only, got: {classification}"
    assert "is_sidechain" in classification.get("claude-code_only", []), f"is_sidechain should be claude-code only, got: {classification}"

    conn.close()
    print("All tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Explore agent payload field shapes")
    parser.add_argument("--db", default=str(DEFAULT_DB), help="Path to open-story.db")
    parser.add_argument("--json", action="store_true", help="Output as JSON")
    parser.add_argument("--test", action="store_true", help="Run smoke tests")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    data = print_report(args.db)
    if args.json:
        print(json.dumps(data, indent=2))
