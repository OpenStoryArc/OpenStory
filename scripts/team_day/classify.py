#!/usr/bin/env python3
"""Step 2 — tag every session with author, role, kind, is_team_repo.

Methodology:
  - author: resolved from project_id path against roster. Path is identity.
    Fallback to files_touched if project_id is missing — but we don't have
    files yet, so unknowns at this stage stay unknown until enrich runs.
  - role: derived from session_id prefix.
      "agent-acompact-*" → compaction (these are context-loss summaries, skip
                            from headcount; preserve for health metrics)
      "agent-a*"         → sub-agent (folds into a parent primary session)
      else               → primary
  - kind: derived from top_tools shape. Best-effort categorization to help the
    narrator decide what beat to write:
      "recall"   → ≥3 mcp__openstory__* calls AND no Edit/Write
      "ship"    → Edit or Write in top_tools
      "explore" → WebFetch dominant, no Edit/Write
      "chat"    → tool_count == 0
      "work"    → fallback for everything else
  - is_team_repo: project name (last segment of project_id slug) in roster's
    team_repos list. Off-team sessions stay in the report but get tagged so
    the narrator can de-emphasize them.

We do not drop rows here. Filtering is the report-composer's job.

Input: gather.py output. Output: same shape with `tags` per session.
"""

from __future__ import annotations

import argparse
import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import author_for, load_roster, read_json, write_json  # noqa: E402


def role_of(session_id: str) -> str:
    if session_id.startswith("agent-acompact-"):
        return "compaction"
    if session_id.startswith("agent-a"):
        return "subagent"
    return "primary"


def kind_of(top_tools: list, tool_count: int) -> str:
    if tool_count == 0:
        return "chat"
    tools = {t["tool"]: t["count"] for t in top_tools if isinstance(t, dict)}
    has_edit = any(t in tools for t in ("Edit", "Write", "MultiEdit", "NotebookEdit"))
    mcp_openstory = sum(c for t, c in tools.items() if t.startswith("mcp__openstory"))
    if has_edit:
        return "ship"
    if mcp_openstory >= 3:
        return "recall"
    if tools.get("WebFetch", 0) >= 2:
        return "explore"
    return "work"


def project_repo_name(project_id: str | None) -> str:
    """Repo name from a project_id slug.

    The slug encodes a path with '-' replacing '/'. Repo names themselves can
    contain hyphens ("yc-app", "telegram-int-local"), so naive rsplit is wrong.
    Strategy: cut after the last "workspace" or "projects" segment.

    Examples:
      -Users-kloughra-workspace-telegram-int-local -> telegram-int-local
      -Users-maxglassie-projects-yc-app            -> yc-app
      -Users-maxglassie-projects-OpenStory         -> OpenStory
    """
    if not project_id:
        return ""
    pid = project_id.strip("-")
    parts = pid.split("-")
    for marker in ("workspace", "projects"):
        if marker in parts:
            idx = len(parts) - 1 - parts[::-1].index(marker)
            tail = parts[idx + 1 :]
            if tail:
                return "-".join(tail)
    return parts[-1] if parts else ""


def classify(bundle: dict, roster: dict) -> dict:
    team_repos = set(roster.get("team_repos", []))
    out_sessions = []
    for s in bundle["sessions"]:
        repo = project_repo_name(s.get("project_id"))
        tags = {
            "author": author_for(s.get("project_id"), None, roster, user=s.get("user")),
            "role": role_of(s.get("session_id") or ""),
            "kind": kind_of(s.get("top_tools") or [], s.get("tool_count") or 0),
            "is_team_repo": repo in team_repos,
            "repo": repo,
        }
        out_sessions.append({**s, "tags": tags})
    return {**bundle, "sessions": out_sessions}


def main() -> None:
    parser = argparse.ArgumentParser(description="Tag sessions with author/role/kind.")
    parser.add_argument("--in", dest="input", default="-")
    parser.add_argument("--out", dest="output", default="-")
    args = parser.parse_args()
    bundle = read_json(args.input)
    write_json(classify(bundle, load_roster()), args.output)


if __name__ == "__main__":
    main()
