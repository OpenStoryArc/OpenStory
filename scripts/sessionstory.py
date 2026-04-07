"""Tell the story of an OpenStory session as a deterministic fact sheet.

The script does NOT narrate. It collects facts. Narration is the agent's job —
the fact sheet is the input the agent reads to write a story.

Three API calls (sessions, records, patterns), one structured output:
- shape: records, turns, duration, tool histogram
- patterns: pattern-type counts, turn.phase mix, sample turn.sentence strings
- arc: top-level user prompts in time order, lightly filtered
- unfinished: trailing assistant messages — what was in flight at session end

Companion to scripts/daystory.sh (day-scoped, narrative-style). This one is
session-scoped and emits structured facts for the agent to interpret.

Usage:
    python3 scripts/sessionstory.py SESSION_ID            # markdown report
    python3 scripts/sessionstory.py latest                # most recent session
    python3 scripts/sessionstory.py SESSION_ID --json     # machine-readable
    python3 scripts/sessionstory.py SESSION_ID --brief    # shape + arc only
    python3 scripts/sessionstory.py SESSION_ID --unfinished  # include trailing messages
    python3 scripts/sessionstory.py --list                # list recent sessions
    python3 scripts/sessionstory.py --test                # run self-tests
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from collections import Counter
from dataclasses import asdict, dataclass, field
from datetime import datetime


DEFAULT_URL = "http://localhost:3002"

NOISE_PREFIXES = (
    "<task-notification>",
    "<command-name>",
    "<local-command-",
    "[Image: source:",
    "[Request interrupted",
)


# -- HTTP -------------------------------------------------------------

def fetch(base_url: str, path: str) -> object:
    url = f"{base_url}{path}"
    try:
        with urllib.request.urlopen(url, timeout=30) as resp:
            return json.loads(resp.read())
    except urllib.error.URLError as e:
        sys.stderr.write(f"error: failed to fetch {url}: {e}\n")
        sys.exit(2)


# -- Data models ------------------------------------------------------

@dataclass
class PromptLine:
    timestamp: str  # ISO
    content: str    # truncated to ~200 chars

    def hh_mm(self) -> str:
        return self.timestamp[11:16] if len(self.timestamp) >= 16 else self.timestamp


@dataclass
class SessionFacts:
    session_id: str
    started_at: str
    ended_at: str
    duration_hours: float

    total_records: int
    record_type_counts: dict[str, int]
    tool_call_counts: dict[str, int]
    turn_count: int
    sidechain_count: int

    opening_prompt: PromptLine | None
    closing_prompt: PromptLine | None
    prompt_timeline: list[PromptLine]

    pattern_total: int
    pattern_type_counts: dict[str, int]
    turn_phase_counts: dict[str, int]
    sample_sentences: list[str]
    error_recovery_count: int
    test_cycle_count: int

    trailing_assistant: list[PromptLine] = field(default_factory=list)


# -- Pure aggregation -------------------------------------------------

def is_noise(content: str) -> bool:
    if not content:
        return True
    return any(content.lstrip().startswith(p) for p in NOISE_PREFIXES)


def extract_text(payload: object) -> str:
    """Pull a string out of an assistant or user payload, tolerant of shape."""
    if isinstance(payload, str):
        return payload
    if not isinstance(payload, dict):
        return ""
    c = payload.get("content", "")
    if isinstance(c, str):
        return c
    if isinstance(c, list):
        parts: list[str] = []
        for block in c:
            if isinstance(block, dict):
                t = block.get("text") or block.get("content")
                if isinstance(t, str):
                    parts.append(t)
        return " ".join(parts)
    return ""


def parse_iso(ts: str) -> datetime | None:
    if not ts:
        return None
    try:
        return datetime.fromisoformat(ts.replace("Z", "+00:00"))
    except ValueError:
        return None


def summarize(records: list[dict], patterns: list[dict], session_id: str) -> SessionFacts:
    record_types: Counter[str] = Counter()
    tool_calls: Counter[str] = Counter()
    sidechain = 0
    turns = 0
    user_prompts: list[PromptLine] = []
    trailing_assistants: list[PromptLine] = []

    for r in records:
        rt = r.get("record_type", "")
        record_types[rt] += 1
        if r.get("is_sidechain"):
            sidechain += 1
        if rt == "turn_end":
            turns += 1
        elif rt == "tool_call":
            tool_calls[(r.get("payload") or {}).get("name", "?")] += 1
        elif rt == "user_message":
            text = extract_text(r.get("payload"))
            if not is_noise(text):
                user_prompts.append(
                    PromptLine(timestamp=r.get("timestamp", ""), content=text[:200])
                )
        elif rt == "assistant_message":
            text = extract_text(r.get("payload"))
            if text:
                trailing_assistants.append(
                    PromptLine(timestamp=r.get("timestamp", ""), content=text[:300])
                )

    started_at = records[0].get("timestamp", "") if records else ""
    ended_at = records[-1].get("timestamp", "") if records else ""
    t0, t1 = parse_iso(started_at), parse_iso(ended_at)
    duration_hours = round((t1 - t0).total_seconds() / 3600, 2) if t0 and t1 else 0.0

    pattern_types: Counter[str] = Counter()
    phase_mix: Counter[str] = Counter()
    sample_sentences: list[str] = []
    error_recovery = 0
    test_cycle = 0

    for p in patterns:
        pt = p.get("pattern_type", "")
        pattern_types[pt] += 1
        if pt == "turn.phase":
            phase = (p.get("metadata") or {}).get("phase")
            if phase:
                phase_mix[phase] += 1
        elif pt == "turn.sentence":
            if len(sample_sentences) < 8:
                summary = p.get("summary", "")
                if summary:
                    sample_sentences.append(summary)
        elif pt == "error.recovery":
            error_recovery += 1
        elif pt == "test.cycle":
            test_cycle += 1

    opening = user_prompts[0] if user_prompts else None
    closing = user_prompts[-1] if user_prompts else None

    return SessionFacts(
        session_id=session_id,
        started_at=started_at,
        ended_at=ended_at,
        duration_hours=duration_hours,
        total_records=len(records),
        record_type_counts=dict(record_types.most_common()),
        tool_call_counts=dict(tool_calls.most_common()),
        turn_count=turns,
        sidechain_count=sidechain,
        opening_prompt=opening,
        closing_prompt=closing,
        prompt_timeline=user_prompts,
        pattern_total=len(patterns),
        pattern_type_counts=dict(pattern_types.most_common()),
        turn_phase_counts=dict(phase_mix.most_common()),
        sample_sentences=sample_sentences,
        error_recovery_count=error_recovery,
        test_cycle_count=test_cycle,
        trailing_assistant=trailing_assistants[-6:],
    )


# -- Formatting -------------------------------------------------------

def fmt_md(f: SessionFacts, brief: bool = False, include_trailing: bool = False) -> str:
    out: list[str] = []
    out.append(f"# Session {f.session_id}")
    out.append("")
    out.append(f"- **Started:** {f.started_at}")
    out.append(f"- **Ended:** {f.ended_at}")
    out.append(f"- **Duration:** {f.duration_hours} h")
    out.append(f"- **Records:** {f.total_records}")
    out.append(f"- **Turns:** {f.turn_count}")
    out.append(f"- **Sidechain records:** {f.sidechain_count}")
    out.append("")

    out.append("## Record types")
    for k, v in f.record_type_counts.items():
        out.append(f"- {k}: {v}")
    out.append("")

    out.append("## Tool calls")
    for k, v in f.tool_call_counts.items():
        out.append(f"- {k}: {v}")
    out.append("")

    if not brief:
        out.append("## Patterns")
        out.append(f"Total: {f.pattern_total}")
        for k, v in f.pattern_type_counts.items():
            out.append(f"- {k}: {v}")
        out.append("")

        if f.turn_phase_counts:
            out.append("## Turn phases")
            for k, v in f.turn_phase_counts.items():
                out.append(f"- {k}: {v}")
            out.append("")

        if f.sample_sentences:
            out.append("## Sample sentences (verbatim from detector)")
            for s in f.sample_sentences:
                out.append(f"- {s}")
            out.append("")

    out.append("## Prompt timeline")
    if f.opening_prompt:
        out.append(f"- **Opening** [{f.opening_prompt.hh_mm()}] {f.opening_prompt.content}")
    for p in f.prompt_timeline[1:-1] if len(f.prompt_timeline) > 2 else []:
        out.append(f"- [{p.hh_mm()}] {p.content}")
    if f.closing_prompt and f.closing_prompt is not f.opening_prompt:
        out.append(f"- **Closing** [{f.closing_prompt.hh_mm()}] {f.closing_prompt.content}")
    out.append("")

    if include_trailing and f.trailing_assistant:
        out.append("## Trailing assistant messages (what was in flight)")
        for m in f.trailing_assistant:
            out.append(f"- [{m.hh_mm()}] {m.content}")
        out.append("")

    return "\n".join(out)


def fmt_json(f: SessionFacts) -> str:
    return json.dumps(asdict(f), indent=2, default=str)


# -- Session resolution -----------------------------------------------

def resolve_session_id(base_url: str, arg: str) -> str:
    if arg and arg != "latest":
        return arg
    sessions = fetch(base_url, "/api/sessions")
    if not isinstance(sessions, list) or not sessions:
        sys.stderr.write("error: no sessions found\n")
        sys.exit(2)
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    return sessions[0].get("session_id", "")


def list_sessions(base_url: str, limit: int = 10) -> None:
    sessions = fetch(base_url, "/api/sessions")
    if not isinstance(sessions, list):
        return
    sessions.sort(key=lambda s: s.get("start_time", ""), reverse=True)
    for s in sessions[:limit]:
        sid = s.get("session_id", "")
        start = s.get("start_time", "")
        print(f"{start}  {sid}")


def run(base_url: str, session_id: str, as_json: bool, brief: bool, unfinished: bool) -> None:
    records = fetch(base_url, f"/api/sessions/{session_id}/records")
    patterns_resp = fetch(base_url, f"/api/sessions/{session_id}/patterns")
    if isinstance(patterns_resp, dict):
        patterns = patterns_resp.get("patterns", [])
    else:
        patterns = patterns_resp or []
    if not isinstance(records, list):
        sys.stderr.write("error: records endpoint did not return a list\n")
        sys.exit(2)

    facts = summarize(records, patterns, session_id)
    if as_json:
        print(fmt_json(facts))
    else:
        print(fmt_md(facts, brief=brief, include_trailing=unfinished))


# -- Self-tests -------------------------------------------------------

def _fixture_records() -> list[dict]:
    return [
        {"record_type": "user_message", "timestamp": "2026-04-06T10:00:00Z",
         "payload": {"content": "let's build a thing"}},
        {"record_type": "assistant_message", "timestamp": "2026-04-06T10:00:05Z",
         "payload": {"content": [{"text": "ok, planning..."}]}},
        {"record_type": "tool_call", "timestamp": "2026-04-06T10:00:10Z",
         "payload": {"name": "Bash"}},
        {"record_type": "tool_result", "timestamp": "2026-04-06T10:00:11Z", "payload": {}},
        {"record_type": "tool_call", "timestamp": "2026-04-06T10:00:12Z",
         "payload": {"name": "Read"}},
        {"record_type": "tool_result", "timestamp": "2026-04-06T10:00:13Z", "payload": {}},
        {"record_type": "user_message", "timestamp": "2026-04-06T10:05:00Z",
         "payload": {"content": "<task-notification>noise</task-notification>"}},
        {"record_type": "user_message", "timestamp": "2026-04-06T10:06:00Z",
         "payload": {"content": "looks good, ship it"}},
        {"record_type": "turn_end", "timestamp": "2026-04-06T10:06:30Z", "payload": {}},
        {"record_type": "assistant_message", "timestamp": "2026-04-06T10:07:00Z",
         "payload": {"content": "shipped."}},
    ]


def _fixture_patterns() -> list[dict]:
    return [
        {"pattern_type": "turn.sentence", "summary": "Claude ran 2 tools because 'let's build a thing' → answered"},
        {"pattern_type": "turn.phase", "metadata": {"phase": "implementation"}},
        {"pattern_type": "turn.phase", "metadata": {"phase": "conversation"}},
        {"pattern_type": "eval_apply.eval"},
        {"pattern_type": "eval_apply.apply"},
        {"pattern_type": "error.recovery"},
        {"pattern_type": "test.cycle"},
    ]


def selftest() -> int:
    failures = 0

    def check(name: str, cond: bool, detail: str = "") -> None:
        nonlocal failures
        if cond:
            print(f"  ok   {name}")
        else:
            failures += 1
            print(f"  FAIL {name}: {detail}")

    print("== summarize() ==")
    f = summarize(_fixture_records(), _fixture_patterns(), "test-session")

    check("session_id propagated", f.session_id == "test-session")
    check("total_records", f.total_records == 10, str(f.total_records))
    check("turn_count", f.turn_count == 1, str(f.turn_count))
    check("tool histogram", f.tool_call_counts == {"Bash": 1, "Read": 1}, str(f.tool_call_counts))
    check("noise filtered", all("task-notification" not in p.content for p in f.prompt_timeline))
    check("opening prompt", f.opening_prompt is not None and "build a thing" in f.opening_prompt.content)
    check("closing prompt", f.closing_prompt is not None and "ship it" in f.closing_prompt.content)
    check("two distinct prompts after filter", len(f.prompt_timeline) == 2, str(len(f.prompt_timeline)))
    check("pattern total", f.pattern_total == 7)
    check("phase mix", f.turn_phase_counts == {"implementation": 1, "conversation": 1})
    check("sample sentence captured", len(f.sample_sentences) == 1)
    check("error recovery", f.error_recovery_count == 1)
    check("test cycle", f.test_cycle_count == 1)
    check("trailing assistant captured", len(f.trailing_assistant) >= 1)
    check("duration computed", f.duration_hours >= 0)

    print()
    print("== fmt_md() ==")
    md = fmt_md(f, include_trailing=True)
    check("md has session header", "# Session test-session" in md)
    check("md lists tools", "Bash: 1" in md and "Read: 1" in md)
    check("md has trailing", "shipped." in md)

    print()
    print("== fmt_json() ==")
    js = fmt_json(f)
    parsed = json.loads(js)
    check("json round-trip", parsed["session_id"] == "test-session")
    check("json prompt_timeline list", isinstance(parsed["prompt_timeline"], list))

    print()
    print("== extract_text edge cases ==")
    check("extract_text dict-content", extract_text({"content": "hi"}) == "hi")
    check("extract_text list-of-blocks",
          extract_text({"content": [{"text": "a"}, {"text": "b"}]}) == "a b")
    check("extract_text empty", extract_text({}) == "")
    check("extract_text non-dict", extract_text(None) == "")

    print()
    print("== is_noise ==")
    check("noise: task-notification", is_noise("<task-notification>x"))
    check("noise: command-name", is_noise("<command-name>/compact"))
    check("noise: image ref", is_noise("[Image: source: /tmp/x.png]"))
    check("not noise: real prompt", not is_noise("can you check the data?"))

    print()
    if failures:
        print(f"FAILED: {failures}")
        return 1
    print("all tests passed")
    return 0


# -- CLI --------------------------------------------------------------

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("session_id", nargs="?", help="session id, or 'latest'")
    parser.add_argument("--url", default=DEFAULT_URL, help="OpenStory API base URL")
    parser.add_argument("--json", action="store_true", help="emit JSON instead of markdown")
    parser.add_argument("--brief", action="store_true", help="shape + prompts only")
    parser.add_argument("--unfinished", action="store_true",
                        help="include trailing assistant messages")
    parser.add_argument("--list", action="store_true", help="list recent sessions and exit")
    parser.add_argument("--test", action="store_true", help="run self-tests and exit")
    args = parser.parse_args()

    if args.test:
        sys.exit(selftest())
    if args.list:
        list_sessions(args.url)
        return
    if not args.session_id:
        parser.error("session_id required (or use --list / --test)")

    sid = resolve_session_id(args.url, args.session_id)
    run(args.url, sid, as_json=args.json, brief=args.brief, unfinished=args.unfinished)


if __name__ == "__main__":
    main()
