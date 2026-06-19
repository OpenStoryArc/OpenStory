"""Analyze how OpenStory itself is used: which openstory MCP tools agents call,
what queries they pass, and what prompts make agents reach for the tool.

Three lenses on the same store:

  1. AGENT lens   — every real `mcp__openstory*` tool_use block, histogrammed by
     tool, with the actual `query`/input agents passed. "What question does an
     agent ask the data, and how does each tool answer it."

  2. QUERY lens   — the verbatim FTS search terms agents passed, themselves
     bucketed. "What are agents actually searching the sessions for."

  3. PROMPT lens  — the user prompt that *triggered* each openstory call (nearest
     preceding prompt in the same session), bucketed by intent. The user almost
     never types "openstory" literally, so this is the honest answer to "what are
     my prompts that use openstory."

Tool name and input live at `data.raw.message.content[i]` (name / input). The
flattened `data.args` and `data.tool` are NOT reliably populated, and matching
the raw payload on the string `mcp__openstory` alone catches ToolSearch *select*
calls — so we parse the content blocks and keep only real tool_use blocks whose
name starts with `mcp__openstory`.

Usage:
    uv run python scripts/openstory_self_usage.py
    uv run python scripts/openstory_self_usage.py --data-dir ./data
    uv run python scripts/openstory_self_usage.py --test
"""

import argparse
import json
import re
import sqlite3
import sys
from bisect import bisect_right
from collections import Counter, defaultdict
from pathlib import Path


# -- Extraction -------------------------------------------------------

def openstory_calls(conn: sqlite3.Connection) -> list[dict]:
    """Every real openstory MCP tool_use block.

    Returns [{session_id, timestamp, tool (bare), full, input}]. Parses the raw
    content array so ToolSearch-select noise (name='ToolSearch') is excluded."""
    rows = conn.execute(
        "SELECT session_id, timestamp, payload FROM events "
        "WHERE subtype = 'message.assistant.tool_use' "
        "AND payload LIKE '%mcp__openstory%'"
    ).fetchall()
    calls: list[dict] = []
    for r in rows:
        try:
            content = json.loads(r["payload"])["data"]["raw"]["message"]["content"]
        except (KeyError, TypeError, json.JSONDecodeError):
            continue
        if not isinstance(content, list):
            continue
        for block in content:
            if not isinstance(block, dict):
                continue
            name = block.get("name", "")
            if block.get("type") == "tool_use" and name.startswith("mcp__openstory"):
                calls.append({
                    "session_id": r["session_id"],
                    "timestamp": r["timestamp"],
                    "tool": name.split("__")[-1],          # fold local/remote
                    "full": name,
                    "input": block.get("input") or {},
                })
    return calls


def _prompt_text(payload: str) -> str | None:
    """Pull the typed prompt text. The translator populates `data.text` for only
    a sliver of prompts; the durable home is `data.raw.message.content`, which is
    either a plain string (typed text) or a list of content blocks."""
    try:
        data = json.loads(payload)["data"]
    except (KeyError, TypeError, json.JSONDecodeError):
        return None
    if data.get("text"):
        return data["text"]
    content = data.get("raw", {}).get("message", {}).get("content")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [b.get("text", "") for b in content if isinstance(b, dict) and b.get("type") == "text"]
        joined = " ".join(p for p in parts if p)
        return joined or None
    return None


def user_prompts(conn: sqlite3.Connection) -> list[dict]:
    """All user prompts as [{session_id, timestamp, text}], timestamp-sorted."""
    rows = conn.execute(
        "SELECT session_id, timestamp, payload FROM events "
        "WHERE subtype = 'message.user.prompt' ORDER BY session_id, timestamp"
    ).fetchall()
    out = []
    for r in rows:
        text = _prompt_text(r["payload"])
        if text:
            out.append({"session_id": r["session_id"], "timestamp": r["timestamp"], "text": text})
    return out


def triggering_prompts(calls: list[dict], prompts: list[dict]) -> list[str]:
    """For each openstory call, the nearest preceding user prompt in its session.

    Deduped — a prompt that spawns ten openstory calls counts once."""
    by_session: dict[str, list[dict]] = defaultdict(list)
    for p in prompts:
        by_session[p["session_id"]].append(p)
    for ps in by_session.values():
        ps.sort(key=lambda p: p["timestamp"])

    seen: set[tuple[str, str]] = set()
    result: list[str] = []
    for call in calls:
        ps = by_session.get(call["session_id"])
        if not ps:
            continue
        times = [p["timestamp"] for p in ps]
        i = bisect_right(times, call["timestamp"]) - 1
        if i < 0:
            continue
        p = ps[i]
        key = (p["session_id"], p["timestamp"])
        if key not in seen:
            seen.add(key)
            result.append(p["text"])
    return result


def calls_with_context(conn: sqlite3.Connection) -> list[dict]:
    """openstory_calls enriched with the session's project name (for the story)."""
    proj = {r["id"]: r["project_name"]
            for r in conn.execute("SELECT id, project_name FROM sessions").fetchall()}
    calls = openstory_calls(conn)
    for c in calls:
        c["project"] = proj.get(c["session_id"]) or "(none)"
    return calls


def story_corpus(conn: sqlite3.Connection) -> dict:
    """The raw material for the narrative: month/project spread, plus the verbatim
    triggering prompts and search queries in chronological order with context."""
    calls = calls_with_context(conn)
    prompts = user_prompts(conn)

    # nearest preceding prompt per call, carrying the call's date/project
    by_session: dict[str, list[dict]] = defaultdict(list)
    for p in prompts:
        by_session[p["session_id"]].append(p)
    for ps in by_session.values():
        ps.sort(key=lambda p: p["timestamp"])

    trig, seen = [], set()
    for c in sorted(calls, key=lambda c: c["timestamp"]):
        ps = by_session.get(c["session_id"])
        if not ps:
            continue
        times = [p["timestamp"] for p in ps]
        i = bisect_right(times, c["timestamp"]) - 1
        if i < 0:
            continue
        p = ps[i]
        key = (p["session_id"], p["timestamp"])
        if key in seen:
            continue
        seen.add(key)
        trig.append({"date": (c["timestamp"] or "")[:10], "project": c["project"],
                     "intent": categorize(p["text"]), "text": " ".join(p["text"].split())})

    qrows = [{"date": (c["timestamp"] or "")[:10], "project": c["project"],
              "query": str(c["input"]["query"])}
             for c in sorted(calls, key=lambda c: c["timestamp"]) if c["input"].get("query")]

    return {
        "months": Counter((c["timestamp"] or "")[:7] for c in calls),
        "projects": Counter(c["project"] for c in calls),
        "triggering": trig,
        "queries": qrows,
    }


def print_story(conn: sqlite3.Connection) -> None:
    s = story_corpus(conn)
    print("BY MONTH:", dict(sorted(s["months"].items())))
    print("\nBY PROJECT:")
    for p, n in s["projects"].most_common():
        print(f"  {n:>4}  {p}")
    print(f"\n=== TRIGGERING PROMPTS ({len(s['triggering'])}) — chronological ===")
    for t in s["triggering"]:
        print(f"\n[{t['date']} · {t['project']} · {t['intent']}]\n  {t['text'][:400]}")
    print(f"\n=== SEARCH QUERIES ({len(s['queries'])}) — chronological ===")
    for q in s["queries"]:
        print(f"  {q['date']} · {q['project']:<22} {q['query']}")


# -- Intent buckets ---------------------------------------------------

# First match wins; specific before generic.
PROMPT_BUCKETS: list[tuple[str, re.Pattern]] = [
    ("what happened / summarize a session", re.compile(r"\b(what happened|summar|recap|tell.*story|session ?story|what.*work(ed|ing)? on|catch me up|pick up where|story of)\b", re.I)),
    ("token / cost", re.compile(r"\b(tokens?|cost|spend|spent|how much|budget)\b", re.I)),
    ("search / find across sessions", re.compile(r"\b(search|find|look up|where did|which session|have (we|i) ever|did (we|i) ever|last time)\b", re.I)),
    ("dogfood / verify the product", re.compile(r"\b(dogfood|verify|does .* render|rendering|check the (data|api|ui|store)|eat our own|use openstory)\b", re.I)),
    ("query data shape / counts", re.compile(r"\b(how many|count|distribution|how often|most common|pattern|detector|histogram|breakdown|analy[sz])\b", re.I)),
    ("project / activity pulse", re.compile(r"\b(pulse|activity|recent sessions|what.*projects?|across projects|productiv)\b", re.I)),
    ("boot / run the stack", re.compile(r"\b(start|boot|run|stand up|spin up|launch)\b", re.I)),
]


def categorize(text: str) -> str:
    for label, pat in PROMPT_BUCKETS:
        if pat.search(text):
            return label
    return "other / open-ended"


QUERY_BUCKETS: list[tuple[str, re.Pattern]] = [
    ("error / failure", re.compile(r"\b(error|fail|panic|crash|bug|broke|exception)\b", re.I)),
    ("token / cost", re.compile(r"\b(tokens?|cost|usage|cache)\b", re.I)),
    ("a tool / command", re.compile(r"\b(bash|edit|grep|tool|command|cargo|npm|git)\b", re.I)),
    ("a feature / subsystem", re.compile(r"\b(nats|leaf|hub|federation|mongo|sqlite|watcher|translate|consumer|pattern|detector|websocket|auth|deploy)\b", re.I)),
    ("a file / path", re.compile(r"\.(rs|ts|py|md|toml)\b|/|src", re.I)),
]


def categorize_query(q: str) -> str:
    for label, pat in QUERY_BUCKETS:
        if pat.search(q):
            return label
    return "concept / freeform"


# Short chart labels for the intent buckets (the long names are for the text report).
SHORT_LABEL = {
    "what happened / summarize a session": "what happened",
    "token / cost": "token / cost",
    "search / find across sessions": "search",
    "dogfood / verify the product": "dogfood",
    "query data shape / counts": "data shape",
    "project / activity pulse": "project pulse",
    "boot / run the stack": "boot",
}

# Not every "nearest preceding message" is a real prompt. Harness injections and
# continuation banners get captured as text but aren't something the user asked —
# they must not be charted as intent. Excluded entirely from the split.
_NOISE = re.compile(
    r"<system-reminder>|this session is being continued|<local-command-|<command-(name|message)|"
    r"caveat: the messages below|^\s*\(base\)|git:\(", re.I)


def _is_noise(text: str) -> bool:
    return not text.strip() or bool(_NOISE.search(text))


def deliberate_ambient(calls: list[dict], prompts: list[dict]) -> dict:
    """Split the calls' triggering prompts into 'deliberate' (the prompt itself asks
    OpenStory something — a named intent) vs 'ambient' (a real task prompt where the
    agent reached for OpenStory on its own). Harness/continuation noise is dropped,
    not counted — so 'other' never sits at the top of a chart as an artifact."""
    real = [t for t in triggering_prompts(calls, prompts) if not _is_noise(t)]
    intents: Counter = Counter()
    ambient = 0
    for t in real:
        c = categorize(t)
        if c == "other / open-ended":
            ambient += 1
        else:
            intents[c] += 1
    deliberate = sum(intents.values())
    total = deliberate + ambient
    return {"ambient": ambient, "deliberate": deliberate, "intents": intents,
            "total": total, "ambient_pct": round(ambient / total * 100) if total else 0,
            "noise_dropped": len(triggering_prompts(calls, prompts)) - len(real)}


# -- Tool purpose (the "how does the data help" column) ---------------

TOOL_PURPOSE = {
    "search": "full-text FTS across all events — 'where did X happen'",
    "agent_search": "FTS grouped by session — 'which sessions touched X'",
    "list_sessions": "session inventory — 'what have I worked on'",
    "session_synopsis": "one session's goal/tools/errors — 'what was this'",
    "session_story": "narrated session walkthrough",
    "session_activity": "live activity / event timeline",
    "session_transcript": "reconstructed conversation",
    "session_patterns": "detected eval-apply / sentence patterns",
    "session_plans": "extracted plan markdown",
    "session_sentences": "turn.sentence detector output",
    "session_errors": "errors in a session",
    "tool_journey": "ordered tool-call sequence",
    "token_usage": "input/output/cache tokens + cost",
    "daily_token_usage": "tokens per day",
    "project_pulse": "activity per project over a window",
    "project_context": "recent sessions for a project",
    "recent_files": "recently touched files",
    "file_impact": "reads vs writes per file",
    "productivity": "events by hour-of-day",
    "subscribe_session": "live WebSocket stream of a session",
    "subscribe_tokens": "live token stream",
}


# -- Report -----------------------------------------------------------

def connect(db_path: Path) -> sqlite3.Connection:
    if not db_path.exists():
        print(f"Database not found: {db_path}", file=sys.stderr)
        sys.exit(1)
    conn = sqlite3.connect(str(db_path))
    conn.row_factory = sqlite3.Row
    return conn


def bar(n: int, total: int, width: int = 30) -> str:
    return "█" * round(n / total * width) if total else ""


def print_report(conn: sqlite3.Connection) -> None:
    calls = openstory_calls(conn)
    prompts = user_prompts(conn)
    total = len(calls)

    hist = Counter(c["tool"] for c in calls)
    print("=" * 76)
    print("  AGENT LENS — which openstory MCP tools agents call")
    print("=" * 76)
    print(f"  {total:,} real openstory MCP calls (local + remote folded)\n")
    print(f"  {'calls':>5}  {'tool':<20}  what question it answers")
    print(f"  {'-'*5}  {'-'*20}  {'-'*42}")
    for tool, n in hist.most_common():
        print(f"  {n:>5}  {tool:<20}  {TOOL_PURPOSE.get(tool,'')}")

    # Query lens
    queries = [str(c["input"].get("query")) for c in calls if c["input"].get("query")]
    qhist = Counter(categorize_query(q) for q in queries)
    print()
    print("=" * 76)
    print(f"  QUERY LENS — what agents searched for ({len(queries)} search/agent_search queries)")
    print("=" * 76)
    for label, n in qhist.most_common():
        print(f"  {n:>5}  {bar(n, len(queries))}  {label}")
    print("\n  Verbatim sample (most recent 20):")
    seen = set()
    for c in sorted(calls, key=lambda c: c["timestamp"], reverse=True):
        q = c["input"].get("query")
        if q and q not in seen:
            seen.add(q)
            print(f"    • {q}")
        if len(seen) >= 20:
            break

    # Explicit-mention lens — your prompts that name OpenStory directly
    mentions = [p["text"] for p in prompts if "openstor" in p["text"].lower()]
    mhist = Counter(categorize(t) for t in mentions)
    print()
    print("=" * 76)
    print("  YOUR-WORDS LENS — prompts where you explicitly say 'openstory'")
    print("=" * 76)
    print(f"  {len(mentions)} such prompts, bucketed by what you ask for\n")
    print(f"  {'count':>5}  intent")
    print(f"  {'-'*5}  {'-'*50}")
    for label, n in mhist.most_common():
        print(f"  {n:>5}  {label}")
        for s in [t for t in mentions if categorize(t) == label][:2]:
            print(f"           “{' '.join(s.split())[:120]}”")

    # Prompt lens
    trig = triggering_prompts(calls, prompts)
    phist = Counter(categorize(t) for t in trig)
    print()
    print("=" * 76)
    print("  PROMPT LENS — the prompts that made an agent reach for openstory")
    print("=" * 76)
    lit = sum(1 for p in prompts if "openstor" in p["text"].lower())
    print(f"  {len(trig)} distinct triggering prompts "
          f"(you typed the literal word 'openstory' in only {lit} of {len(prompts):,} prompts)\n")
    print(f"  {'count':>5}  intent")
    print(f"  {'-'*5}  {'-'*50}")
    for label, n in phist.most_common():
        print(f"  {n:>5}  {label}")
        for s in [t for t in trig if categorize(t) == label][:3]:
            print(f"           “{' '.join(s.split())[:120]}”")
    print()


# -- HTML report ------------------------------------------------------

def _esc(s: str) -> str:
    return (str(s).replace("&", "&amp;").replace("<", "&lt;")
            .replace(">", "&gt;").replace('"', "&quot;"))


def _oneline(s: str, n: int = 160) -> str:
    s = " ".join(s.split())
    return s[:n] + ("…" if len(s) > n else "")


# Standing preference: keep collaborators' real names out of written-down
# artifacts (this draft is heading for the public site). Redact to roles in any
# verbatim user text we surface. The agent name "Bobby" is intentionally kept.
_SCRUB = [(re.compile(r"\bkloughra\b", re.I), "a-teammate"),
          (re.compile(r"\bkatie('s)?\b", re.I), "a teammate"),
          (re.compile(r"\bmaxglassie\b", re.I), "the-user"),
          # project codenames / client names → neutral, for a public artifact
          (re.compile(r"\byc[- ]?app\b", re.I), "a-project"),
          (re.compile(r"\bycombinator(-app)?\b", re.I), "a-project"),
          (re.compile(r"\by combinator\b", re.I), "a program"),
          (re.compile(r"\ba16z[-\w]*", re.I), "a-project"),
          (re.compile(r"\braptor[-\w]*", re.I), "a-project"),
          (re.compile(r"\bdora-metrics\b", re.I), "a-project"),
          (re.compile(r"\byc\b", re.I), "a program")]


def _scrub(s: str) -> str:
    for pat, repl in _SCRUB:
        s = pat.sub(repl, s)
    return s


# The narrative — the "story of the problem set." Static prose (not data-derived),
# anonymized, with name-free verbatim quotes pulled from the corpus. Each act
# rotates an accent through the Tokyo Night palette.
NARRATIVE = [
    {"n": 1, "c": "blue", "title": "The mirror", "period": "Week 1",
     "q": "does it even work on me?",
     "body": "The first calls are tentative and self-directed — what happened recently, then "
             "cost and conscience. And before any data is shared, a sweep of your own record "
             "for anything you wouldn't want seen. The first real job isn't observability — "
             "it's sovereignty in practice.",
     "quotes": ["can you tell me my total spend for all my sessions?",
                "session history will get ported to a friend's laptop… see if there's "
                "anything sensitive or embarrassing?"]},
    {"n": 2, "c": "cyan", "title": "Co-presence", "period": "Week 1–2",
     "q": "is anyone else here?",
     "body": "The lens swings outward to people. The moment sessions federate, OpenStory stops "
             "being a personal mirror and becomes a shared room — you can tell when someone "
             "else is in it.",
     "quotes": ["see if a teammate is watching this session right now",
                "can you see recent sessions / branches related to this deployment?"]},
    {"n": 3, "c": "purple", "title": "Origin", "period": "Week 2–4",
     "q": "tell the story of the work",
     "body": "Once it can see across people and time, the natural next ask is narration — first "
             "playfully, then in earnest. The session log graduates from log to canonical "
             "memory: the record of how the work itself came to be.",
     "quotes": ["analyze all my session history, compile the relevant sessions, and trace the "
                "story of the work… script when possible to be deterministic.",
                "highlight the key decisions, key moments, and the creative process that led here."]},
    {"n": 4, "c": "green", "title": "The coach", "period": "Week 3–5",
     "q": "how am I doing?",
     "body": "A quieter, sharper use — the mirror turned evaluative. Not 'what happened' but "
             "'what does what-happened say about how I work.'",
     "quotes": ["give me feedback on the quality of my prompt engineering… what do I do well, "
                "what can I do better?",
                "what semantic direction have my sessions been pointing?"]},
    {"n": 5, "c": "orange", "title": "The market", "period": "Week 4–5",
     "q": "do other people have this problem?",
     "body": "Questions from an early adopter arrive — and the meta-move right after: looking "
             "for the product hidden inside how the tool is already being used.",
     "quotes": ["these are questions from an early adopter — explore the data and see if any "
                "examples emerge",
                "if we treat the use of OpenStory as a skill, what patterns emerge?"]},
    {"n": 6, "c": "red", "title": "Substrate", "period": "Week 5–10",
     "q": "build the next thing on the record",
     "body": "By now it's plumbing. Other projects pull context from it; the record becomes raw "
             "material for new features — and not reaching for it becomes the surprising choice.",
     "quotes": ["query OpenStory, get the context around our project, and pick up where we left off",
                "why are you not using openstory for this?"]},
]

THROUGHLINE = [
    ("The subject of “truth” widens",
     "Self (cost, quality) → a collaborator (are they live?) → the project (its own origin) → "
     "the market (what adopters need) → the next feature. One verb — recall faithfully — "
     "pointed at ever-widening circles."),
    ("The tool sinks from novelty to infrastructure",
     "Early on you invoke it by name. Later, agents reach for it unprompted — and the "
     "surprising question becomes why they didn't. Novelty → habit → assumed substrate."),
    ("The stakes rise with the work",
     "Playful early exploration becomes origin-story and live-deployment weight. The problem "
     "set matures as the project does."),
]

SYNTHESIS = ("A faithful, queryable record of agent work is the substrate for self-knowledge, "
             "trust, collaboration, narrative, and the product itself. It isn't reached for to "
             "monitor — it's reached for, again and again, to answer the only question memory "
             "ever answers: <em>what really happened?</em> — and each answer turns out to be "
             "load-bearing for something bigger.")


def render_html(conn: sqlite3.Connection) -> str:
    calls = openstory_calls(conn)
    prompts = user_prompts(conn)
    total = len(calls)
    hist = Counter(c["tool"] for c in calls)
    queries = [str(c["input"].get("query")) for c in calls if c["input"].get("query")]
    qhist = Counter(categorize_query(q) for q in queries)
    mentions = [p["text"] for p in prompts if "openstor" in p["text"].lower()]
    trig = triggering_prompts(calls, prompts)
    phist = Counter(categorize(t) for t in trig)
    da = deliberate_ambient(calls, prompts)

    def split_html() -> str:
        amb, dlb = da["ambient_pct"], 100 - da["ambient_pct"]
        bars = (f'<div class="splitrow"><span class="sl">ambient · agent-led</span>'
                f'<div class="track"><div class="fill" style="width:{amb}%"></div>'
                f'<span class="rn">{amb}%</span></div></div>'
                f'<div class="splitrow"><span class="sl">you asked directly</span>'
                f'<div class="track"><div class="fill alt" style="width:{dlb}%"></div>'
                f'<span class="rn">{dlb}%</span></div></div>')
        mx = da["intents"].most_common(1)[0][1] if da["intents"] else 1
        rows = "".join(
            f'<div class="irow"><span class="ilabel">{_esc(SHORT_LABEL.get(label, label))}</span>'
            f'<div class="ibar"><div class="ifill" style="width:{round(n / mx * 100)}%"></div></div>'
            f'<span class="in">{n}</span></div>'
            for label, n in da["intents"].most_common())
        return (f'<div class="splitwrap"><div class="caption">how calls happen</div>{bars}</div>'
                f'<div class="intents"><div class="caption">when you ask — for what</div>{rows}</div>')

    # recent unique verbatim queries
    recent_q, seen = [], set()
    for c in sorted(calls, key=lambda c: c["timestamp"], reverse=True):
        q = c["input"].get("query")
        if q and q not in seen:
            seen.add(q); recent_q.append(q)
        if len(recent_q) >= 18:
            break

    top_tool_share = round(sum(n for _, n in hist.most_common(3)) / total * 100) if total else 0

    def tool_rows() -> str:
        mx = hist.most_common(1)[0][1] if hist else 1
        out = []
        for tool, n in hist.most_common():
            w = round(n / mx * 100)
            out.append(
                f'<div class="row"><div class="rlabel">{_esc(tool)}</div>'
                f'<div class="track"><div class="fill" style="width:{w}%"></div>'
                f'<span class="rn">{n}</span></div>'
                f'<div class="rdesc">{_esc(TOOL_PURPOSE.get(tool, ""))}</div></div>')
        return "\n".join(out)

    def bucket_block(hist_c: Counter, source: list[str], catfn, samples: int = 2) -> str:
        mx = hist_c.most_common(1)[0][1] if hist_c else 1
        out = []
        for label, n in hist_c.most_common():
            w = round(n / mx * 100)
            samp = [s for s in source if catfn(s) == label][:samples]
            quotes = "".join(f'<div class="quote">“{_esc(_scrub(_oneline(s, 150)))}”</div>' for s in samp)
            out.append(
                f'<div class="bucket"><div class="brow">'
                f'<div class="bbar"><div class="bfill" style="width:{w}%"></div></div>'
                f'<div class="bn">{n}</div><div class="blabel">{_esc(label)}</div></div>'
                f'{quotes}</div>')
        return "\n".join(out)

    qcat_block = "".join(
        f'<div class="qcat"><span class="qn">{n}</span> {_esc(label)}</div>'
        for label, n in qhist.most_common())
    chips = "".join(f'<span class="chip">{_esc(_scrub(_oneline(q, 60)))}</span>' for q in recent_q)

    def acts_html() -> str:
        out = []
        for a in NARRATIVE:
            pulls = "".join(f'<div class="pull">“{_esc(_scrub(qt))}”</div>' for qt in a["quotes"])
            out.append(
                f'<li class="act {a["c"]}"><div class="node">{a["n"]}</div>'
                f'<div class="actbody"><div class="acthead">'
                f'<span class="acttitle">{_esc(a["title"])}</span>'
                f'<span class="period">{_esc(a["period"])}</span></div>'
                f'<div class="actq">“{_esc(a["q"])}”</div>'
                f'<p class="acttext">{_esc(a["body"])}</p>'
                f'<div class="pulls">{pulls}</div></div></li>')
        return "\n".join(out)

    throughline_html = "".join(
        f'<div class="mv"><div class="mvt">{_esc(t)}</div><p>{_esc(b)}</p></div>'
        for t, b in THROUGHLINE)

    return f"""<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>The story of the problem set — OpenStory</title>
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link href="https://fonts.googleapis.com/css2?family=Inter:wght@400;500;600;700&family=JetBrains+Mono:wght@400;500;600&display=swap" rel="stylesheet">
<style>
  :root {{
    --bg:#0c0d14; --bg2:#10111a; --panel:#14151f; --panel2:#1a1b26; --line:#232539;
    --ink:#c0caf5; --ink2:#a9b1d6; --dim:#7882a8;
    --blue:#7aa2f7; --cyan:#2ac3de; --green:#9ece6a; --purple:#bb9af7; --orange:#e0af68; --red:#f7768e;
    --radius:16px;
  }}
  * {{ box-sizing:border-box; }}
  body {{ margin:0; background:radial-gradient(1100px 520px at 78% -8%, #1a1b2655, transparent), var(--bg);
    color:var(--ink); font-family:Inter,ui-sans-serif,system-ui,sans-serif; -webkit-font-smoothing:antialiased; }}
  .wrap {{ max-width:920px; margin:0 auto; padding:56px 24px 90px; }}
  code, .mono {{ font-family:"JetBrains Mono",ui-monospace,Menlo,monospace; }}
  .eyebrow {{ font:600 12px/1 "JetBrains Mono",monospace; letter-spacing:.18em; text-transform:uppercase; color:var(--blue); margin:0 0 14px; }}
  header h1 {{ font-size:40px; line-height:1.08; letter-spacing:-.025em; margin:0 0 12px; font-weight:700; }}
  header h1 .grad {{ background:linear-gradient(90deg,var(--blue),var(--cyan)); -webkit-background-clip:text; background-clip:text; color:transparent; }}
  header .sub {{ color:var(--ink2); font-size:17px; line-height:1.5; max-width:64ch; margin:0; }}
  header .sub em {{ color:var(--cyan); font-style:italic; }}
  header .meta {{ color:var(--dim); font:500 13px/1.5 "JetBrains Mono",monospace; margin-top:12px; }}
  .stats {{ display:grid; grid-template-columns:repeat(4,1fr); gap:14px; margin:34px 0 6px; }}
  .stat {{ background:var(--panel); border:1px solid var(--line); border-radius:14px; padding:18px 16px; }}
  .stat .num {{ font-size:30px; font-weight:700; letter-spacing:-.02em; }}
  .stat .lbl {{ color:var(--dim); font-size:12.5px; margin-top:4px; }}
  section {{ margin-top:52px; }}
  h2 {{ font:600 12px/1 "JetBrains Mono",monospace; letter-spacing:.16em; text-transform:uppercase; color:var(--blue); margin:0 0 6px; }}
  h2.alt {{ color:var(--cyan); }}
  .lead {{ color:var(--dim); margin:0 0 20px; font-size:15px; line-height:1.55; max-width:70ch; }}
  .panel {{ background:var(--panel); border:1px solid var(--line); border-radius:var(--radius); padding:20px 22px; }}
  /* timeline */
  .timeline {{ list-style:none; margin:0; padding:0; position:relative; }}
  .timeline::before {{ content:""; position:absolute; left:19px; top:10px; bottom:30px; width:2px; background:linear-gradient(var(--line),transparent); }}
  .act {{ position:relative; display:grid; grid-template-columns:40px 1fr; gap:20px; padding:4px 0 30px; }}
  .act .node {{ width:40px; height:40px; border-radius:50%; display:grid; place-items:center;
    font:700 16px/1 "JetBrains Mono",monospace; background:var(--panel2); border:2px solid var(--ac);
    color:var(--ac); box-shadow:0 0 0 5px var(--bg); z-index:1; }}
  .act.blue {{ --ac:var(--blue); }} .act.cyan {{ --ac:var(--cyan); }} .act.purple {{ --ac:var(--purple); }}
  .act.green {{ --ac:var(--green); }} .act.orange {{ --ac:var(--orange); }} .act.red {{ --ac:var(--red); }}
  .acthead {{ display:flex; align-items:baseline; gap:12px; flex-wrap:wrap; }}
  .acttitle {{ font-size:21px; font-weight:700; letter-spacing:-.01em; }}
  .period {{ font:500 12px/1 "JetBrains Mono",monospace; color:var(--dim); border:1px solid var(--line); border-radius:999px; padding:4px 9px; }}
  .actq {{ color:var(--ac); font-size:14px; font-style:italic; margin:7px 0 10px; }}
  .acttext {{ color:var(--ink2); margin:0 0 14px; font-size:15px; line-height:1.6; }}
  .pulls {{ display:flex; flex-direction:column; gap:8px; }}
  .pull {{ border-left:2px solid var(--ac); background:var(--bg2); border-radius:0 8px 8px 0;
    padding:9px 14px; color:var(--dim); font-size:13.5px; font-style:italic; }}
  /* throughline + synthesis */
  .movements {{ display:grid; gap:14px; }}
  .mv {{ background:var(--panel); border:1px solid var(--line); border-left:3px solid var(--purple); border-radius:12px; padding:16px 18px; }}
  .mvt {{ font-weight:600; font-size:15.5px; margin-bottom:5px; }}
  .mv p {{ margin:0; color:var(--dim); font-size:14px; line-height:1.55; }}
  .synthesis {{ margin-top:18px; background:linear-gradient(180deg,var(--panel2),var(--panel)); border:1px solid var(--line);
    border-radius:var(--radius); padding:26px 28px; font-size:18px; line-height:1.55; color:var(--ink); }}
  .synthesis em {{ color:var(--cyan); font-style:italic; }}
  /* data viz */
  .row {{ display:grid; grid-template-columns:160px 1fr; gap:6px 14px; align-items:center; padding:8px 0; border-top:1px solid var(--line); }}
  .row:first-child {{ border-top:none; }}
  .rlabel {{ font-family:"JetBrains Mono",monospace; font-size:13px; color:var(--ink2); }}
  .track {{ position:relative; background:var(--bg2); border-radius:8px; height:24px; overflow:hidden; }}
  .fill {{ height:100%; background:linear-gradient(90deg,var(--blue),var(--cyan)); border-radius:8px; }}
  .rn {{ position:absolute; right:8px; top:50%; transform:translateY(-50%); font:500 12px/1 "JetBrains Mono",monospace; color:#dfe6ff; }}
  .rdesc {{ grid-column:2; color:var(--dim); font-size:12.5px; margin-top:-2px; }}
  .bucket {{ padding:10px 0; border-top:1px solid var(--line); }} .bucket:first-child {{ border-top:none; }}
  .brow {{ display:grid; grid-template-columns:130px 32px 1fr; gap:12px; align-items:center; }}
  .bbar {{ background:var(--bg2); border-radius:6px; height:10px; overflow:hidden; }}
  .bfill {{ height:100%; background:linear-gradient(90deg,var(--orange),var(--cyan)); }}
  .bn {{ font:600 14px/1 "JetBrains Mono",monospace; }} .blabel {{ font-size:14px; }}
  .quote {{ color:var(--dim); font-size:13px; margin:6px 0 0 142px; font-style:italic; }}
  .qcats {{ display:flex; flex-wrap:wrap; gap:8px; margin-bottom:16px; }}
  .qcat {{ background:var(--panel2); border:1px solid var(--line); border-radius:999px; padding:5px 12px; font-size:13px; color:var(--dim); }}
  .qcat .qn {{ color:var(--ink); font-weight:600; }}
  .chips {{ display:flex; flex-wrap:wrap; gap:8px; }}
  .chip {{ background:var(--bg2); border:1px solid var(--line); border-radius:8px; padding:6px 10px; font-family:"JetBrains Mono",monospace; font-size:12px; color:#9db4f0; }}
  .cards {{ display:grid; grid-template-columns:1fr 1fr; gap:16px; }}
  .card {{ background:linear-gradient(180deg,var(--panel2),var(--panel)); border:1px solid var(--line); border-radius:var(--radius); padding:20px; }}
  .card .big {{ font-size:20px; font-weight:700; margin-bottom:6px; letter-spacing:-.01em; }}
  .card p {{ color:var(--dim); margin:0; font-size:13.5px; line-height:1.55; }}
  .muted {{ color:var(--dim); }}
  /* ambient vs deliberate split */
  .panel.split {{ display:grid; grid-template-columns:1fr 1fr; gap:28px; }}
  .caption {{ font:600 11px/1 "JetBrains Mono",monospace; letter-spacing:.14em; text-transform:uppercase; color:var(--dim); margin-bottom:14px; }}
  .splitrow {{ display:grid; grid-template-columns:120px 1fr; gap:12px; align-items:center; margin-bottom:12px; }}
  .splitrow .sl {{ font-size:13px; color:var(--ink2); }}
  .fill.alt {{ background:linear-gradient(90deg,var(--green),var(--cyan)); }}
  .irow {{ display:grid; grid-template-columns:96px 1fr 24px; gap:10px; align-items:center; margin-bottom:9px; }}
  .ilabel {{ font-size:13px; color:var(--ink2); }}
  .ibar {{ background:var(--bg2); border-radius:6px; height:12px; overflow:hidden; }}
  .ifill {{ height:100%; background:linear-gradient(90deg,var(--cyan),var(--blue)); }}
  .in {{ font:600 12px/1 "JetBrains Mono",monospace; color:var(--ink2); text-align:right; }}
  @media (max-width:680px) {{ .panel.split {{ grid-template-columns:1fr; gap:22px; }} }}
  .draftnote {{ margin:26px 0 0; padding:12px 16px; border:1px dashed var(--line); border-radius:10px; color:var(--dim); font-size:12.5px; background:var(--bg2); }}
  footer {{ margin-top:30px; color:var(--dim); font:500 12px/1.6 "JetBrains Mono",monospace; text-align:center; }}
  @media (max-width:680px) {{ .stats,.cards {{ grid-template-columns:1fr 1fr; }} header h1 {{ font-size:30px; }}
    .row {{ grid-template-columns:1fr; }} .quote {{ margin-left:0; }} .brow {{ grid-template-columns:84px 28px 1fr; }} }}
</style></head>
<body><div class="wrap">
  <header>
    <p class="eyebrow">OpenStory · draft</p>
    <h1>The story of the <span class="grad">problem set</span></h1>
    <p class="sub">What OpenStory has actually been used to solve — read from its own event store. Every problem turned out to be the same one wearing different clothes: <em>what really happened?</em></p>
    <p class="meta">{total} openstory calls · {len(prompts):,} prompts · {len(queries)} searches · Apr–Jun 2026</p>
  </header>

  <div class="stats">
    <div class="stat"><div class="num">{total}</div><div class="lbl">openstory MCP calls</div></div>
    <div class="stat"><div class="num">6</div><div class="lbl">acts in the arc</div></div>
    <div class="stat"><div class="num">{da["ambient_pct"]}%</div><div class="lbl">calls were agent-led</div></div>
    <div class="stat"><div class="num">{da["deliberate"]}</div><div class="lbl">deliberate asks</div></div>
  </div>

  <section>
    <h2>The arc</h2>
    <p class="lead">Ten weeks, six problems. The intent buckets dissolve when you read the prompts in order — they climb a ladder from self-knowledge to infrastructure.</p>
    <ol class="timeline">{acts_html()}</ol>
  </section>

  <section>
    <h2 class="alt">The throughline</h2>
    <p class="lead">Three movements run under all six acts.</p>
    <div class="movements">{throughline_html}</div>
    <div class="synthesis">{SYNTHESIS}</div>
  </section>

  <section>
    <h2>Evidence · agent lens</h2>
    <p class="lead">Which tools agents call — and the question each answers. Three are two-thirds of all usage: <strong>find something → zoom into one session → list what's recent.</strong></p>
    <div class="panel">{tool_rows()}</div>
  </section>

  <section>
    <h2>Evidence · query lens</h2>
    <p class="lead">{len(queries)} verbatim search terms — mostly multi-word <em>concept</em> searches, agents using the store as associative memory across sessions.</p>
    <div class="panel"><div class="qcats">{qcat_block}</div><div class="chips">{chips}</div></div>
  </section>

  <section>
    <h2>Evidence · who's asking</h2>
    <p class="lead">Most calls aren't requested — <strong>{da["ambient_pct"]}%</strong> trace back to an ordinary task prompt where an agent reached for OpenStory on its own. The rest are deliberate asks; here's what they want. <span class="muted">(Continuations and harness noise excluded, not bucketed.)</span></p>
    <div class="panel split">{split_html()}</div>
  </section>

  <section>
    <h2>What it means</h2>
    <div class="cards">
      <div class="card"><div class="big">Memory, not logs</div><p>The questions are "what have I done / what did it cost / find that thing." Agents use the same store to ground themselves before acting. It's the one place that remembers every session at once.</p></div>
      <div class="card"><div class="big">Dogfooding works</div><p>Most calls aren't asked for — agents follow the "use OpenStory, don't grep .jsonl" instruction autonomously. The product's first power user is the agent.</p></div>
      <div class="card"><div class="big">A thin working set</div><p><code>search</code>, <code>session_synopsis</code>, <code>list_sessions</code> dominate. The analytical tools — file impact, productivity, patterns — are barely touched.</p></div>
      <div class="card"><div class="big">An opportunity</div><p>Those unused tools are either undiscoverable or not yet useful. Surfacing them — or folding their signal into the common path — is low-hanging fruit.</p></div>
    </div>
  </section>

  <div class="draftnote">Draft · incubating for openstory.work — not yet live. Collaborator names redacted to roles. Generated from the live event store by <span class="mono">scripts/openstory_self_usage.py --html</span>.</div>
  <footer>OpenStory eats its own cooking.</footer>
</div></body></html>"""


# -- Tests ------------------------------------------------------------

def run_tests() -> None:
    print("Running tests...")
    conn = sqlite3.connect(":memory:")
    conn.row_factory = sqlite3.Row
    conn.executescript("""
        CREATE TABLE events (id TEXT PRIMARY KEY, session_id TEXT, subtype TEXT, timestamp TEXT, payload TEXT);
    """)
    def raw_tool(name, inp):
        return json.dumps({"data": {"raw": {"message": {"content": [
            {"type": "text", "text": "hmm"},
            {"type": "tool_use", "name": name, "input": inp},
        ]}}}})
    rows = [
        ("1", "s1", "message.assistant.tool_use", "t02", raw_tool("mcp__openstory__search", {"query": "auth error"})),
        ("2", "s1", "message.assistant.tool_use", "t03", raw_tool("mcp__openstory-remote__search", {"query": "nats leaf"})),
        ("3", "s1", "message.assistant.tool_use", "t04", raw_tool("mcp__openstory__token_usage", {})),
        # ToolSearch select noise — must be excluded even though string matches
        ("4", "s1", "message.assistant.tool_use", "t05",
         json.dumps({"data": {"raw": {"message": {"content": [
             {"type": "tool_use", "name": "ToolSearch", "input": {"query": "select:mcp__openstory__search"}}]}}}})),
        ("5", "s1", "message.user.prompt", "t01", json.dumps({"data": {"text": "search the sessions for the auth error"}})),
        ("6", "s2", "message.user.prompt", "t10", json.dumps({"data": {"text": "how many tokens did we burn"}})),
        ("7", "s2", "message.assistant.tool_use", "t11", raw_tool("mcp__openstory__token_usage", {})),
    ]
    conn.executemany("INSERT INTO events VALUES (?,?,?,?,?)", rows)

    calls = openstory_calls(conn)
    tools = Counter(c["tool"] for c in calls)
    assert tools["search"] == 2, tools          # local + remote folded
    assert tools["token_usage"] == 2, tools
    assert "ToolSearch" not in tools
    assert len(calls) == 4, calls                # noise excluded
    print("  OK: parses real tool_use blocks, folds namespaces, drops ToolSearch noise")

    qs = [c["input"]["query"] for c in calls if c["input"].get("query")]
    assert set(qs) == {"auth error", "nats leaf"}, qs
    print("  OK: extracts verbatim search queries from raw input")

    prompts = user_prompts(conn)
    trig = triggering_prompts(calls, prompts)
    # s1: three calls all trace to the one preceding prompt (deduped) -> 1
    # s2: one call -> its prompt
    assert len(trig) == 2, trig
    assert any("auth error" in t for t in trig)
    assert any("tokens" in t for t in trig)
    print("  OK: maps each call to nearest preceding prompt, deduped")

    assert categorize("how many tokens did we burn") == "token / cost"
    assert categorize("search the sessions for the auth error") == "search / find across sessions"
    assert categorize("what happened in that session") == "what happened / summarize a session"
    assert categorize_query("auth error") == "error / failure"
    assert categorize_query("nats leaf") == "a feature / subsystem"
    print("  OK: intent + query categorizers")

    assert _is_noise("<system-reminder>do thing</system-reminder>")
    assert _is_noise("This session is being continued from a previous conversation")
    assert _is_noise("   ")
    assert not _is_noise("how many tokens did we burn")
    da = deliberate_ambient(calls, prompts)
    assert da["ambient"] == 0 and da["deliberate"] == 2, da
    assert da["intents"]["token / cost"] == 1, da
    print("  OK: deliberate/ambient split + noise filter")

    html = render_html(conn)
    assert html.startswith("<!doctype html>") and html.rstrip().endswith("</html>")
    assert "openstory MCP calls" in html and "token_usage" in html
    assert "<script" not in html.lower(), "report must be static — no scripts"
    print("  OK: HTML report renders, self-contained, no scripts")

    conn.close()
    print("\nAll tests passed.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--data-dir", default="./data")
    parser.add_argument("--html", metavar="PATH", help="write an HTML report to PATH instead of text")
    parser.add_argument("--story", action="store_true", help="dump the verbatim prompt/query corpus, chronological")
    parser.add_argument("--test", action="store_true")
    args = parser.parse_args()

    if args.test:
        run_tests()
        sys.exit(0)

    conn = connect(Path(args.data_dir) / "open-story.db")
    if args.story:
        print_story(conn)
    elif args.html:
        Path(args.html).write_text(render_html(conn), encoding="utf-8")
        print(f"Wrote {args.html}")
    else:
        print_report(conn)
    conn.close()
