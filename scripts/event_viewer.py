"""Live event viewer -- pretty-print the unified event log as it streams.

Reads data/events.jsonl and displays events with color-coded formatting.
In --follow mode, watches for new lines and prints them live.

Usage:
    uv run python scripts/event_viewer.py                    # last 20 (header + key fields)
    uv run python scripts/event_viewer.py --follow           # live stream
    uv run python scripts/event_viewer.py --follow --compact # live stream (one-liners)
    uv run python scripts/event_viewer.py --follow --full    # live stream (full JSON)
    uv run python scripts/event_viewer.py --last 50          # last 50 events
    uv run python scripts/event_viewer.py --filter tool_use  # filter by subtype
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Optional


# ── ANSI colors ──

class C:
    """ANSI escape codes for terminal coloring."""
    RESET   = "\033[0m"
    DIM     = "\033[2m"
    BOLD    = "\033[1m"
    # Foreground
    RED     = "\033[31m"
    GREEN   = "\033[32m"
    YELLOW  = "\033[33m"
    BLUE    = "\033[34m"
    MAGENTA = "\033[35m"
    CYAN    = "\033[36m"
    WHITE   = "\033[37m"
    GRAY    = "\033[90m"


# Subtype -> color mapping
SUBTYPE_COLORS = {
    "message.user.prompt":        C.GREEN,
    "message.user.tool_result":   C.CYAN,
    "message.assistant.text":     C.BLUE,
    "message.assistant.tool_use": C.YELLOW,
    "message.assistant.thinking": C.MAGENTA,
    "system.turn.complete":       C.DIM,
    "system.error":               C.RED,
    "system.compact":             C.DIM,
    "system.hook":                C.DIM,
    "progress.bash":              C.GRAY,
    "progress.agent":             C.GRAY,
    "progress.hook":              C.GRAY,
    "file.snapshot":              C.DIM,
    "queue.enqueue":              C.DIM,
    "queue.dequeue":              C.DIM,
}


def color_for_subtype(subtype: Optional[str]) -> str:
    if subtype is None:
        return C.WHITE
    if subtype in SUBTYPE_COLORS:
        return SUBTYPE_COLORS[subtype]
    # Match by prefix
    for prefix in ("progress.", "system.", "message.assistant", "message.user"):
        if subtype.startswith(prefix):
            for key, color in SUBTYPE_COLORS.items():
                if key.startswith(prefix):
                    return color
    return C.WHITE


def extract_summary(event: dict) -> str:
    """Extract a one-line summary from the event data."""
    data = event.get("data", {})
    subtype = event.get("subtype", "")

    if subtype == "message.user.prompt":
        raw = data.get("raw", {})
        msg = raw.get("message", {})
        content = msg.get("content", [])
        for block in content:
            if isinstance(block, dict) and block.get("type") == "text":
                text = block.get("text", "")
                return _truncate(text, 100)
            if isinstance(block, str):
                return _truncate(block, 100)
        return ""

    if subtype == "message.assistant.text":
        text = data.get("text", "")
        if text:
            return _truncate(text, 100)
        # Try raw content
        raw = data.get("raw", {})
        msg = raw.get("message", {})
        for block in msg.get("content", []):
            if isinstance(block, dict) and block.get("type") == "text":
                return _truncate(block.get("text", ""), 100)
        return ""

    if subtype == "message.assistant.tool_use":
        tool = data.get("tool", "")
        args = data.get("args", {})
        if isinstance(args, dict):
            cmd = args.get("command", "")
            if cmd:
                return f"{tool}: {_truncate(cmd, 80)}"
            file_path = args.get("file_path", "")
            if file_path:
                return f"{tool}: {file_path}"
            pattern = args.get("pattern", "")
            if pattern:
                return f"{tool}: {_truncate(pattern, 60)}"
        return tool

    if subtype == "message.user.tool_result":
        raw = data.get("raw", {})
        msg = raw.get("message", {})
        for block in msg.get("content", []):
            if isinstance(block, dict) and block.get("type") == "tool_result":
                content = block.get("content", "")
                if isinstance(content, str):
                    lines = content.strip().split("\n")
                    if len(lines) > 1:
                        return f"({len(lines)} lines) {_truncate(lines[0], 60)}"
                    return _truncate(content, 80)
        return ""

    if subtype and subtype.startswith("progress."):
        raw = data.get("raw", {})
        content = raw.get("content", "")
        if isinstance(content, str) and content:
            return _truncate(content.strip(), 80)
        return ""

    if subtype == "message.assistant.thinking":
        return "(thinking)"

    if subtype and subtype.startswith("system."):
        return data.get("raw", {}).get("type", "")

    return ""


def _truncate(text: str, max_len: int) -> str:
    text = text.replace("\n", " ").replace("\r", "").strip()
    if len(text) <= max_len:
        return text
    return text[:max_len - 3] + "..."


def _format_header(event: dict) -> str:
    """One-line colored header: TIME SESSION SUBTYPE."""
    timestamp = event.get("time", "")
    if "T" in timestamp:
        time_part = timestamp.split("T")[1]
        if "." in time_part:
            time_part = time_part.split(".")[0]
        if time_part.endswith("Z"):
            time_part = time_part[:-1]
        timestamp = time_part

    subtype = event.get("subtype", event.get("type", "?"))
    session_id = event.get("data", {}).get("sessionId", "")
    if session_id:
        session_id = session_id[:8]

    color = color_for_subtype(event.get("subtype"))

    parts = [
        f"{C.GRAY}{timestamp}{C.RESET}",
        f"{C.DIM}{session_id}{C.RESET}" if session_id else "",
        f"{color}{C.BOLD}{subtype}{C.RESET}",
    ]
    return " ".join(p for p in parts if p)


def _colorize_json(text: str) -> str:
    """Add ANSI colors to pretty-printed JSON."""
    lines = []
    for line in text.split("\n"):
        stripped = line.lstrip()
        if stripped.startswith('"') and '": ' in stripped:
            # Key-value line: color the key
            colon_pos = line.index('": ')
            key_part = line[:colon_pos + 1]
            value_part = line[colon_pos + 1:]
            lines.append(f"{C.CYAN}{key_part}{C.RESET}{value_part}")
        else:
            lines.append(f"{C.DIM}{line}{C.RESET}")
    return "\n".join(lines)


def _format_summary_fields(event: dict) -> list[str]:
    """Extract the interesting fields from an event, skip noise."""
    data = event.get("data", {})
    subtype = event.get("subtype", "")
    lines = []

    # Show event id (short)
    eid = event.get("id", "")
    if eid:
        lines.append(f"  {C.CYAN}id{C.RESET} {eid[:12]}")

    # Subtype-specific fields
    if "tool_use" in subtype:
        tool = data.get("tool", "")
        args = data.get("args", {})
        if tool:
            lines.append(f"  {C.CYAN}tool{C.RESET} {tool}")
        if isinstance(args, dict):
            for key in ("command", "file_path", "pattern", "query", "prompt"):
                val = args.get(key)
                if val:
                    lines.append(f"  {C.CYAN}{key}{C.RESET} {_truncate(str(val), 120)}")

    elif "tool_result" in subtype:
        raw = data.get("raw", {})
        msg = raw.get("message", {})
        for block in msg.get("content", []):
            if isinstance(block, dict) and block.get("type") == "tool_result":
                content = block.get("content", "")
                if isinstance(content, str):
                    content_lines = content.strip().split("\n")
                    n = len(content_lines)
                    preview = _truncate(content_lines[0], 100)
                    lines.append(f"  {C.CYAN}output{C.RESET} ({n} lines) {preview}")

    elif "user.prompt" in subtype:
        raw = data.get("raw", {})
        msg = raw.get("message", {})
        for block in msg.get("content", []):
            if isinstance(block, dict) and block.get("type") == "text":
                lines.append(f"  {C.CYAN}text{C.RESET} {_truncate(block.get('text', ''), 120)}")
            elif isinstance(block, str):
                lines.append(f"  {C.CYAN}text{C.RESET} {_truncate(block, 120)}")

    elif "assistant.text" in subtype:
        text = data.get("text", "")
        if text:
            lines.append(f"  {C.CYAN}text{C.RESET} {_truncate(text, 120)}")

    elif "thinking" in subtype:
        lines.append(f"  {C.DIM}(thinking){C.RESET}")

    elif subtype.startswith("progress."):
        raw = data.get("raw", {})
        content = raw.get("content", "")
        if isinstance(content, str) and content.strip():
            lines.append(f"  {C.CYAN}content{C.RESET} {_truncate(content.strip(), 120)}")

    elif "error" in subtype:
        raw = data.get("raw", {})
        err = raw.get("error", raw.get("message", ""))
        if err:
            lines.append(f"  {C.RED}error{C.RESET} {_truncate(str(err), 120)}")

    # Agent identity (Story 037 fields)
    agent_id = data.get("agentId")
    if agent_id:
        lines.append(f"  {C.MAGENTA}agentId{C.RESET} {agent_id}")
    is_sidechain = data.get("isSidechain")
    if is_sidechain:
        lines.append(f"  {C.MAGENTA}sidechain{C.RESET} true")

    return lines


def format_event(event: dict, mode: str = "default") -> str:
    """Format a single event for display.

    Modes:
      default:  header + key fields (2-4 lines per event)
      compact:  one-line summary
      full:     header + full pretty-printed JSON
    """
    header = _format_header(event)

    if mode == "compact":
        summary = extract_summary(event)
        if summary:
            return f"{header} {C.DIM}{summary}{C.RESET}"
        return header

    if mode == "full":
        body = _colorize_json(json.dumps(event, indent=2))
        return f"{header}\n{body}\n"

    # Default: header + summary fields
    fields = _format_summary_fields(event)
    if fields:
        return header + "\n" + "\n".join(fields)
    return header


def read_last_n_lines(path: Path, n: int) -> list[str]:
    """Read the last N lines of a file efficiently."""
    if not path.exists():
        return []
    with open(path, "rb") as f:
        # Seek backwards to find N newlines
        try:
            f.seek(0, 2)  # end of file
            file_size = f.tell()
        except OSError:
            return []

        if file_size == 0:
            return []

        # Read in chunks from the end
        lines: list[bytes] = []
        chunk_size = 8192
        pos = file_size
        leftover = b""

        while pos > 0 and len(lines) < n + 1:
            read_size = min(chunk_size, pos)
            pos -= read_size
            f.seek(pos)
            chunk = f.read(read_size) + leftover
            chunk_lines = chunk.split(b"\n")
            leftover = chunk_lines[0]
            lines = chunk_lines[1:] + lines

        if leftover:
            lines = [leftover] + lines

        # Take last N non-empty lines
        result = []
        for raw_line in lines:
            line = raw_line.decode("utf-8", errors="replace").strip()
            if line:
                result.append(line)

        return result[-n:]


def follow_file(path: Path, filter_pattern: Optional[str], mode: str):
    """Tail -f equivalent: watch for new lines and print them."""
    if not path.exists():
        print(f"{C.YELLOW}Waiting for {path} to be created...{C.RESET}", file=sys.stderr)
        while not path.exists():
            time.sleep(0.5)
        print(f"{C.GREEN}File created. Streaming events...{C.RESET}", file=sys.stderr)

    with open(path, "r", encoding="utf-8", errors="replace") as f:
        # Seek to end
        f.seek(0, 2)
        print(f"{C.DIM}-- following {path.name} (Ctrl+C to stop) --{C.RESET}", file=sys.stderr)

        while True:
            line = f.readline()
            if not line:
                time.sleep(0.1)
                continue

            line = line.strip()
            if not line:
                continue

            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue

            if filter_pattern:
                subtype = event.get("subtype", "")
                if filter_pattern not in (subtype or ""):
                    continue

            print(format_event(event, mode=mode))
            sys.stdout.flush()


def main():
    parser = argparse.ArgumentParser(
        description="Live event viewer for open-story's unified event log"
    )
    parser.add_argument(
        "--follow", "-f", action="store_true",
        help="Follow the log file (like tail -f)"
    )
    parser.add_argument(
        "--last", "-n", type=int, default=20,
        help="Show last N events (default: 20)"
    )
    parser.add_argument(
        "--filter", type=str, default=None,
        help="Filter by subtype substring (e.g., 'tool_use', 'progress')"
    )
    parser.add_argument(
        "--compact", "-c", action="store_true",
        help="One-line summary per event"
    )
    parser.add_argument(
        "--full", action="store_true",
        help="Full pretty-printed JSON per event"
    )
    parser.add_argument(
        "--path", type=str, default=None,
        help="Path to events.jsonl (default: ./data/events.jsonl)"
    )
    args = parser.parse_args()

    # Find the events.jsonl file
    if args.path:
        log_path = Path(args.path)
    else:
        # Try relative to script, then cwd
        candidates = [
            Path("data/events.jsonl"),
            Path(__file__).parent.parent / "data" / "events.jsonl",
        ]
        log_path = next((p for p in candidates if p.exists()), candidates[0])

    if args.follow:
        try:
            mode = "compact" if args.compact else "full" if args.full else "default"
            follow_file(log_path, args.filter, mode)
        except KeyboardInterrupt:
            print(f"\n{C.DIM}-- stopped --{C.RESET}", file=sys.stderr)
            sys.exit(0)
    else:
        if not log_path.exists():
            print(f"No event log found at {log_path}", file=sys.stderr)
            print(f"Start open-story to begin recording events.", file=sys.stderr)
            sys.exit(1)

        lines = read_last_n_lines(log_path, args.last)
        count = 0
        for line in lines:
            try:
                event = json.loads(line)
            except json.JSONDecodeError:
                continue

            if args.filter:
                subtype = event.get("subtype", "")
                if args.filter not in (subtype or ""):
                    continue

            mode = "compact" if args.compact else "full" if args.full else "default"
            print(format_event(event, mode=mode))
            count += 1

        if count == 0 and args.filter:
            print(f"No events matching '{args.filter}'", file=sys.stderr)


if __name__ == "__main__":
    main()
