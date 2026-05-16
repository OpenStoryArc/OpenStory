"""Detect which skills and knowledge sources were used during sessions.

Three orthogonal signals, all already in the event stream:

1. INVOKE — `user_message` contains "Base directory for this skill: .../skills/<name>".
   This is a slash-command invocation; it appears once per `/skill-name` call.
2. READ   — `tool_call` Read whose path matches `**/SKILL.md` or `**/.agents/skills/**`.
   The agent loaded skill text without invoking the skill as a slash command.
3. KNOW   — `tool_call` Read of `*.md` files outside `node_modules`/`target`/etc.
   Knowledge consultation: docs/, README.md, CLAUDE.md.

Each signal is reported separately. They overlap intentionally — invoking a
skill often also reads the SKILL.md, but they are different intent signals.

Usage:
    python3 scripts/skills_used.py SESSION_ID
    python3 scripts/skills_used.py all                 # corpus-level rollup
    python3 scripts/skills_used.py all --csv           # one row per skill
    python3 scripts/skills_used.py --test
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

SKILL_INVOKE_RE = re.compile(r"Base directory for this skill:\s+(\S+)")
SKILL_NAME_RE = re.compile(r"/skills/([a-z0-9_-]+)")
SKILL_FILE_RE = re.compile(r"/(skills|agents/skills)/([a-z0-9_-]+)/SKILL\.md$")

KNOWLEDGE_EXCLUDES = ("/node_modules/", "/target/", "/.git/", "/dist/", "/build/")


def fetch(base_url: str, path: str) -> object:
    try:
        with urllib.request.urlopen(f"{base_url}{path}", timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {path}: {e}\n")
        sys.exit(2)


@dataclass
class SkillsUsed:
    session_id: str
    user: str | None
    project: str | None
    invoked: list[str] = field(default_factory=list)        # skill names
    read_skills: list[str] = field(default_factory=list)    # skill names
    knowledge: list[str] = field(default_factory=list)      # md paths


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


def detect_invokes(records: list[dict]) -> list[str]:
    out: list[str] = []
    for r in records:
        if r.get("record_type") != "user_message":
            continue
        text = extract_text(r.get("payload"))
        m = SKILL_INVOKE_RE.search(text)
        if not m:
            continue
        path = m.group(1)
        nm = SKILL_NAME_RE.search(path)
        if nm:
            out.append(nm.group(1))
    return out


def detect_skill_reads(records: list[dict]) -> list[str]:
    out: list[str] = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") not in ("Read",):
            continue
        path = ((payload.get("input") or {}).get("file_path")) or ""
        m = SKILL_FILE_RE.search(path)
        if m:
            out.append(m.group(2))
    return out


def detect_knowledge(records: list[dict]) -> list[str]:
    out: list[str] = []
    for r in records:
        if r.get("record_type") != "tool_call":
            continue
        payload = r.get("payload") or {}
        if payload.get("name") not in ("Read",):
            continue
        path = ((payload.get("input") or {}).get("file_path")) or ""
        if not path.endswith(".md"):
            continue
        if any(x in path for x in KNOWLEDGE_EXCLUDES):
            continue
        if SKILL_FILE_RE.search(path):
            continue  # already counted under skill reads
        out.append(path)
    return out


def for_session(base_url: str, session_id: str, sess_meta: dict | None = None) -> SkillsUsed:
    """User/project from /api/sessions list (the /summary endpoint omits them)."""
    if sess_meta is None:
        sess_list = fetch(base_url, "/api/sessions")
        for s in (sess_list.get("sessions") or sess_list):
            if s.get("session_id") == session_id:
                sess_meta = s
                break
        sess_meta = sess_meta or {}
    recs = fetch(base_url, f"/api/sessions/{session_id}/records") or []
    if isinstance(recs, dict):
        recs = recs.get("records") or recs.get("items") or []
    return SkillsUsed(
        session_id=session_id,
        user=sess_meta.get("user"),
        project=sess_meta.get("project_name"),
        invoked=detect_invokes(recs),
        read_skills=detect_skill_reads(recs),
        knowledge=detect_knowledge(recs),
    )


def all_sessions(base_url: str) -> list[dict]:
    res = fetch(base_url, "/api/sessions")
    return res.get("sessions", []) if isinstance(res, dict) else res


# -- Output --------------------------------------------------------

def fmt_md(s: SkillsUsed) -> str:
    inv = Counter(s.invoked).most_common()
    rs = Counter(s.read_skills).most_common()
    out = [f"# Skills used: `{s.session_id}`",
           f"_{s.user or '?'} · {s.project or '?'}_",
           "",
           f"## Invoked ({sum(c for _,c in inv)} call(s))"]
    out += [f"- /{n} × {c}" for n, c in inv] or ["_(none)_"]
    out += ["", f"## Skill files read (without invoke) ({sum(c for _,c in rs)} read(s))"]
    out += [f"- {n} × {c}" for n, c in rs] or ["_(none)_"]
    out += ["", f"## Knowledge consulted ({len(s.knowledge)} read(s))"]
    out += [f"- `{p}`" for p in dict.fromkeys(s.knowledge)] or ["_(none)_"]
    return "\n".join(out) + "\n"


def fmt_corpus_csv(rows: list[SkillsUsed]) -> str:
    skill_user: dict[str, Counter] = defaultdict(Counter)
    for r in rows:
        for sk in r.invoked:
            skill_user[sk][r.user or "?"] += 1
    out = ["skill,total_invocations,by_user"]
    for sk, users in sorted(skill_user.items(), key=lambda kv: -sum(kv[1].values())):
        total = sum(users.values())
        usermix = ";".join(f"{u}={c}" for u, c in users.most_common())
        out.append(f"/{sk},{total},{usermix}")
    return "\n".join(out) + "\n"


# -- Self-tests ----------------------------------------------------

def _fixture_records() -> list[dict]:
    return [
        {"record_type": "user_message",
         "payload": {"content": "Base directory for this skill: /Users/x/.claude/skills/team-day"}},
        {"record_type": "user_message",
         "payload": {"content": "hi there"}},
        {"record_type": "tool_call",
         "payload": {"name": "Read",
                     "input": {"file_path": "/Users/x/.claude/skills/team-day/SKILL.md"}}},
        {"record_type": "tool_call",
         "payload": {"name": "Read",
                     "input": {"file_path": "/Users/x/proj/docs/architecture.md"}}},
        {"record_type": "tool_call",
         "payload": {"name": "Read",
                     "input": {"file_path": "/Users/x/proj/CLAUDE.md"}}},
        {"record_type": "tool_call",
         "payload": {"name": "Read",
                     "input": {"file_path": "/Users/x/proj/node_modules/foo/README.md"}}},
    ]


def selftest() -> int:
    failures = 0
    def check(name, cond, detail=""):
        nonlocal failures
        print(f"  {'ok  ' if cond else 'FAIL'} {name}{('   '+detail) if not cond else ''}")
        if not cond: failures += 1

    recs = _fixture_records()
    print("== detect_invokes ==")
    inv = detect_invokes(recs)
    check("one invoke detected", inv == ["team-day"], str(inv))

    print("== detect_skill_reads ==")
    rs = detect_skill_reads(recs)
    check("one skill-md read", rs == ["team-day"], str(rs))

    print("== detect_knowledge ==")
    kn = detect_knowledge(recs)
    check("two knowledge reads (SKILL.md and node_modules excluded)", len(kn) == 2, str(kn))
    check("CLAUDE.md included", any(p.endswith("CLAUDE.md") for p in kn))
    check("node_modules excluded", not any("node_modules" in p for p in kn))

    print("== fmt_md ==")
    s = SkillsUsed(session_id="S1", user="u", project="P",
                   invoked=inv, read_skills=rs, knowledge=kn)
    md = fmt_md(s)
    check("md lists invoke", "/team-day" in md)
    check("md lists knowledge path", "CLAUDE.md" in md)

    print("\nFAILED" if failures else "\nall tests passed")
    return 1 if failures else 0


# -- CLI -----------------------------------------------------------

def main() -> None:
    p = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    p.add_argument("session_id", nargs="?")
    p.add_argument("--url", default=DEFAULT_URL)
    p.add_argument("--json", action="store_true")
    p.add_argument("--csv", action="store_true")
    p.add_argument("--test", action="store_true")
    args = p.parse_args()

    if args.test:
        sys.exit(selftest())
    if not args.session_id:
        p.error("session_id required (or 'all', --test)")

    if args.session_id == "all":
        sids = [s["session_id"] for s in all_sessions(args.url)]
        rows = [for_session(args.url, sid) for sid in sids]
        rows = [r for r in rows if r.invoked or r.read_skills]
        if args.json:
            print(json.dumps([asdict(r) for r in rows], indent=2))
        elif args.csv:
            print(fmt_corpus_csv(rows))
        else:
            for r in rows:
                inv = ",".join(sorted(set(r.invoked))) or "—"
                print(f"{r.user or '?':<12} {r.project or '?':<24} invoke=[{inv}]  reads={len(r.read_skills)}  knowledge={len(r.knowledge)}  {r.session_id}")
        return

    s = for_session(args.url, args.session_id)
    if args.json:
        print(json.dumps(asdict(s), indent=2))
    else:
        print(fmt_md(s))


if __name__ == "__main__":
    main()
