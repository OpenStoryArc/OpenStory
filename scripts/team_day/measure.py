#!/usr/bin/env python3
"""Step 4 — DORA-flavored numbers across the enriched bundle.

Methodology:
  Numbers, not prose. The compose step decides which to surface.

  Throughput:
    - sessions per author (with primary/subagent/compaction split)
    - commits in the local-day window per author (via `git log --since/--until`)
    - PRs merged in window (via `gh pr list --search merged:..` if available)
    - branches active per author (commits within window grouped by branch)

  Hot files:
    - files touched in ≥2 sessions today (any author)
    - files touched by both authors → flagged as cross-author

  Health:
    - ghost_sessions: sessions with event_count <= 5 AND no files_touched
    - error_sessions: sessions with errors > 0 (including count of empty-message
      errors, which usually mean hook timeouts)
    - compaction_count: agent-acompact-* sessions in window
    - recall_sessions: sessions tagged kind=='recall'
    - tokens: today vs 7d trailing average from /api/daily-token-usage if
      available (best-effort — endpoint may not exist on every deployment)

Input: enrich.py output. Output: a sibling 'metrics' object alongside sessions.
"""

from __future__ import annotations

import argparse
import shutil
import subprocess
import sys
from collections import Counter, defaultdict
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
from _lib import DEFAULT_URL, fetch, read_json, write_json  # noqa: E402


def _git(*args: str, cwd: str | None = None) -> str:
    if not shutil.which("git"):
        return ""
    try:
        return subprocess.check_output(
            ["git", *args],
            cwd=cwd,
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
    except subprocess.CalledProcessError:
        return ""


def commits_in_window(utc_start: str, utc_end: str, cwd: str | None = None) -> list[dict]:
    """Return one row per commit landed in the window (across all branches)."""
    raw = _git(
        "log",
        f"--since={utc_start}",
        f"--until={utc_end}",
        "--all",
        "--no-merges",
        "--pretty=format:%H%x09%aI%x09%an%x09%s",
        cwd=cwd,
    )
    if not raw:
        return []
    rows = []
    for line in raw.splitlines():
        parts = line.split("\t", 3)
        if len(parts) != 4:
            continue
        sha, iso, author, subject = parts
        rows.append({"sha": sha[:8], "iso": iso, "author": author, "subject": subject})
    return rows


def merges_in_window(utc_start: str, utc_end: str, cwd: str | None = None) -> list[dict]:
    raw = _git(
        "log",
        f"--since={utc_start}",
        f"--until={utc_end}",
        "--all",
        "--merges",
        "--pretty=format:%H%x09%aI%x09%an%x09%s",
        cwd=cwd,
    )
    if not raw:
        return []
    rows = []
    for line in raw.splitlines():
        parts = line.split("\t", 3)
        if len(parts) != 4:
            continue
        sha, iso, author, subject = parts
        rows.append({"sha": sha[:8], "iso": iso, "author": author, "subject": subject})
    return rows


def throughput(sessions: list[dict], commits: list[dict], merges: list[dict]) -> dict:
    by_author = defaultdict(lambda: {"primary": 0, "subagent": 0, "compaction": 0, "events": 0})
    for s in sessions:
        author = s.get("tags", {}).get("author", "unknown")
        role = s.get("tags", {}).get("role", "primary")
        by_author[author][role] = by_author[author].get(role, 0) + 1
        by_author[author]["events"] += s.get("event_count", 0)

    commits_by_author = Counter(c["author"] for c in commits)
    merges_by_author = Counter(m["author"] for m in merges)

    return {
        "sessions_by_author": dict(by_author),
        "commits_total": len(commits),
        "commits_by_author": dict(commits_by_author),
        "merges_total": len(merges),
        "merges_by_author": dict(merges_by_author),
    }


def hot_files(sessions: list[dict]) -> list[dict]:
    file_authors: dict[str, set[str]] = defaultdict(set)
    file_count: Counter = Counter()
    for s in sessions:
        author = s.get("tags", {}).get("author", "unknown")
        for f in (s.get("enriched") or {}).get("files_touched", []):
            path = f.get("path") if isinstance(f, dict) else str(f)
            if not path:
                continue
            file_count[path] += 1
            file_authors[path].add(author)
    hot = [
        {
            "path": p,
            "session_count": file_count[p],
            "authors": sorted(file_authors[p]),
            "cross_author": len(file_authors[p]) > 1,
        }
        for p in file_count
        if file_count[p] >= 2
    ]
    hot.sort(key=lambda r: (-r["session_count"], r["path"]))
    return hot


def health(sessions: list[dict]) -> dict:
    ghosts = []
    errors = []
    recall = []
    compactions = []
    for s in sessions:
        sid = s.get("session_id")
        author = s.get("tags", {}).get("author", "unknown")
        role = s.get("tags", {}).get("role", "primary")
        kind = s.get("tags", {}).get("kind")
        ev = s.get("event_count", 0)
        files = (s.get("enriched") or {}).get("files_touched") or []
        errs = (s.get("enriched") or {}).get("errors") or []

        if role == "compaction":
            compactions.append({"session_id": sid, "author": author})
            continue
        if ev <= 5 and not files:
            ghosts.append({"session_id": sid, "author": author, "events": ev})
        if errs:
            empty = sum(1 for e in errs if not (e.get("message") or "").strip())
            errors.append(
                {
                    "session_id": sid,
                    "author": author,
                    "count": len(errs),
                    "empty_message_count": empty,
                }
            )
        if kind == "recall":
            recall.append({"session_id": sid, "author": author})
    return {
        "ghost_sessions": ghosts,
        "error_sessions": errors,
        "compactions": compactions,
        "recall_sessions": recall,
    }


def tokens_trend(base_url: str, today_str: str) -> dict | None:
    """Best-effort 7-day token trend.

    Uses urllib directly to fail soft (the shared fetch() exits on error).
    Returns None if the endpoint is unavailable or the shape differs.
    """
    import json as _json
    import urllib.error
    import urllib.request

    candidates = [
        f"{base_url}/api/daily-token-usage?days=8",
        f"{base_url}/api/tokens/daily?days=8",
    ]
    raw = None
    for url in candidates:
        try:
            with urllib.request.urlopen(url, timeout=10) as resp:
                raw = _json.loads(resp.read())
                break
        except (urllib.error.URLError, _json.JSONDecodeError):
            continue
    if not isinstance(raw, list) or not raw:
        return None
    by_date = {row.get("date"): row for row in raw if isinstance(row, dict)}
    today = by_date.get(today_str)
    if not today:
        return None
    prior = [r.get("total_tokens", 0) for d, r in by_date.items() if d != today_str][-7:]
    avg = sum(prior) / len(prior) if prior else 0
    today_total = today.get("total_tokens", 0)
    return {
        "today_total": today_total,
        "trailing_7d_avg": round(avg),
        "ratio_today_over_avg": round(today_total / avg, 2) if avg else None,
        "today_messages": today.get("message_count", 0),
    }


def measure(bundle: dict, base_url: str, repo_path: str | None) -> dict:
    sessions = bundle["sessions"]
    window = bundle["window"]
    commits = commits_in_window(window["utc_start"], window["utc_end"], cwd=repo_path)
    merges = merges_in_window(window["utc_start"], window["utc_end"], cwd=repo_path)
    metrics = {
        "throughput": throughput(sessions, commits, merges),
        "hot_files": hot_files(sessions),
        "health": health(sessions),
        "tokens": tokens_trend(base_url, window["date"]),
        "commits": commits,
        "merges": merges,
    }
    return {**bundle, "metrics": metrics}


def main() -> None:
    parser = argparse.ArgumentParser(description="Compute DORA-flavored metrics.")
    parser.add_argument("--in", dest="input", default="-")
    parser.add_argument("--out", dest="output", default="-")
    parser.add_argument("--url", default=DEFAULT_URL)
    parser.add_argument("--repo", default=None, help="Path to git repo (default: cwd)")
    args = parser.parse_args()
    bundle = read_json(args.input)
    write_json(measure(bundle, args.url, args.repo), args.output)


if __name__ == "__main__":
    main()
