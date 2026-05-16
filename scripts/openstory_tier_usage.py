"""Classify each session's *introspection tier* — how the agent (or human)
used OpenStory itself as a skill.

Five tiers, each detected via a deterministic event signature:

    Tier 0  rawdog       grep/rg over ~/.claude/projects/**/*.jsonl
    Tier 1  rest         curl http(s)://.../api/...
    Tier 2  script       Bash: python3 scripts/sessionstory.py / query_store.py / ...
    Tier 3  skill        user_message: "Base directory for this skill: .../sessionstory" or .../team-day
    Tier 4  mcp          tool_call.name starts with "mcp__openstory__"

Sessions can hit multiple tiers — that's fine; each is reported. The
output is meant to make it visible *who* uses OpenStory at *which* tier,
which is a meaningful signal about adoption maturity.

Usage:
    python3 scripts/openstory_tier_usage.py            # corpus rollup
    python3 scripts/openstory_tier_usage.py SESSION_ID # single session
    python3 scripts/openstory_tier_usage.py --csv      # one row per (user,tier)
    python3 scripts/openstory_tier_usage.py --test
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from collections import Counter, defaultdict
from dataclasses import asdict, dataclass, field


DEFAULT_URL = "http://localhost:3002"

# Detection regexes
RAWDOG_RE = re.compile(r"\b(grep|rg)\b[^\n]*\.jsonl|\.claude/projects/[^\s]*\.jsonl")
REST_RE = re.compile(r"localhost:\d+/api/|/api/sessions|/api/search|/api/fleet")
SCRIPT_RE = re.compile(r"scripts/(sessionstory|query_store|token_usage|cost_report|analyze_|check_docs|export_session_md)")
SKILL_INVOKE_RE = re.compile(r"Base directory for this skill:\s+\S+/skills/(sessionstory|team-day|check-docs)\b")
MCP_RE = re.compile(r"^mcp__openstory(?:-remote)?__")


def fetch(base_url: str, path: str) -> object:
    try:
        with urllib.request.urlopen(f"{base_url}{path}", timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {path}: {e}\n")
        sys.exit(2)


TIERS = ["rawdog", "rest", "script", "skill", "mcp"]


@dataclass
class TierUsage:
    session_id: str
    user: str | None
    project: str | None
    rawdog: int = 0
    rest: int = 0
    script: int = 0
    skill: int = 0
    mcp: int = 0

    def has_any(self) -> bool:
        return any(getattr(self, t) for t in TIERS)


# -- Detection -----------------------------------------------------

def extract_text(payload: object) -> str:
    if isinstance(payload, str):
        return payload
    if not isinstance(payload, dict):
        return ""
    c = payload.get("content")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        out: list[str] = []
        for b in c:
            if isinstance(b, dict):
                t = b.get("text") or b.get("content")
                if isinstance(t, str):
                    out.append(t)
        return " ".join(out)
    return ""


def classify_record(rec: dict) -> str | None:
    """Return tier name if the record matches a tier signature, else None."""
    rt = rec.get("record_type")
    payload = rec.get("payload") or {}

    if rt == "tool_call":
        name = payload.get("name") or ""
        if MCP_RE.match(name):
            return "mcp"
        if name == "Bash":
            cmd = ((payload.get("input") or {}).get("command")) or ""
            if SCRIPT_RE.search(cmd):
                return "script"
            if REST_RE.search(cmd):
                return "rest"
            if RAWDOG_RE.search(cmd):
                return "rawdog"

    if rt == "user_message":
        text = extract_text(payload)
        if SKILL_INVOKE_RE.search(text):
            return "skill"

    return None


def for_session(base_url: str, session_id: str, sess_meta: dict | None = None) -> TierUsage:
    """Classify one session. Pass sess_meta from /api/sessions to populate user/project."""
    if sess_meta is None:
        sess_meta = {}
    recs = fetch(base_url, f"/api/sessions/{session_id}/records") or []
    if isinstance(recs, dict):
        recs = recs.get("records") or recs.get("items") or []

    counts = Counter()
    for r in recs:
        tier = classify_record(r)
        if tier:
            counts[tier] += 1

    return TierUsage(
        session_id=session_id,
        user=sess_meta.get("user"),
        project=sess_meta.get("project_name"),
        **{t: counts.get(t, 0) for t in TIERS},
    )


def all_sessions(base_url: str) -> list[dict]:
    res = fetch(base_url, "/api/sessions")
    return res.get("sessions", []) if isinstance(res, dict) else res


# -- Output --------------------------------------------------------

def fmt_md(rows: list[TierUsage]) -> str:
    rows = [r for r in rows if r.has_any()]
    if not rows:
        return "_no OpenStory introspection signal in corpus_\n"

    by_tier_user: dict[str, Counter] = defaultdict(Counter)
    by_tier: Counter = Counter()
    for r in rows:
        for t in TIERS:
            n = getattr(r, t)
            if n:
                by_tier[t] += 1  # session count, not event count
                by_tier_user[t][r.user or "?"] += 1

    out = [f"# OpenStory tier usage  ({len(rows)} sessions touched introspection)\n"]
    out.append("| tier | sessions | user breakdown |")
    out.append("|---|---|---|")
    for t in TIERS:
        n = by_tier[t]
        if n == 0:
            continue
        userstr = ", ".join(f"{u}={c}" for u, c in by_tier_user[t].most_common())
        out.append(f"| {t} | {n} | {userstr} |")

    out.append("\n## Per-session detail\n")
    out.append("| session | user | project | " + " | ".join(TIERS) + " |")
    out.append("|---|---|---|" + "|".join(["---"] * len(TIERS)) + "|")
    for r in sorted(rows, key=lambda r: -sum(getattr(r, t) for t in TIERS)):
        cells = [str(getattr(r, t) or "") for t in TIERS]
        out.append(f"| `{r.session_id[:8]}…` | {r.user or '?'} | {r.project or '?'} | " + " | ".join(cells) + " |")
    return "\n".join(out) + "\n"


def fmt_csv(rows: list[TierUsage]) -> str:
    out = ["session_id,user,project," + ",".join(TIERS)]
    for r in rows:
        out.append(",".join([
            r.session_id, r.user or "", r.project or "",
            *[str(getattr(r, t)) for t in TIERS],
        ]))
    return "\n".join(out) + "\n"


# -- Self-tests ----------------------------------------------------

def _records_for_each_tier() -> list[dict]:
    return [
        # rest
        {"record_type": "tool_call",
         "payload": {"name": "Bash",
                     "input": {"command": "curl -s http://localhost:3002/api/sessions"}}},
        # script
        {"record_type": "tool_call",
         "payload": {"name": "Bash",
                     "input": {"command": "python3 scripts/sessionstory.py SID --json"}}},
        # mcp
        {"record_type": "tool_call",
         "payload": {"name": "mcp__openstory__session_synopsis"}},
        # skill (sessionstory)
        {"record_type": "user_message",
         "payload": {"content":
                     "Base directory for this skill: /Users/x/.claude/skills/sessionstory\n# sessionstory"}},
        # rawdog
        {"record_type": "tool_call",
         "payload": {"name": "Bash",
                     "input": {"command": "grep -r 'foo' /Users/x/.claude/projects/-Users-.../*.jsonl"}}},
        # noise — should not classify
        {"record_type": "tool_call",
         "payload": {"name": "Read", "input": {"file_path": "/x/y.rs"}}},
    ]


def selftest() -> int:
    failures = 0
    def check(name, cond, detail=""):
        nonlocal failures
        print(f"  {'ok  ' if cond else 'FAIL'} {name}{('   '+detail) if not cond else ''}")
        if not cond: failures += 1

    print("== classify_record ==")
    recs = _records_for_each_tier()
    classes = [classify_record(r) for r in recs]
    check("rest classified", classes[0] == "rest", str(classes[0]))
    check("script classified", classes[1] == "script", str(classes[1]))
    check("mcp classified", classes[2] == "mcp", str(classes[2]))
    check("skill classified", classes[3] == "skill", str(classes[3]))
    check("rawdog classified", classes[4] == "rawdog", str(classes[4]))
    check("non-introspection ignored", classes[5] is None)

    print("== fmt_md ==")
    u = TierUsage(session_id="S1abcdefghi", user="u", project="P",
                  rest=2, script=1, mcp=1, skill=1, rawdog=0)
    md = fmt_md([u])
    check("md has header", "tier usage" in md)
    check("md has user", "u" in md)
    check("md per-session row", "S1abcdef" in md)

    print("\nFAILED" if failures else "\nall tests passed")
    return 1 if failures else 0


# -- CLI -----------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("session_id", nargs="?", help="single session, omit for corpus")
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--json", action="store_true")
    p.add_argument("--csv", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())

    sessions = all_sessions(args.url)
    by_id = {s["session_id"]: s for s in sessions}

    if args.session_id:
        u = for_session(args.url, args.session_id, by_id.get(args.session_id))
        if args.json:
            print(json.dumps(asdict(u), indent=2))
        elif args.csv:
            print(fmt_csv([u]))
        else:
            print(fmt_md([u]))
        return

    rows = [for_session(args.url, s["session_id"], s) for s in sessions]
    if args.json:
        print(json.dumps([asdict(r) for r in rows if r.has_any()], indent=2))
    elif args.csv:
        print(fmt_csv([r for r in rows if r.has_any()]))
    else:
        print(fmt_md(rows))


if __name__ == "__main__":
    main()
