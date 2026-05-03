#!/usr/bin/env python3
"""Step 3 — pull deep facts: files_touched, opening transcript, errors, MCP calls.

Methodology:
  For each PRIMARY session (we skip subagents and compactions here — they
  enrich quickly enough but rarely carry meaning on their own; if needed,
  re-run with --include-subagents):

  - files_touched: from /api/sessions/{id}/activity. Authoritative for
    create/modify counts and for the author fallback when project_id is
    missing.
  - opening_prompt: try to read the literal first user message from
    /api/sessions/{id}/transcript. The 'label' on the synopsis is a derived
    short string that sometimes reflects a sub-prompt rather than the opening
    turn — never quote the label as if it were the user's words.
  - errors: full /api/sessions/{id}/errors list, not just the count.
  - mcp_openstory_calls: re-derived from /activity's tool_breakdown so we
    catch every endpoint name (gather only stores top_tools). This is the
    dogfooding signal — surface it.
  - shared_files: deferred to measure (cross-session).

If transcript is unavailable (endpoint returns empty), we record opening_prompt
as null and leave the synopsis label untouched. Do not invent prompts.

Input: classify.py output. Output: same with `enriched` per session.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import (  # noqa: E402
    DEFAULT_URL,
    author_for,
    fetch,
    load_roster,
    read_json,
    write_json,
)


def first_user_prompt(transcript: dict) -> str | None:
    """Extract the first user-authored message text from a transcript payload."""
    entries = transcript.get("entries") if isinstance(transcript, dict) else None
    if not entries:
        return None
    for entry in entries:
        # Different shapes possible. Look for user role / record_type.
        role = entry.get("role") or entry.get("record_type") or ""
        if "user" in role.lower():
            text = entry.get("text") or entry.get("content") or entry.get("snippet")
            if isinstance(text, list):  # content blocks
                for block in text:
                    if isinstance(block, dict) and block.get("type") == "text":
                        return block.get("text")
            if isinstance(text, str) and text.strip():
                return text
    return None


def count_mcp_calls(tool_breakdown: dict) -> dict:
    """Sum mcp__openstory__* calls and return per-endpoint detail."""
    detail = {t: c for t, c in tool_breakdown.items() if t.startswith("mcp__openstory")}
    return {"total": sum(detail.values()), "by_endpoint": detail}


def enrich_session(session: dict, base_url: str, roster: dict) -> dict:
    sid = session["session_id"]
    role = session.get("tags", {}).get("role", "primary")

    enriched: dict = {
        "files_touched": [],
        "opening_prompt": None,
        "errors": [],
        "mcp_openstory": {"total": 0, "by_endpoint": {}},
        "skipped": False,
    }

    # Skip deep enrichment for compactions (they're context summaries).
    if role == "compaction":
        enriched["skipped"] = True
        return {**session, "enriched": enriched}

    activity = fetch(f"/api/sessions/{sid}/activity", base_url)
    enriched["files_touched"] = activity.get("files_touched") or []
    tool_breakdown = activity.get("tool_breakdown") or {}
    enriched["mcp_openstory"] = count_mcp_calls(tool_breakdown)
    enriched["activity_first_prompt"] = activity.get("first_prompt")

    # Reconcile author if it was unknown.
    if session.get("tags", {}).get("author") == "unknown":
        new_author = author_for(session.get("project_id"), enriched["files_touched"], roster)
        if new_author != "unknown":
            session["tags"]["author"] = new_author
            session["tags"]["author_resolved_via"] = "files_touched"

    # Errors with timestamps and (often empty) messages.
    enriched["errors"] = fetch(f"/api/sessions/{sid}/errors", base_url) or []

    # Opening prompt — best effort.
    transcript = fetch(f"/api/sessions/{sid}/transcript", base_url)
    enriched["opening_prompt"] = first_user_prompt(transcript)
    enriched["transcript_available"] = bool(
        isinstance(transcript, dict) and transcript.get("entries")
    )

    return {**session, "enriched": enriched}


def enrich(bundle: dict, base_url: str, include_subagents: bool) -> dict:
    out_sessions = []
    for s in bundle["sessions"]:
        role = s.get("tags", {}).get("role")
        if role == "subagent" and not include_subagents:
            out_sessions.append({**s, "enriched": {"skipped": True, "reason": "subagent"}})
            continue
        out_sessions.append(enrich_session(s, base_url, load_roster()))
    return {**bundle, "sessions": out_sessions}


def main() -> None:
    parser = argparse.ArgumentParser(description="Enrich sessions with files, transcript, errors.")
    parser.add_argument("--in", dest="input", default="-")
    parser.add_argument("--out", dest="output", default="-")
    parser.add_argument("--url", default=DEFAULT_URL)
    parser.add_argument(
        "--include-subagents",
        action="store_true",
        help="Also enrich agent-* subagent sessions (slower, more API calls).",
    )
    args = parser.parse_args()
    bundle = read_json(args.input)
    write_json(enrich(bundle, args.url, args.include_subagents), args.output)


if __name__ == "__main__":
    main()
