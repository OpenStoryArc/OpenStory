"""Analyze how plan events appear in transcript data.

Questions:
- Where do ExitPlanMode events live? Main transcript, subagent, hooks?
- What message types carry plan data?
- Why do some sessions have plans and others don't?

Usage:
    uv run python scripts/analyze_plan_events.py
    uv run python scripts/analyze_plan_events.py --test
"""

import json
import os
import sys
import glob
from pathlib import Path
from collections import Counter, defaultdict


def find_project_dir():
    """Find the Claude projects directory for open-story."""
    home = Path.home()
    # Look for any open-story/open-story project directory
    claude_projects = home / ".claude/projects"
    candidates = []
    if claude_projects.exists():
        for d in sorted(claude_projects.iterdir()):
            if d.is_dir() and ("open-story" in d.name or "open-story" in d.name):
                candidates.append(d)
    for c in candidates:
        if c.exists():
            return c
    return None


def scan_transcript(path: Path) -> dict:
    """Scan a JSONL transcript for plan-related events.

    Returns dict with:
        exit_plan_tool_use: list of ExitPlanMode tool_use blocks
        enter_plan_tool_use: list of EnterPlanMode tool_use blocks
        progress_plan_mentions: count of progress events mentioning PlanMode
        tool_result_plan_mentions: count of tool_results mentioning PlanMode
        total_lines: total line count
        message_types: Counter of message types
    """
    result = {
        "path": str(path),
        "exit_plan_tool_use": [],
        "enter_plan_tool_use": [],
        "progress_plan_mentions": 0,
        "tool_result_plan_mentions": 0,
        "total_lines": 0,
        "message_types": Counter(),
    }

    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            for line in f:
                result["total_lines"] += 1
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue

                msg_type = obj.get("type", "unknown")
                result["message_types"][msg_type] += 1

                # Check for tool_use blocks in message content
                msg = obj.get("message", {})
                content = msg.get("content", [])
                if isinstance(content, list):
                    for block in content:
                        if not isinstance(block, dict):
                            continue
                        if block.get("type") == "tool_use":
                            name = block.get("name", "")
                            if name == "ExitPlanMode":
                                plan_text = block.get("input", {}).get("plan", "")
                                title = plan_text.split("\n")[0].strip().lstrip("# ") if plan_text else ""
                                result["exit_plan_tool_use"].append({
                                    "title": title[:80],
                                    "plan_length": len(plan_text),
                                    "msg_type": msg_type,
                                    "role": msg.get("role", "?"),
                                })
                            elif name == "EnterPlanMode":
                                result["enter_plan_tool_use"].append({
                                    "msg_type": msg_type,
                                    "role": msg.get("role", "?"),
                                })

                # Check progress events for plan mentions
                if msg_type == "progress":
                    raw = json.dumps(obj)
                    if "PlanMode" in raw or "plan_mode" in raw:
                        result["progress_plan_mentions"] += 1

                # Check tool results for plan mentions
                if msg_type == "result" or (msg.get("role") == "user" and isinstance(content, list)):
                    raw = json.dumps(content)
                    if "ExitPlanMode" in raw and any(
                        isinstance(b, dict) and b.get("type") == "tool_result"
                        for b in (content if isinstance(content, list) else [])
                    ):
                        result["tool_result_plan_mentions"] += 1
    except Exception as e:
        result["error"] = str(e)

    return result


def analyze_session(session_dir: Path, session_id: str) -> dict:
    """Analyze a session and all its subagents for plan data."""
    result = {
        "session_id": session_id,
        "main_transcript": None,
        "subagent_transcripts": [],
    }

    # Main transcript
    main_jsonl = session_dir.parent / f"{session_id}.jsonl"
    if main_jsonl.exists():
        result["main_transcript"] = scan_transcript(main_jsonl)

    # Subagent transcripts
    subagents_dir = session_dir / "subagents"
    if subagents_dir.exists():
        for f in sorted(subagents_dir.glob("*.jsonl")):
            scan = scan_transcript(f)
            if (scan["exit_plan_tool_use"] or scan["enter_plan_tool_use"] or
                scan["progress_plan_mentions"] > 0):
                result["subagent_transcripts"].append(scan)

    return result


def main():
    project_dir = find_project_dir()
    if not project_dir:
        print("Could not find Claude projects directory")
        sys.exit(1)

    print(f"Scanning: {project_dir}")
    print()

    # Find all session directories
    sessions_with_plans = []
    sessions_without_plans = []

    total_exit_plan_main = 0
    total_exit_plan_subagent = 0
    total_progress_mentions = 0

    session_dirs = sorted(project_dir.iterdir())
    session_ids = set()
    for item in session_dirs:
        if item.is_dir() and not item.name.startswith("."):
            session_ids.add(item.name)

    for sid in sorted(session_ids):
        session_dir = project_dir / sid
        result = analyze_session(session_dir, sid)

        has_plans = False

        # Check main transcript
        if result["main_transcript"]:
            mt = result["main_transcript"]
            if mt["exit_plan_tool_use"]:
                has_plans = True
                total_exit_plan_main += len(mt["exit_plan_tool_use"])

        # Check subagents
        for sa in result["subagent_transcripts"]:
            if sa["exit_plan_tool_use"]:
                has_plans = True
                total_exit_plan_subagent += len(sa["exit_plan_tool_use"])
            total_progress_mentions += sa["progress_plan_mentions"]

        if has_plans:
            sessions_with_plans.append(result)
        # Skip sessions without plans for brevity

    # Report
    print("=" * 70)
    print("PLAN EVENT ANALYSIS")
    print("=" * 70)
    print(f"Total sessions scanned: {len(session_ids)}")
    print(f"Sessions with ExitPlanMode tool_use: {len(sessions_with_plans)}")
    print(f"ExitPlanMode in main transcripts: {total_exit_plan_main}")
    print(f"ExitPlanMode in subagent transcripts: {total_exit_plan_subagent}")
    print(f"Progress events mentioning PlanMode: {total_progress_mentions}")
    print()

    print("=" * 70)
    print("SESSIONS WITH PLAN EVENTS")
    print("=" * 70)
    for result in sessions_with_plans:
        sid = result["session_id"]
        print(f"\n  {sid}")

        mt = result["main_transcript"]
        if mt and mt["exit_plan_tool_use"]:
            print(f"    Main transcript: {len(mt['exit_plan_tool_use'])} ExitPlanMode")
            for ep in mt["exit_plan_tool_use"]:
                print(f"      [{ep['msg_type']}/{ep['role']}] {ep['title']}")

        for sa in result["subagent_transcripts"]:
            agent_file = Path(sa["path"]).name
            if sa["exit_plan_tool_use"]:
                print(f"    {agent_file}: {len(sa['exit_plan_tool_use'])} ExitPlanMode")
                for ep in sa["exit_plan_tool_use"]:
                    print(f"      [{ep['msg_type']}/{ep['role']}] {ep['title']}")
            if sa["progress_plan_mentions"]:
                print(f"    {agent_file}: {sa['progress_plan_mentions']} progress mentions")

    print()
    print("=" * 70)
    print("KEY INSIGHT")
    print("=" * 70)
    print(f"Main transcript plans:    {total_exit_plan_main} (detectable by current is_plan_event)")
    print(f"Subagent transcript plans: {total_exit_plan_subagent} (need subagent -> parent mapping)")
    print(f"Progress-only mentions:   {total_progress_mentions} (not actual tool_use events)")


def test():
    """Self-tests."""
    import tempfile

    # Test 1: scan_transcript finds ExitPlanMode tool_use
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False, encoding="utf-8") as f:
        f.write(json.dumps({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "name": "ExitPlanMode", "input": {"plan": "# My Plan\nDo stuff"}}
                ]
            }
        }) + "\n")
        f.write(json.dumps({
            "type": "assistant",
            "message": {
                "role": "assistant",
                "content": [
                    {"type": "tool_use", "name": "Read", "input": {"file_path": "/foo"}}
                ]
            }
        }) + "\n")
        path = f.name

    result = scan_transcript(Path(path))
    assert len(result["exit_plan_tool_use"]) == 1, f"expected 1, got {len(result['exit_plan_tool_use'])}"
    assert result["exit_plan_tool_use"][0]["title"] == "My Plan"
    assert result["total_lines"] == 2
    os.unlink(path)

    # Test 2: progress events counted separately
    with tempfile.NamedTemporaryFile(mode="w", suffix=".jsonl", delete=False, encoding="utf-8") as f:
        f.write(json.dumps({
            "type": "progress",
            "data": {"message": "ExitPlanMode completed"},
        }) + "\n")
        path = f.name

    result = scan_transcript(Path(path))
    assert len(result["exit_plan_tool_use"]) == 0
    assert result["progress_plan_mentions"] == 1
    os.unlink(path)

    print("All tests passed.")


if __name__ == "__main__":
    if "--test" in sys.argv:
        test()
    else:
        main()
