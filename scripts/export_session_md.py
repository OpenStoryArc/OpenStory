#!/usr/bin/env python3
"""Export a Claude Code session transcript to readable markdown.

Reads a JSONL transcript from `~/.claude/projects/<project-dir>/<session-id>.jsonl`
and emits a markdown file with user prompts as `>` blockquotes and assistant
text as prose. Tool calls are folded to a single line each by default
(`--with-tools` to expand them).

Run:
    python3 scripts/export_session_md.py <session-id-prefix>
    python3 scripts/export_session_md.py f0a0601d --out captures/yc-origin.md
    python3 scripts/export_session_md.py f0a0601d --with-tools
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

CLAUDE_PROJECTS = Path.home() / ".claude" / "projects"


def find_transcript(prefix: str) -> Path | None:
    for proj in CLAUDE_PROJECTS.iterdir():
        if not proj.is_dir():
            continue
        for f in proj.rglob(f"{prefix}*.jsonl"):
            if "subagents" not in f.parts:
                return f
    return None


def render_text_blocks(content) -> str:
    if isinstance(content, str):
        return content
    if not isinstance(content, list):
        return ""
    out = []
    for blk in content:
        if not isinstance(blk, dict):
            continue
        t = blk.get("type")
        if t == "text":
            out.append(blk.get("text", ""))
        elif t == "thinking":
            continue  # don't emit thinking
    return "\n\n".join(s for s in out if s)


def render_tool_calls(content, with_tools: bool) -> list[str]:
    if not isinstance(content, list):
        return []
    lines = []
    for blk in content:
        if not isinstance(blk, dict):
            continue
        if blk.get("type") != "tool_use":
            continue
        name = blk.get("name", "?")
        if with_tools:
            inp = json.dumps(blk.get("input", {}), indent=2)
            lines.append(f"<details><summary>tool: <code>{name}</code></summary>\n\n```json\n{inp}\n```\n\n</details>")
        else:
            inp_summary = ""
            inp = blk.get("input", {})
            if isinstance(inp, dict):
                if "command" in inp:
                    inp_summary = f" — `{(inp.get('command') or '')[:80]}`"
                elif "file_path" in inp:
                    inp_summary = f" — `{inp.get('file_path','')}`"
                elif "query" in inp:
                    inp_summary = f" — `{(inp.get('query') or '')[:80]}`"
                elif "session_id" in inp:
                    inp_summary = f" — session `{inp.get('session_id','')[:8]}`"
            lines.append(f"*[tool: {name}{inp_summary}]*")
    return lines


def is_system_pseudo_user(text: str) -> bool:
    if not text:
        return True
    if text.startswith("<system-reminder>") or text.startswith("<command-name>"):
        return True
    if text.startswith("Caveat:"):
        return True
    if text.startswith("[Request interrupted"):
        return True
    return False


def export(jsonl: Path, out: Path, with_tools: bool) -> int:
    out.parent.mkdir(parents=True, exist_ok=True)
    turns = []
    first_ts = last_ts = None
    user_count = assistant_count = 0

    with jsonl.open("r", encoding="utf-8") as fh:
        for line in fh:
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            t = rec.get("type")
            ts = rec.get("timestamp", "")
            if ts:
                if first_ts is None:
                    first_ts = ts
                last_ts = ts
            if t == "user":
                msg = rec.get("message", {})
                text = render_text_blocks(msg.get("content"))
                if is_system_pseudo_user(text):
                    continue
                if not text.strip():
                    continue
                # Strip tool_result blocks if present
                stripped = []
                for line2 in text.split("\n"):
                    if line2.startswith("[{") or line2.startswith('{"tool_use_id"'):
                        continue
                    stripped.append(line2)
                cleaned = "\n".join(stripped).strip()
                if not cleaned:
                    continue
                turns.append(("user", ts, cleaned))
                user_count += 1
            elif t == "assistant":
                msg = rec.get("message", {})
                text = render_text_blocks(msg.get("content"))
                tool_lines = render_tool_calls(msg.get("content"), with_tools)
                if not text.strip() and not tool_lines:
                    continue
                turns.append(("assistant", ts, text.strip(), tool_lines))
                assistant_count += 1

    # Render
    title = jsonl.stem[:8]
    lines = []
    lines.append(f"# Session `{title}`")
    lines.append("")
    if first_ts and last_ts:
        lines.append(f"_Recorded {first_ts[:19]} → {last_ts[:19]}_")
        lines.append("")
    lines.append(f"_{user_count} user prompts · {assistant_count} assistant turns · transcript: `{jsonl}`_")
    lines.append("")
    lines.append("---")
    lines.append("")

    for turn in turns:
        role = turn[0]
        ts = turn[1][:19] if len(turn) > 1 else ""
        if role == "user":
            text = turn[2]
            lines.append(f"## 👤 User · {ts}")
            lines.append("")
            for ln in text.split("\n"):
                lines.append(f"> {ln}" if ln else ">")
            lines.append("")
        else:
            text = turn[2]
            tool_lines = turn[3] if len(turn) > 3 else []
            lines.append(f"## 🤖 Assistant · {ts}")
            lines.append("")
            if text:
                lines.append(text)
                lines.append("")
            for tl in tool_lines:
                lines.append(tl)
                lines.append("")

    out.write_text("\n".join(lines), encoding="utf-8")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("session", help="session id (prefix ok)")
    ap.add_argument("--out", help="output markdown path", default=None)
    ap.add_argument("--with-tools", action="store_true",
                    help="expand tool calls with full JSON input")
    args = ap.parse_args()

    p = find_transcript(args.session)
    if p is None:
        print(f"Session transcript not found for prefix: {args.session}",
              file=sys.stderr)
        return 1

    out = Path(args.out) if args.out else Path(f"captures/session-{args.session[:8]}.md")
    return export(p, out, args.with_tools)


if __name__ == "__main__":
    sys.exit(main())
