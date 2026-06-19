#!/usr/bin/env python3
"""Side-by-side compare of Claude Code's two observation channels.

For one session, print:
  1. What OTel carries that the JSONL doesn't (the structural sidecar).
  2. What the JSONL carries that OTel doesn't (the transcript).
  3. The shared join keys and per-tool-call alignment.

The point is empirical: the BACKLOG entry "Cowork OTel exporter as second
observation channel" hypothesizes a clean split between transcript and
sidecar. This script tests that hypothesis on real data.

Usage:
    python3 scripts/otel_vs_jsonl.py SESSION_ID
    python3 scripts/otel_vs_jsonl.py --list           # show captured sessions
    python3 scripts/otel_vs_jsonl.py --test           # synthetic-fixture smoke test
    python3 scripts/otel_vs_jsonl.py SESSION_ID --json  # raw JSON for piping

Conventions:
  - OTel capture: captures/otel-lab/data/otel-capture.jsonl (file exporter).
  - JSONL transcript: ~/.claude/projects/<encoded-cwd>/<session_id>.jsonl
    (also discovers Cowork sessions under
    ~/Library/Application Support/Claude/local-agent-mode-sessions/).
"""

from __future__ import annotations

import argparse
import json
import pathlib
import sys
from collections import Counter
from dataclasses import dataclass, field
from typing import Iterator

REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
DEFAULT_OTEL = REPO_ROOT / "captures" / "otel-lab" / "data" / "otel-capture.jsonl"

CLAUDE_PROJECTS = pathlib.Path.home() / ".claude" / "projects"
COWORK_ROOT = (
    pathlib.Path.home()
    / "Library"
    / "Application Support"
    / "Claude"
    / "local-agent-mode-sessions"
)


# ── OTLP helpers ─────────────────────────────────────────────────────


def _otlp_attrs(obj: dict) -> dict:
    """Flatten OTLP's `[{key, value: {stringValue: ...}}, ...]` into a dict."""
    out: dict = {}
    for a in obj.get("attributes", []):
        v = a.get("value", {})
        out[a["key"]] = next(iter(v.values())) if v else None
    return out


def _otlp_envelopes(otel_path: pathlib.Path) -> Iterator[dict]:
    """Yield each OTLP envelope from a file-exporter JSONL capture."""
    with otel_path.open() as f:
        for line in f:
            line = line.strip()
            if line:
                yield json.loads(line)


@dataclass
class OtelSession:
    session_id: str
    metrics: dict = field(default_factory=dict)
    events_by_name: Counter = field(default_factory=Counter)
    tool_results: list = field(default_factory=list)
    spans_by_name: Counter = field(default_factory=Counter)
    prompts: list = field(default_factory=list)
    # Set when OTEL_LOG_RAW_API_BODIES=file:<dir> is on. Each entry is the
    # event's attrs (carries `body_ref` plus model + token usage); resolve
    # the pointer with `_load_body_ref()` to see what shipped on the wire.
    api_request_bodies: list = field(default_factory=list)
    api_response_bodies: list = field(default_factory=list)
    # Set when OTEL_LOG_TOOL_CONTENT=1 is on. Span events attached to
    # `claude_code.tool` carrying the tool's input + output content
    # (60KB cap per attribute). Joined on `tool_use_id` from the parent span.
    tool_output_events: list = field(default_factory=list)


def _load_body_ref(ref: str) -> dict | None:
    """Resolve a `body_ref` pointer to a `{size, model, messages, tools}` summary.

    Returns None when the file is missing — bodies live in a sidecar dir, so
    moving the OTel capture without the dir is a supported case to detect.
    """
    p = pathlib.Path(ref)
    if not p.exists():
        return {"ref": ref, "missing": True}
    size = p.stat().st_size
    try:
        body = json.loads(p.read_text())
    except json.JSONDecodeError:
        return {"ref": ref, "size_bytes": size, "parse_error": True}
    summary: dict = {"ref": ref, "size_bytes": size}
    if isinstance(body, dict):
        if "model" in body:
            summary["model"] = body["model"]
        if isinstance(body.get("messages"), list):
            summary["message_turns"] = len(body["messages"])
        if isinstance(body.get("tools"), list):
            summary["tool_schemas"] = len(body["tools"])
        if isinstance(body.get("system"), (str, list)):
            sys_val = body["system"]
            summary["system_chars"] = (
                len(sys_val) if isinstance(sys_val, str)
                else sum(len(b.get("text", "")) for b in sys_val if isinstance(b, dict))
            )
    return summary


def otel_sessions(otel_path: pathlib.Path) -> dict[str, OtelSession]:
    """Group OTLP envelopes by session.id.

    `session.id` is a per-record attribute in Claude Code's exporter
    (data points for metrics, log records for events, spans for traces) —
    not a resource attribute — so we read it off each record.
    """
    sessions: dict[str, OtelSession] = {}

    def get(sid: str) -> OtelSession:
        if sid not in sessions:
            sessions[sid] = OtelSession(session_id=sid)
        return sessions[sid]

    for env in _otlp_envelopes(otel_path):
        for rm in env.get("resourceMetrics", []):
            for sm in rm.get("scopeMetrics", []):
                for m in sm.get("metrics", []):
                    name = m.get("name", "")
                    points = (
                        m.get("sum", {}).get("dataPoints", [])
                        or m.get("gauge", {}).get("dataPoints", [])
                        or m.get("histogram", {}).get("dataPoints", [])
                    )
                    for p in points:
                        attrs = _otlp_attrs(p)
                        sid = attrs.get("session.id")
                        if not sid:
                            continue
                        s = get(sid)
                        val = p.get("asInt") or p.get("asDouble") or p.get("count")
                        s.metrics[name] = val

        for rl in env.get("resourceLogs", []):
            for sl in rl.get("scopeLogs", []):
                for lr in sl.get("logRecords", []):
                    attrs = _otlp_attrs(lr)
                    sid = attrs.get("session.id")
                    if not sid:
                        continue
                    s = get(sid)
                    name = attrs.get("event.name", "?")
                    s.events_by_name[name] += 1
                    if name == "tool_result":
                        s.tool_results.append(attrs)
                    elif name == "user_prompt":
                        s.prompts.append(attrs)
                    elif name == "api_request_body":
                        s.api_request_bodies.append(attrs)
                    elif name == "api_response_body":
                        s.api_response_bodies.append(attrs)

        for rs in env.get("resourceSpans", []):
            for ss in rs.get("scopeSpans", []):
                for span in ss.get("spans", []):
                    span_attrs = _otlp_attrs(span)
                    sid = span_attrs.get("session.id")
                    if not sid:
                        continue
                    s = get(sid)
                    s.spans_by_name[span.get("name", "?")] += 1
                    # OTEL_LOG_TOOL_CONTENT=1 attaches a `tool.output` span
                    # event to claude_code.tool spans. Carry the tool_use_id
                    # from the parent span so the comparison can join.
                    for ev in span.get("events", []):
                        if ev.get("name") == "tool.output":
                            ev_attrs = _otlp_attrs(ev)
                            ev_attrs["_tool_use_id"] = span_attrs.get("tool_use_id")
                            s.tool_output_events.append(ev_attrs)

    return sessions


# ── JSONL helpers ────────────────────────────────────────────────────


@dataclass
class JsonlSession:
    session_id: str
    path: pathlib.Path
    record_types: Counter = field(default_factory=Counter)
    tool_uses: dict = field(default_factory=dict)  # tool_use_id -> {name, input_keys}
    tool_results: dict = field(default_factory=dict)  # tool_use_id -> {has_output}
    first_prompt: str = ""
    bytes_total: int = 0


def find_jsonl(session_id: str) -> pathlib.Path | None:
    """Locate the transcript for a session — Claude Code or Cowork."""
    for root in (CLAUDE_PROJECTS, COWORK_ROOT):
        if not root.exists():
            continue
        for path in root.rglob(f"{session_id}.jsonl"):
            return path
    return None


def parse_jsonl(session_id: str, path: pathlib.Path) -> JsonlSession:
    s = JsonlSession(session_id=session_id, path=path)
    with path.open() as f:
        for line in f:
            s.bytes_total += len(line)
            try:
                rec = json.loads(line)
            except json.JSONDecodeError:
                continue
            rtype = rec.get("type", "?")
            s.record_types[rtype] += 1

            msg = rec.get("message") or {}
            content = msg.get("content")
            if isinstance(content, list):
                for blk in content:
                    if not isinstance(blk, dict):
                        continue
                    btype = blk.get("type")
                    if btype == "tool_use":
                        tid = blk.get("id", "")
                        s.tool_uses[tid] = {
                            "name": blk.get("name"),
                            "input_keys": sorted((blk.get("input") or {}).keys()),
                        }
                    elif btype == "tool_result":
                        tid = blk.get("tool_use_id", "")
                        out = blk.get("content")
                        s.tool_results[tid] = {
                            "output_bytes": len(json.dumps(out)) if out else 0,
                        }
            if not s.first_prompt and rtype == "user" and isinstance(content, str):
                s.first_prompt = content[:120]

    return s


# ── Comparison render ────────────────────────────────────────────────


def render(otel: OtelSession, jsonl: JsonlSession) -> None:
    print(f"\n┌─ Session {otel.session_id}")
    print(f"│  JSONL: {jsonl.path}")
    print(f"│         {jsonl.bytes_total/1024:.1f} KB, {sum(jsonl.record_types.values())} records")
    print(f"│  OTel:  {sum(otel.events_by_name.values())} events, "
          f"{sum(otel.spans_by_name.values())} spans, {len(otel.metrics)} metrics")
    print("└─\n")

    # ── what OTel adds ──
    print("OTel-only signals (the structural sidecar the JSONL doesn't carry):")
    print()
    print("  Metrics:")
    for k in sorted(otel.metrics):
        print(f"    {k}: {otel.metrics[k]}")
    print()
    print("  Spans:")
    for k, c in otel.spans_by_name.most_common():
        print(f"    {k}: {c}")
    print()

    # ── content layers — only present under the full content-gate set ──
    has_content = otel.api_request_bodies or otel.tool_output_events
    if has_content:
        print("OTel content layers (require OTEL_LOG_TOOL_CONTENT / OTEL_LOG_RAW_API_BODIES):")
        print()
        if otel.api_request_bodies:
            total = 0
            summaries = []
            for ev in otel.api_request_bodies:
                ref = ev.get("body_ref")
                if not ref:
                    continue
                summary = _load_body_ref(ref)
                if summary and not summary.get("missing"):
                    total += summary.get("size_bytes", 0)
                    summaries.append(summary)
            print(f"  api_request_body: {len(otel.api_request_bodies)} events, "
                  f"{len(summaries)} bodies resolved "
                  f"({total/1024:.1f} KB on disk)")
            if summaries:
                last = summaries[-1]
                print(f"    last body: model={last.get('model')}  "
                      f"messages={last.get('message_turns')} turns  "
                      f"tools={last.get('tool_schemas')} schemas  "
                      f"system_chars={last.get('system_chars')}")
                print(f"    body_ref:  {last['ref']}")
            print("    Each body is the literal Anthropic Messages API payload —")
            print("    full conversation, tool schemas, system prompt, sampling params.")
            print("    The JSONL doesn't carry the tool *schemas* or the model-side params.")
        if otel.api_response_bodies:
            print(f"  api_response_body: {len(otel.api_response_bodies)} events "
                  f"(server-side response JSON in the sidecar dir)")
        if otel.tool_output_events:
            print(f"  tool.output span events: {len(otel.tool_output_events)} "
                  f"(per-tool input + output bodies on the trace, 60KB cap per attr)")
        print()

    # ── what JSONL adds ──
    print("JSONL-only signals (the transcript OTel doesn't carry):")
    print()
    print("  Record types:")
    for k, c in jsonl.record_types.most_common():
        print(f"    {k}: {c}")
    print()
    if jsonl.first_prompt:
        print(f"  First user prompt: {jsonl.first_prompt!r}")
        print()

    # ── shared join: tool calls ──
    print("Per-tool-call alignment (joined on tool_use_id):")
    print()
    aligned: list[tuple[str, dict, dict]] = []
    otel_by_id = {tr.get("tool_use_id", ""): tr for tr in otel.tool_results}

    for tid, jl in jsonl.tool_uses.items():
        oe = otel_by_id.get(tid)
        if oe:
            aligned.append((tid, jl, oe))

    if not aligned:
        print("  (no overlap — different sessions, or telemetry was off when tools ran)")
    else:
        print(f"  {'tool':<22} {'duration_ms':>11}  {'in_size':>7}  {'out_size':>8}  decision")
        print(f"  {'-'*22} {'-'*11}  {'-'*7}  {'-'*8}  {'-'*15}")
        for tid, jl, oe in aligned[:20]:
            print(
                f"  {jl['name']:<22} "
                f"{oe.get('duration_ms','?'):>11}  "
                f"{oe.get('tool_input_size_bytes','?'):>7}  "
                f"{oe.get('tool_result_size_bytes','?'):>8}  "
                f"{oe.get('decision_source','?')}"
            )
        if len(aligned) > 20:
            print(f"  ... ({len(aligned)-20} more)")
    print()

    # ── attribute diff: what each side knows about the SAME tool call ──
    if aligned:
        # Prefer a call that *also* has a tool.output span event so the
        # display shows the strongest version of the OTel side; fall back
        # to the first aligned call when no content-layer overlap exists.
        tool_out_ids = {ev.get("_tool_use_id") for ev in otel.tool_output_events}
        rich = [t for t in aligned if t[0] in tool_out_ids]
        tid, jl, oe = (rich[0] if rich else aligned[0])
        print(f"Same tool call, two views (tool_use_id={tid}):")
        print()
        print(f"  JSONL knows:  name={jl['name']!r}  input_keys={jl['input_keys']}  "
              f"output_bytes={jsonl.tool_results.get(tid,{}).get('output_bytes',0)}")
        print()
        # Strip noisy keys for readability
        skip = {"session.id", "user.id", "user.account_uuid", "user.account_id",
                "user.email", "organization.id", "terminal.type"}
        keys = sorted(k for k in oe if k not in skip)
        for k in keys:
            v = oe[k]
            if isinstance(v, str) and len(v) > 80:
                v = v[:77] + "..."
            print(f"  OTel knows:   {k}: {v}")
        # If tool.output span events are present, surface the matching one too —
        # it's where the actual output text lives on the trace channel.
        tool_out_by_id = {ev.get("_tool_use_id"): ev for ev in otel.tool_output_events}
        match = tool_out_by_id.get(tid)
        if match:
            print()
            print("  OTel tool.output span event (only under OTEL_LOG_TOOL_CONTENT=1):")
            for k in sorted(match):
                if k.startswith("_"):
                    continue
                v = match[k]
                if isinstance(v, str) and len(v) > 80:
                    v = v[:77] + "..."
                print(f"    {k}: {v}")
        print()


# ── CLI ──────────────────────────────────────────────────────────────


def cmd_list(otel_path: pathlib.Path) -> int:
    if not otel_path.exists():
        print(f"no OTel capture at {otel_path}", file=sys.stderr)
        return 1
    sessions = otel_sessions(otel_path)
    if not sessions:
        print("(no sessions found — telemetry on but nothing captured?)")
        return 0
    print(f"{'session_id':<40} jsonl?  metrics  events  spans")
    for sid, s in sessions.items():
        path = find_jsonl(sid)
        flag = "✓" if path else "✗"
        print(
            f"{sid:<40} {flag:^6}  "
            f"{len(s.metrics):>7}  {sum(s.events_by_name.values()):>6}  "
            f"{sum(s.spans_by_name.values()):>5}"
        )
    return 0


def cmd_compare(session_id: str, otel_path: pathlib.Path, as_json: bool) -> int:
    if not otel_path.exists():
        print(f"no OTel capture at {otel_path}", file=sys.stderr)
        return 1
    otel = otel_sessions(otel_path).get(session_id)
    if not otel:
        print(f"session {session_id} not in OTel capture", file=sys.stderr)
        return 1
    path = find_jsonl(session_id)
    if not path:
        print(f"session {session_id} JSONL not found under "
              f"{CLAUDE_PROJECTS} or {COWORK_ROOT}", file=sys.stderr)
        return 1
    jsonl = parse_jsonl(session_id, path)

    if as_json:
        otel_by_id = {tr.get("tool_use_id", ""): tr for tr in otel.tool_results}
        body_summaries = [
            _load_body_ref(ev["body_ref"])
            for ev in otel.api_request_bodies
            if ev.get("body_ref")
        ]
        out = {
            "session_id": session_id,
            "jsonl_path": str(path),
            "otel": {
                "metrics": otel.metrics,
                "events": dict(otel.events_by_name),
                "spans": dict(otel.spans_by_name),
                "api_request_bodies": [b for b in body_summaries if b],
                "api_response_body_count": len(otel.api_response_bodies),
                "tool_output_event_count": len(otel.tool_output_events),
            },
            "jsonl": {
                "bytes": jsonl.bytes_total,
                "record_types": dict(jsonl.record_types),
                "first_prompt": jsonl.first_prompt,
            },
            "aligned_tool_calls": [
                {
                    "tool_use_id": tid,
                    "tool_name": jl["name"],
                    "duration_ms": otel_by_id.get(tid, {}).get("duration_ms"),
                    "decision_source": otel_by_id.get(tid, {}).get("decision_source"),
                    "input_size_bytes": otel_by_id.get(tid, {}).get("tool_input_size_bytes"),
                    "result_size_bytes": otel_by_id.get(tid, {}).get("tool_result_size_bytes"),
                }
                for tid, jl in jsonl.tool_uses.items()
                if tid in otel_by_id
            ],
        }
        print(json.dumps(out, indent=2))
    else:
        render(otel, jsonl)
    return 0


def cmd_test() -> int:
    """Tiny smoke test on a synthetic capture so the script can't silently rot."""
    import tempfile
    # session.id lives on the data point / log record / span, not the resource.
    fixture_otel = [
        {"resourceMetrics": [{
            "resource": {"attributes": []},
            "scopeMetrics": [{"metrics": [{"name": "claude_code.cost.usage", "sum": {"dataPoints": [
                {"asDouble": 0.42, "attributes": [
                    {"key": "session.id", "value": {"stringValue": "test-sess"}},
                ]}
            ]}}]}],
        }]},
        {"resourceLogs": [{
            "resource": {"attributes": []},
            "scopeLogs": [{"logRecords": [{"attributes": [
                {"key": "session.id", "value": {"stringValue": "test-sess"}},
                {"key": "event.name", "value": {"stringValue": "tool_result"}},
                {"key": "tool_use_id", "value": {"stringValue": "tu-1"}},
                {"key": "tool_name", "value": {"stringValue": "Read"}},
                {"key": "duration_ms", "value": {"intValue": 12}},
                {"key": "decision_source", "value": {"stringValue": "config"}},
            ]}]}],
        }]},
        # OTEL_LOG_RAW_API_BODIES emits a pointer event; the body lives on
        # disk. Fixture creates the body file in the temp dir below.
        {"resourceLogs": [{
            "resource": {"attributes": []},
            "scopeLogs": [{"logRecords": [{"attributes": [
                {"key": "session.id", "value": {"stringValue": "test-sess"}},
                {"key": "event.name", "value": {"stringValue": "api_request_body"}},
                {"key": "body_ref", "value": {"stringValue": "__BODY_REF__"}},
                {"key": "model", "value": {"stringValue": "claude-opus-4-7"}},
            ]}]}],
        }]},
        # OTEL_LOG_TOOL_CONTENT attaches a tool.output span event to
        # claude_code.tool. tool_use_id is read off the parent span attrs.
        {"resourceSpans": [{
            "resource": {"attributes": []},
            "scopeSpans": [{"spans": [{
                "name": "claude_code.tool",
                "attributes": [
                    {"key": "session.id", "value": {"stringValue": "test-sess"}},
                    {"key": "tool_use_id", "value": {"stringValue": "tu-1"}},
                ],
                "events": [{
                    "name": "tool.output",
                    "attributes": [
                        {"key": "bash_command", "value": {"stringValue": "ls"}},
                        {"key": "output", "value": {"stringValue": "Cargo.toml\nsrc"}},
                    ],
                }],
            }]}],
        }]},
    ]
    fixture_jsonl = [
        {"type": "user", "message": {"content": "test prompt"}},
        {"type": "assistant", "message": {"content": [
            {"type": "tool_use", "id": "tu-1", "name": "Read", "input": {"file_path": "/x"}}
        ]}},
        {"type": "user", "message": {"content": [
            {"type": "tool_result", "tool_use_id": "tu-1", "content": "hello"}
        ]}},
    ]

    with tempfile.TemporaryDirectory() as td:
        # Materialise a fake body file the api_request_body event can point at.
        body_p = pathlib.Path(td) / "fake.request.json"
        body_p.write_text(json.dumps({
            "model": "claude-opus-4-7",
            "messages": [{"role": "user", "content": "hi"}],
            "tools": [{"name": "Read"}, {"name": "Bash"}],
            "system": "you are claude",
        }))
        otel_text = "\n".join(json.dumps(e) for e in fixture_otel).replace("__BODY_REF__", str(body_p))
        otel_p = pathlib.Path(td) / "otel.jsonl"
        otel_p.write_text(otel_text)

        sessions = otel_sessions(otel_p)
        assert "test-sess" in sessions, "session.id not surfaced from OTel resource"
        os_ = sessions["test-sess"]
        assert os_.metrics.get("claude_code.cost.usage") == 0.42, os_.metrics
        assert os_.events_by_name["tool_result"] == 1, os_.events_by_name
        assert os_.tool_results[0]["tool_name"] == "Read"
        assert len(os_.api_request_bodies) == 1, os_.api_request_bodies
        assert len(os_.tool_output_events) == 1, os_.tool_output_events
        assert os_.tool_output_events[0]["_tool_use_id"] == "tu-1"

        ref = os_.api_request_bodies[0]["body_ref"]
        summary = _load_body_ref(ref)
        assert summary["model"] == "claude-opus-4-7", summary
        assert summary["message_turns"] == 1
        assert summary["tool_schemas"] == 2
        # missing-pointer case must not raise — important for moved-capture scenarios
        missing = _load_body_ref("/nonexistent/path.json")
        assert missing == {"ref": "/nonexistent/path.json", "missing": True}

        jl_p = pathlib.Path(td) / "test-sess.jsonl"
        jl_p.write_text("\n".join(json.dumps(r) for r in fixture_jsonl))
        js = parse_jsonl("test-sess", jl_p)
        assert "tu-1" in js.tool_uses, js.tool_uses
        assert js.tool_uses["tu-1"]["name"] == "Read"

    print("test ok ✓")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("session_id", nargs="?", help="Session UUID to compare")
    ap.add_argument("--otel", default=str(DEFAULT_OTEL),
                    help=f"OTel capture file (default: {DEFAULT_OTEL.relative_to(REPO_ROOT)})")
    ap.add_argument("--list", action="store_true",
                    help="List sessions present in the OTel capture")
    ap.add_argument("--json", action="store_true",
                    help="Emit the comparison as JSON for piping")
    ap.add_argument("--test", action="store_true",
                    help="Run smoke test against synthetic fixtures")
    args = ap.parse_args()

    if args.test:
        return cmd_test()
    otel_path = pathlib.Path(args.otel)
    if args.list:
        return cmd_list(otel_path)
    if not args.session_id:
        ap.print_help()
        return 2
    return cmd_compare(args.session_id, otel_path, args.json)


if __name__ == "__main__":
    sys.exit(main())
