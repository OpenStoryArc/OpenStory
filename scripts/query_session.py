"""Query a session via the Open Story API — synopsis, files, errors, tool journey.

Usage:
    uv run python scripts/query_session.py [SESSION_ID] [--url URL]
    uv run python scripts/query_session.py --current
"""

import argparse
import json
import urllib.request


def fetch(base_url: str, path: str):
    return json.loads(urllib.request.urlopen(f"{base_url}{path}").read())


def query_session(base_url: str, session_id: str) -> None:
    sid = session_id
    print(f"Session: {sid[:12]}")
    print()

    # Synopsis
    synopsis = fetch(base_url, f"/api/sessions/{sid}/synopsis")
    print("=== Synopsis ===")
    for k, v in synopsis.items():
        if k == "top_tools":
            print(f"  {k}:")
            for t in v:
                print(f"    {t['tool']}: {t['count']}")
        else:
            print(f"  {k}: {v}")
    print()

    # File impact
    files = fetch(base_url, f"/api/sessions/{sid}/file-impact")
    print(f"=== File Impact ({len(files)} files) ===")
    for f in sorted(files, key=lambda x: x["reads"] + x["writes"], reverse=True)[:15]:
        path = f["file"].replace("\\", "/")
        parts = path.split("/")
        name = "/".join(parts[-3:]) if len(parts) > 3 else path
        print(f"  {name:50s} {f['reads']}R {f['writes']}W")
    print()

    # Errors
    errors = fetch(base_url, f"/api/sessions/{sid}/errors")
    print(f"=== Errors ({len(errors)}) ===")
    for e in errors:
        ts = e["timestamp"][:19]
        msg = e["message"][:120]
        print(f"  {ts}  {msg}")
    if not errors:
        print("  (none)")
    print()

    # Tool journey
    journey = fetch(base_url, f"/api/sessions/{sid}/tool-journey")
    print(f"=== Tool Journey ({len(journey)} steps) ===")
    for step in journey[:40]:
        ts = step["timestamp"][:19]
        tool = step["tool"]
        target = step.get("file") or ""
        if target:
            target = target.replace("\\", "/")
            parts = target.split("/")
            target = "/".join(parts[-2:]) if len(parts) > 2 else target
        print(f"  {ts}  {tool:8s}  {target}")
    if len(journey) > 40:
        print(f"  ... +{len(journey) - 40} more steps")


def find_current_session(base_url: str) -> str:
    sessions = fetch(base_url, "/api/sessions")
    ongoing = [s for s in sessions if s.get("status") == "ongoing" and not s["session_id"].startswith("agent-")]
    if ongoing:
        ongoing.sort(key=lambda s: s.get("start_time", ""), reverse=True)
        return ongoing[0]["session_id"]
    # Fall back to most recent main session
    main = [s for s in sessions if not s["session_id"].startswith("agent-")]
    main.sort(key=lambda s: s.get("event_count", 0), reverse=True)
    if main:
        return main[0]["session_id"]
    raise SystemExit("No sessions found")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="Query session data")
    parser.add_argument("session_id", nargs="?", default="", help="Session ID")
    parser.add_argument("--url", default="http://localhost:3002", help="API base URL")
    parser.add_argument("--current", action="store_true", help="Use the current ongoing session")
    args = parser.parse_args()

    sid = args.session_id
    if not sid or args.current:
        sid = find_current_session(args.url)

    query_session(args.url, sid)
