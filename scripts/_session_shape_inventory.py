#!/usr/bin/env python3
"""Quick reconnaissance for session-shape research.

Surfaces a spread of sessions to use as classifier seeds:
  - top-N by event count (long sessions = richer shapes)
  - subagent sessions (agent-* — direct tree-recursive signal source)
  - small sessions (< 30 events) to check the "single-shot/empty" case

Not a permanent script — delete or fold into analyze_session_shape.py once the
classifier exists. The leading underscore signals "throwaway probe."
"""

import json
import urllib.request

API = "http://localhost:3002/api"


def main() -> None:
    sessions = json.loads(urllib.request.urlopen(f"{API}/sessions").read())["sessions"]
    sessions.sort(key=lambda s: -s["event_count"])

    agents = [s for s in sessions if s["session_id"].startswith("agent-")]
    mains = [s for s in sessions if not s["session_id"].startswith("agent-")]
    small = [s for s in mains if 5 < s["event_count"] < 30]

    print(f"total={len(sessions)}  mains={len(mains)}  agents={len(agents)}\n")

    print("=== top 10 main sessions by event count ===")
    for s in mains[:10]:
        sid = s["session_id"]
        label = (s.get("label") or "")[:58]
        print(f"  {sid:<40} {s['event_count']:>6}  {label}")

    print("\n=== sample agent (subagent) sessions ===")
    for s in agents[:10]:
        label = (s.get("label") or "")[:50]
        print(f"  {s['session_id']:<40} {s['event_count']:>5}  {label}")

    print("\n=== small sessions (5 < events < 30) ===")
    for s in small[:5]:
        label = (s.get("label") or "")[:50]
        print(f"  {s['session_id']:<40} {s['event_count']:>5}  {label}")


if __name__ == "__main__":
    main()
