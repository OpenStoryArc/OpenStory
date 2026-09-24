#!/usr/bin/env python3
"""The four DORA keys from a node's own presence history and git (D-02).

Every beat a node publishes lands as one line in `data/presence.jsonl`
(time, host, principal_id, git_sha, built_at, level, findings). Every line
says which build was running and how it felt. Git says when each build's
commit was made. From those two records, per window:

- deployment frequency: distinct git shas first seen in presence, per day
- lead time for changes: commit time to the first beat carrying that sha
- change failure rate: share of shas whose first hour of presence held a
  critical beat
- time to restore: from a critical beat to the next ok beat on that host

    python3 scripts/dora.py                          # data/presence.jsonl, 7 days, table
    python3 scripts/dora.py --json --days 30
    python3 scripts/dora.py --log /path/presence.jsonl --git-dir /path/repo
    python3 scripts/dora.py --test                   # self-tests on fixtures
"""
from __future__ import annotations

import argparse
import json
import statistics
import subprocess
import sys
from collections.abc import Callable
from datetime import datetime, timedelta, timezone
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
DEFAULT_LOG = REPO / "data" / "presence.jsonl"
FIRST_HOUR = timedelta(hours=1)


def parse_time(s: str) -> datetime | None:
    try:
        return datetime.fromisoformat(s.replace("Z", "+00:00")).astimezone(timezone.utc)
    except (ValueError, AttributeError):
        return None


def read_log(path: Path) -> list[dict]:
    """The presence lines with a readable time and a sha, oldest first."""
    if not path.exists():
        return []
    out = []
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        raw = raw.strip()
        if not raw:
            continue
        try:
            line = json.loads(raw)
        except json.JSONDecodeError:
            continue
        t = parse_time(str(line.get("time", "")))
        if t is None or not line.get("git_sha"):
            continue
        line["_t"] = t
        out.append(line)
    out.sort(key=lambda l: l["_t"])
    return out


def git_commit_time(git_dir: Path) -> Callable[[str], datetime | None]:
    """A lookup from short sha to commit time, via git; None when unknown."""
    cache: dict[str, datetime | None] = {}

    def lookup(sha: str) -> datetime | None:
        if sha not in cache:
            try:
                out = subprocess.run(
                    ["git", "-C", str(git_dir), "show", "-s", "--format=%cI", sha],
                    capture_output=True, text=True, timeout=10, check=False,
                )
                cache[sha] = parse_time(out.stdout.strip()) if out.returncode == 0 else None
            except (OSError, subprocess.SubprocessError):
                cache[sha] = None
        return cache[sha]

    return lookup


def compute(lines: list[dict], now: datetime, days: int, commit_time: Callable[[str], datetime | None]) -> dict:
    """The four keys over the window ending at `now`. Pure given `commit_time`."""
    since = now - timedelta(days=days)
    window = [l for l in lines if l["_t"] >= since]
    by_host: dict[str, list[dict]] = {}
    for l in window:
        by_host.setdefault(str(l.get("host", "?")), []).append(l)

    # First sight of each sha in the window, per host and overall.
    first_seen: dict[str, datetime] = {}
    for l in window:
        sha = str(l["git_sha"])
        if sha not in first_seen or l["_t"] < first_seen[sha]:
            first_seen[sha] = l["_t"]
    shas = sorted(first_seen, key=lambda s: first_seen[s])

    per_day: dict[str, int] = {}
    for sha, t in first_seen.items():
        per_day[t.date().isoformat()] = per_day.get(t.date().isoformat(), 0) + 1
    deployment_frequency = len(shas) / days if days else 0.0

    lead_hours: list[float] = []
    sha_rows = []
    for sha in shas:
        ct = commit_time(sha)
        lead = (first_seen[sha] - ct).total_seconds() / 3600 if ct else None
        if lead is not None and lead >= 0:
            lead_hours.append(lead)
        # Change failure: a critical beat within the first hour of this sha, on any host.
        failed = any(
            l["level"] == "critical"
            for l in window
            if l["git_sha"] == sha and first_seen[sha] <= l["_t"] <= first_seen[sha] + FIRST_HOUR
        )
        sha_rows.append({
            "git_sha": sha,
            "first_seen": first_seen[sha].isoformat(),
            "commit_time": ct.isoformat() if ct else None,
            "lead_time_hours": round(lead, 3) if lead is not None else None,
            "failed_in_first_hour": failed,
        })
    failures = sum(1 for r in sha_rows if r["failed_in_first_hour"])
    change_failure_rate = failures / len(sha_rows) if sha_rows else 0.0

    # Time to restore: each critical episode on a host, from its first
    # critical beat to the next ok beat.
    restore_minutes: list[float] = []
    open_episodes = 0
    for host, hl in by_host.items():
        started: datetime | None = None
        for l in hl:
            level = l.get("level")
            if level == "critical" and started is None:
                started = l["_t"]
            elif level == "ok" and started is not None:
                restore_minutes.append((l["_t"] - started).total_seconds() / 60)
                started = None
        if started is not None:
            open_episodes += 1

    return {
        "window_days": days,
        "since": since.isoformat(),
        "until": now.isoformat(),
        "beats": len(window),
        "hosts": sorted(by_host),
        "deployment_frequency_per_day": round(deployment_frequency, 3),
        "deployments": len(shas),
        "deployments_per_day": per_day,
        "lead_time_hours_median": round(statistics.median(lead_hours), 3) if lead_hours else None,
        "lead_time_samples": len(lead_hours),
        "change_failure_rate": round(change_failure_rate, 3),
        "time_to_restore_minutes_median": round(statistics.median(restore_minutes), 3) if restore_minutes else None,
        "restore_samples": len(restore_minutes),
        "open_criticals": open_episodes,
        "shas": sha_rows,
    }


def render(r: dict) -> str:
    lt = r["lead_time_hours_median"]
    ttr = r["time_to_restore_minutes_median"]
    rows = [
        ("window", f"{r['window_days']} d, {r['beats']} beats, hosts {', '.join(r['hosts']) or 'none'}"),
        ("deployment frequency", f"{r['deployment_frequency_per_day']} / day ({r['deployments']} shas)"),
        ("lead time (median)", f"{lt} h over {r['lead_time_samples']} shas" if lt is not None else "no sha with a known commit time"),
        ("change failure rate", f"{r['change_failure_rate']:.0%} of {r['deployments']} shas"),
        ("time to restore (median)", f"{ttr} min over {r['restore_samples']} episodes" if ttr is not None else "no restored episode"),
        ("open criticals", str(r["open_criticals"])),
    ]
    w = max(len(k) for k, _ in rows)
    return "\n".join(f"{k.ljust(w)}  {v}" for k, v in rows)


# ── self-tests ────────────────────────────────────────────────────────────────

def _fixture(now: datetime) -> list[dict]:
    d = lambda **kw: now - timedelta(**kw)  # noqa: E731
    mk = lambda t, sha, level, host="node-a": {"time": t.isoformat(), "host": host, "principal_id": "dev",  # noqa: E731
                                              "git_sha": sha, "built_at": t.isoformat(), "level": level, "findings": []}
    return [
        # sha A: first seen 3 days ago, critical 10 min in (change failure), ok 30 min later (restore 20 min)
        mk(d(days=3, hours=2), "aaa", "ok"),
        mk(d(days=3, hours=2) + timedelta(minutes=10), "aaa", "critical"),
        mk(d(days=3, hours=2) + timedelta(minutes=30), "aaa", "ok"),
        # sha B: first seen 2 days ago, clean
        mk(d(days=2), "bbb", "ok"),
        mk(d(days=2) + timedelta(minutes=15), "bbb", "ok"),
        # sha C: same day as B, critical after two hours (not a change failure), restored 10 min later
        mk(d(days=2) + timedelta(hours=1), "ccc", "ok"),
        mk(d(days=2) + timedelta(hours=3), "ccc", "critical"),
        mk(d(days=2) + timedelta(hours=3, minutes=10), "ccc", "ok"),
        # another host on sha C, still critical (open episode)
        mk(d(days=1), "ccc", "critical", host="node-b"),
        # outside the window
        mk(d(days=20), "old", "ok"),
        # junk
        {"time": "not a time", "git_sha": "zzz", "level": "ok"},
    ]


def _test() -> int:
    now = datetime(2026, 9, 24, 12, 0, tzinfo=timezone.utc)
    import tempfile
    with tempfile.TemporaryDirectory() as td:
        p = Path(td) / "presence.jsonl"
        p.write_text("\n".join(json.dumps(l) for l in _fixture(now)) + "\nnot json\n")
        lines = read_log(p)
    assert len(lines) == 10, len(lines)
    commits = {"aaa": now - timedelta(days=3, hours=6), "bbb": now - timedelta(days=2, hours=1), "ccc": None, "old": now - timedelta(days=21)}
    r = compute(lines, now, 7, lambda s: commits.get(s))
    assert r["beats"] == 9, r["beats"]
    assert r["deployments"] == 3 and [s["git_sha"] for s in r["shas"]] == ["aaa", "bbb", "ccc"], r["shas"]
    assert r["deployment_frequency_per_day"] == round(3 / 7, 3), r["deployment_frequency_per_day"]
    assert sum(r["deployments_per_day"].values()) == 3
    assert r["lead_time_samples"] == 2, "ccc has no commit time"
    assert r["lead_time_hours_median"] == 2.5, r["lead_time_hours_median"]  # aaa 4 h, bbb 1 h
    assert r["change_failure_rate"] == round(1 / 3, 3), r["change_failure_rate"]
    assert [s["failed_in_first_hour"] for s in r["shas"]] == [True, False, False]
    assert r["restore_samples"] == 2 and r["time_to_restore_minutes_median"] == 15.0, r
    assert r["open_criticals"] == 1, "node-b is still critical"
    assert r["hosts"] == ["node-a", "node-b"]
    empty = compute([], now, 7, lambda s: None)
    assert empty["deployments"] == 0 and empty["lead_time_hours_median"] is None and empty["change_failure_rate"] == 0.0
    assert "deployment frequency" in render(r) and "33%" in render(r)
    print("ok: 14 assertions")
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--log", type=Path, default=DEFAULT_LOG, help="presence.jsonl (default data/presence.jsonl)")
    ap.add_argument("--git-dir", type=Path, default=REPO, help="repo to resolve commit times in")
    ap.add_argument("--days", type=int, default=7)
    ap.add_argument("--json", action="store_true")
    ap.add_argument("--test", action="store_true")
    a = ap.parse_args(argv)
    if a.test:
        return _test()
    lines = read_log(a.log)
    r = compute(lines, datetime.now(timezone.utc), a.days, git_commit_time(a.git_dir))
    r["log"] = str(a.log)
    print(json.dumps(r, indent=2) if a.json else render(r))
    return 0


if __name__ == "__main__":
    sys.exit(main())
