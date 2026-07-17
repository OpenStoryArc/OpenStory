#!/usr/bin/env python3
"""Extract seed fixtures from a real Grok Build `updates.jsonl` session.

Default source: the live session that built OpenStory's Grok support
(Max's grok-build project, session 019f6cb5-…).

Produces:
  rs/tests/fixtures/grok/real_turn_*.jsonl   — one complete turn each
  rs/tests/fixtures/grok/seed_tree/…/updates.jsonl — nested layout for
      watcher/container mounts (urlencoded-cwd / session-id / updates.jsonl)

Large tool outputs are truncated (structure preserved) so goldens stay small.
Set --no-truncate to keep full wire fidelity (large).

Usage:
  python3 scripts/extract_grok_session_seed.py
  python3 scripts/extract_grok_session_seed.py --source /path/to/updates.jsonl
  python3 scripts/extract_grok_session_seed.py --test
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / "rs" / "tests" / "fixtures" / "grok"

# This conversation's session (OpenStory Grok support work, 2026-07-16).
DEFAULT_SOURCE = (
    Path.home()
    / ".grok/sessions"
    / "%2FUsers%2Fmaxglassie%2Fprojects%2Fgrok-build"
    / "019f6cb5-f7e4-7bc1-bb25-9985af59619e"
    / "updates.jsonl"
)

# Turns to export as named seeds (index → slug). Chosen for size + shape.
# turn 1: pure text (TUI) — tiny golden-friendly
# turn 2: multi-read/grep about session storage — tool chain
# turn 7: OpenStory vs Grok compare — medium
# turn 9: ACP / MCP — tool mix including search_tool
SEED_TURNS = {
    1: "real_turn_01_text_only",
    2: "real_turn_02_session_storage",
    7: "real_turn_07_openstory_vs_grok",
    9: "real_turn_09_acp_and_mcp",
}

MAX_OUTPUT_CHARS = 800
MAX_THOUGHT_CHARS = 400
MAX_MESSAGE_CHARS = 600


def split_turns(rows: list[dict]) -> list[list[dict]]:
    turns: list[list[dict]] = []
    cur: list[dict] = []
    for row in rows:
        cur.append(row)
        kind = row.get("params", {}).get("update", {}).get("sessionUpdate")
        if kind == "turn_completed":
            turns.append(cur)
            cur = []
    if cur:
        turns.append(cur)
    return turns


def user_text(turn: list[dict]) -> str:
    for row in turn:
        u = row.get("params", {}).get("update", {})
        if u.get("sessionUpdate") == "user_message_chunk":
            t = (u.get("content") or {}).get("text")
            if t:
                return t
    return ""


def truncate_str(s: str, n: int) -> str:
    if len(s) <= n:
        return s
    return s[: n - 1] + "…"


def truncate_raw_output(raw):
    """Shrink tool outputs for fixture size; keep type/shape."""
    if isinstance(raw, str):
        return truncate_str(raw, MAX_OUTPUT_CHARS)
    if not isinstance(raw, dict):
        return raw
    out = dict(raw)
    # Nested Content.content string (ListDir / ReadFile / Bash shapes)
    content = out.get("Content")
    if isinstance(content, dict) and isinstance(content.get("content"), str):
        c = dict(content)
        c["content"] = truncate_str(c["content"], MAX_OUTPUT_CHARS)
        out["Content"] = c
    if isinstance(out.get("content"), str):
        out["content"] = truncate_str(out["content"], MAX_OUTPUT_CHARS)
    # output_for_prompt / FileContent blobs sometimes appear as nested JSON strings
    for key in ("output", "text", "raw_output"):
        if isinstance(out.get(key), str):
            out[key] = truncate_str(out[key], MAX_OUTPUT_CHARS)
    return out


def minimize_row(row: dict, *, truncate: bool) -> dict:
    if not truncate:
        return row
    row = json.loads(json.dumps(row))  # deep copy
    update = row.get("params", {}).get("update", {})
    kind = update.get("sessionUpdate")
    if kind in ("agent_thought_chunk", "agent_message_chunk", "user_message_chunk"):
        content = update.get("content")
        if isinstance(content, dict) and isinstance(content.get("text"), str):
            limit = {
                "agent_thought_chunk": MAX_THOUGHT_CHARS,
                "agent_message_chunk": MAX_MESSAGE_CHARS,
                "user_message_chunk": MAX_MESSAGE_CHARS,
            }[kind]
            content["text"] = truncate_str(content["text"], limit)
    if kind == "tool_call_update" and "rawOutput" in update:
        update["rawOutput"] = truncate_raw_output(update["rawOutput"])
    # Drop bulky in_progress content blocks if present
    if kind == "tool_call_update" and update.get("status") == "in_progress":
        update.pop("content", None)
        update.pop("rawOutput", None)
    return row


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":"), ensure_ascii=False) + "\n")


def extract(
    source: Path,
    out: Path,
    *,
    truncate: bool = True,
) -> dict:
    rows = [json.loads(l) for l in source.read_text().splitlines() if l.strip()]
    turns = split_turns(rows)
    session_id = (
        rows[0].get("params", {}).get("sessionId")
        if rows
        else "unknown-session"
    )
    # Prefer encoded cwd from path if present
    cwd_enc = "%2FUsers%2Fmaxglassie%2Fprojects%2Fgrok-build"
    for part in source.parts:
        if part.startswith("%2F"):
            cwd_enc = part
            break

    written = []
    for idx, slug in SEED_TURNS.items():
        if idx >= len(turns):
            print(f"warn: turn {idx} missing (only {len(turns)} turns)", file=sys.stderr)
            continue
        turn = [minimize_row(r, truncate=truncate) for r in turns[idx]]
        path = out / f"{slug}.jsonl"
        write_jsonl(path, turn)
        written.append(path)
        rel = path if not str(path).startswith(str(ROOT)) else path.relative_to(ROOT)
        print(
            f"wrote {rel} "
            f"({len(turn)} lines, user={user_text(turns[idx])[:60]!r})"
        )

    # seed_tree: layout the watcher/container expects
    seed_rows: list[dict] = []
    for idx in sorted(SEED_TURNS):
        if idx < len(turns):
            seed_rows.extend(minimize_row(r, truncate=truncate) for r in turns[idx])
    tree_path = out / "seed_tree" / cwd_enc / session_id / "updates.jsonl"
    write_jsonl(tree_path, seed_rows)
    rel_tree = (
        tree_path
        if not str(tree_path).startswith(str(ROOT))
        else tree_path.relative_to(ROOT)
    )
    print(f"wrote {rel_tree} ({len(seed_rows)} lines)")

    # Also drop noise siblings so accidental recursive walks don't ingest them
    sess_dir = tree_path.parent
    (sess_dir / "chat_history.jsonl").write_text(
        '{"type":"system","content":"noise — must not be ingested"}\n',
        encoding="utf-8",
    )

    prov = out / "SESSION_SEED.md"
    prov.write_text(
        f"""# Grok session seed — this OpenStory integration conversation

**Source session** (live Grok Build on Max's machine):

```
{source}
```

- **session_id:** `{session_id}`
- **cwd (encoded):** `{cwd_enc}`
- **extracted:** 2026-07-16 (OpenStory Grok support implementation thread)
- **turns in source:** {len(turns)}
- **truncation:** {"on (tool/text caps for golden size)" if truncate else "off (full wire)"}

## Seed files

| File | Source turn | Prompt (abbrev) |
|------|-------------|-----------------|
"""
        + "\n".join(
            f"| `{SEED_TURNS[i]}.jsonl` | turn {i} | {user_text(turns[i])[:70]!r} |"
            for i in sorted(SEED_TURNS)
            if i < len(turns)
        )
        + f"""

## Container / watcher tree

```
fixtures/grok/seed_tree/
  {cwd_enc}/
    {session_id}/
      updates.jsonl          ← concatenated seed turns
      chat_history.jsonl     ← noise (must be ignored by watcher filter)
```

Mount `seed_tree` as the Grok watch root (same shape as `~/.grok/sessions`).

Regenerate:

```
python3 scripts/extract_grok_session_seed.py
python3 scripts/extract_grok_session_seed.py --no-truncate   # full outputs
```
""",
        encoding="utf-8",
    )
    rel_prov = prov if not str(prov).startswith(str(ROOT)) else prov.relative_to(ROOT)
    print(f"wrote {rel_prov}")
    return {"session_id": session_id, "turns": len(turns), "files": written}


def self_test() -> None:
    import tempfile

    # Minimal fake session with 2 turns
    fake = [
        {
            "timestamp": 1,
            "method": "session/update",
            "params": {
                "sessionId": "019f6cb5-f7e4-7bc1-bb25-aaaaaaaaaaaa",
                "update": {
                    "sessionUpdate": "user_message_chunk",
                    "content": {"type": "text", "text": "hi"},
                },
                "_meta": {"eventId": "a1"},
            },
        },
        {
            "timestamp": 2,
            "method": "_x.ai/session/update",
            "params": {
                "sessionId": "019f6cb5-f7e4-7bc1-bb25-aaaaaaaaaaaa",
                "update": {
                    "sessionUpdate": "turn_completed",
                    "prompt_id": "p",
                    "stop_reason": "end_turn",
                },
                "_meta": {"eventId": "a2"},
            },
        },
    ]
    # only turn 0 exists; SEED_TURNS includes 1 — should warn but not crash
    with tempfile.TemporaryDirectory() as td:
        src = Path(td) / "updates.jsonl"
        write_jsonl(src, fake + fake)  # 2 turns
        out = Path(td) / "out"
        # temporarily only export turn 0 via monkeypatch-like override
        global SEED_TURNS
        old = SEED_TURNS
        SEED_TURNS = {0: "real_turn_00_hi"}
        try:
            extract(src, out, truncate=True)
            assert (out / "real_turn_00_hi.jsonl").exists()
            assert list((out / "seed_tree").rglob("updates.jsonl"))
        finally:
            SEED_TURNS = old
    print("extract_grok_session_seed.py --test: ok")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, default=DEFAULT_SOURCE)
    p.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p.add_argument("--no-truncate", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args(argv)
    if args.test:
        self_test()
        return 0
    if not args.source.exists():
        print(f"source not found: {args.source}", file=sys.stderr)
        print("Pass --source PATH to a Grok updates.jsonl", file=sys.stderr)
        return 1
    extract(args.source, args.out, truncate=not args.no_truncate)
    return 0


if __name__ == "__main__":
    sys.exit(main())
