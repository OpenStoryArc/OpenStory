"""Extract chronological git commands from a team-day window.

Reads bundle.json for the date, resolves each session's full ID from the
sessions API (bundle IDs are sometimes truncated/derived), then pulls
records and filters Bash tool_use payloads whose command starts with `git`.

Usage: python3 scripts/team_day/git_flow.py --date 2026-05-02
"""
from __future__ import annotations
import argparse, json, sys, urllib.request
from pathlib import Path
from datetime import datetime, timezone, timedelta

API = "http://localhost:3002"

def get(path):
    with urllib.request.urlopen(f"{API}{path}") as r:
        return json.loads(r.read())

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--date", required=True)
    ap.add_argument("--tz-offset", type=int, default=-4, help="hours from UTC")
    args = ap.parse_args()

    bundle_path = Path(f"captures/team_day/{args.date}/bundle.json")
    bundle = json.loads(bundle_path.read_text())
    bundle_sessions = bundle["sessions"]

    # Resolve to live session IDs by (project_id, start_time within day)
    all_sessions = get("/api/sessions")
    if isinstance(all_sessions, dict):
        all_sessions = all_sessions.get("sessions", [])

    tz = timezone(timedelta(hours=args.tz_offset))
    day_start = datetime.fromisoformat(f"{args.date}T00:00:00").replace(tzinfo=tz).astimezone(timezone.utc)
    day_end = day_start + timedelta(days=1)

    todays = []
    for s in all_sessions:
        st = s.get("start_time")
        if not st: continue
        ts = datetime.fromisoformat(st.replace("Z","+00:00"))
        if day_start <= ts < day_end:
            todays.append(s)

    print(f"# Git command flow — {args.date}", flush=True)
    print(f"# {len(todays)} sessions in window\n", flush=True)

    rows = []
    for s in todays:
        sid = s["session_id"]
        user = s.get("user", "?")
        try:
            recs = get(f"/api/sessions/{sid}/records?limit=5000")
        except Exception as e:
            print(f"# skip {sid}: {e}", flush=True)
            continue
        if isinstance(recs, dict):
            recs = recs.get("records", [])
        for r in recs:
            if r.get("record_type") != "tool_call": continue
            p = r.get("payload", {})
            name = p.get("name") or p.get("tool_name", "")
            if name != "Bash": continue
            inp = p.get("input") or p.get("tool_input") or p.get("args") or {}
            cmd = (inp.get("command") or "").strip()
            if not cmd: continue
            # Match git verbs (top-level, not inside flags/strings)
            first = cmd.split()[0] if cmd.split() else ""
            if first != "git" and not cmd.startswith("git "): continue
            rows.append((r.get("timestamp",""), user, sid[:8], cmd))

    rows.sort()
    for ts, user, sid, cmd in rows:
        local = ts[:19]  # UTC for now; user can mentally adjust
        # one-line: trim long commands
        c = cmd if len(cmd) <= 140 else cmd[:137] + "..."
        print(f"{local}Z  {user:12} {sid}  {c}")

    print(f"\n# total git invocations: {len(rows)}")

if __name__ == "__main__":
    main()
