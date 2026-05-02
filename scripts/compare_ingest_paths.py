#!/usr/bin/env python3
"""Phase 3 of the OTel ingest prototype: diff the transcript and OTel paths.

For one session, fetch the events the file-watcher already produced (via the
OpenStory REST API) and compare them against the CloudEvents translated from
captured OTLP records. Reports:

  - subtype histogram, side-by-side
  - subtypes unique to each source (the actual coverage delta)
  - field-level diff for overlapping subtypes (e.g. message.user.prompt)
  - "new signal" surfaced only by OTel: query_source, request_id, cost_usd,
    per-tier token counts

This is the scientific-method moment for the OTel prototype: data — not docs —
tells us whether OTel adds signal beyond the transcript pipeline.

Usage:
    # 1. Translate a capture first (Phase 2)
    python3 scripts/translate_otel.py captures/otel-probe.jsonl

    # 2. Diff against the live transcript pipeline
    python3 scripts/compare_ingest_paths.py \\
        --session 81d9cb6a-653c-4b65-a092-3be91b24e220 \\
        --otel captures/otel-probe.cloudevents.jsonl

    # 3. (Optional) self-test with synthetic fixtures
    python3 scripts/compare_ingest_paths.py --test

The session id is the *Claude Code* session id — the same UUID that appears in
~/.claude/projects/ transcript filenames AND in OTel session.id attributes.
"""

import argparse
import json
import sys
import urllib.error
import urllib.request
from collections import Counter
from pathlib import Path
from typing import Any, Iterable, Optional


DEFAULT_API = "http://localhost:3002"


# ---- transcript path: OpenStory REST API ----

def fetch_transcript_events(session_id: str, api_base: str) -> list[dict]:
    """GET /api/sessions/{id}/records → list of records.

    Each record has at minimum: id, seq, session_id, timestamp, record_type,
    payload (with subtype). May also have parent_uuid, depth, truncated, etc."""
    url = f"{api_base.rstrip('/')}/api/sessions/{session_id}/records?limit=10000"
    try:
        with urllib.request.urlopen(url, timeout=10) as resp:
            body = json.loads(resp.read())
    except urllib.error.URLError as e:
        print(f"failed to reach {url}: {e}", file=sys.stderr)
        return []

    if isinstance(body, list):
        return body
    return body.get("records") or body.get("data") or []


def transcript_subtype(record: dict) -> str:
    """Best-effort subtype extraction. Different paths through the API may put
    it on `payload.subtype`, `subtype`, or fall back to `record_type`."""
    payload = record.get("payload") or {}
    if isinstance(payload, dict):
        st = payload.get("subtype")
        if st:
            return st
    return record.get("subtype") or record.get("record_type") or "unknown"


# ---- otel path: pre-translated cloudevents jsonl ----

def load_otel_cloudevents(path: Path, session_id: Optional[str] = None) -> list[dict]:
    """Read CloudEvents JSONL (output of translate_otel.py), optionally filter."""
    out = []
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            ce = json.loads(line)
            if session_id and (ce.get("data") or {}).get("session_id") != session_id:
                continue
            out.append(ce)
    return out


# ---- analysis ----

def histogram(events: Iterable[dict], key_fn) -> Counter:
    return Counter(key_fn(e) for e in events)


def coverage_table(t_hist: Counter, o_hist: Counter) -> list[tuple[str, int, int]]:
    subtypes = sorted(set(t_hist) | set(o_hist))
    return [(s, t_hist.get(s, 0), o_hist.get(s, 0)) for s in subtypes]


def render_table(rows: list[tuple[str, int, int]]) -> str:
    if not rows:
        return "(no events)"
    width = max(len(r[0]) for r in rows)
    lines = [f"  {'subtype'.ljust(width)}  transcript  otel"]
    lines.append(f"  {'-' * width}  ----------  ----")
    for s, t, o in rows:
        marker = ""
        if t and not o:
            marker = "  ← transcript only"
        elif o and not t:
            marker = "  ← otel only"
        lines.append(f"  {s.ljust(width)}  {t:>10}  {o:>4}{marker}")
    return "\n".join(lines)


def field_diff_for_user_prompt(transcript: list[dict], otel: list[dict]) -> dict:
    """For the one subtype guaranteed to overlap (message.user.prompt), surface
    which fields each source carries."""
    t = next((r for r in transcript if transcript_subtype(r) == "message.user.prompt"), None)
    o = next((c for c in otel if c["subtype"] == "message.user.prompt"), None)

    if not t or not o:
        return {"available": False, "reason": "no overlap to diff"}

    t_fields = _flat_keys(t)
    o_fields = _flat_keys(o)
    return {
        "available": True,
        "transcript_only_fields": sorted(t_fields - o_fields),
        "otel_only_fields": sorted(o_fields - t_fields),
        "shared_fields": sorted(t_fields & o_fields),
        "transcript_sample_id": t.get("id"),
        "otel_sample_id": o.get("id"),
    }


def _flat_keys(obj: Any, prefix: str = "") -> set[str]:
    """Flatten a dict tree into dotted-path keys for shape comparison."""
    out: set[str] = set()
    if isinstance(obj, dict):
        for k, v in obj.items():
            new_prefix = f"{prefix}.{k}" if prefix else k
            if isinstance(v, (dict, list)):
                out |= _flat_keys(v, new_prefix)
            else:
                out.add(new_prefix)
    elif isinstance(obj, list) and obj:
        # only recurse into the first element so list-of-records doesn't
        # explode the keyspace
        out |= _flat_keys(obj[0], f"{prefix}[]")
    return out


def otel_unique_signal(otel: list[dict]) -> dict:
    """What do we get from OTel that we don't get from transcripts? Pull out
    fields that appear in api_request records and metric data points."""
    api_requests = [c for c in otel if c["subtype"] == "system.api_request"]
    metrics = [c for c in otel if c["subtype"].startswith("system.metric.")]

    query_sources = Counter()
    models = Counter()
    request_ids: set[str] = set()
    durations: list[int] = []
    costs: list[float] = []

    for c in api_requests:
        attrs = {a["key"]: a["value"] for a in c["data"]["raw"]["attributes"]}
        qs = (attrs.get("query_source") or {}).get("stringValue")
        if qs:
            query_sources[qs] += 1
        model = (attrs.get("model") or {}).get("stringValue")
        if model:
            models[model] += 1
        rid = (attrs.get("request_id") or {}).get("stringValue")
        if rid:
            request_ids.add(rid)
        dur = (attrs.get("duration_ms") or {}).get("intValue")
        if dur is not None:
            durations.append(int(dur))
        cost = (attrs.get("cost_usd") or {}).get("doubleValue")
        if cost is not None:
            costs.append(float(cost))

    metric_breakdown = Counter()
    for c in metrics:
        metric_name = c["data"].get("metric_name", "?")
        type_attr = c["data"].get("attributes", {}).get("type", "")
        key = f"{metric_name}[type={type_attr}]" if type_attr else metric_name
        metric_breakdown[key] += 1

    return {
        "api_request_count": len(api_requests),
        "metric_event_count": len(metrics),
        "query_source_distribution": dict(query_sources),
        "model_distribution": dict(models),
        "unique_request_ids": len(request_ids),
        "duration_ms_samples": durations[:5],
        "duration_ms_total": sum(durations),
        "cost_usd_samples": costs[:5],
        "cost_usd_total": round(sum(costs), 6) if costs else 0,
        "metric_breakdown": dict(metric_breakdown),
    }


# ---- report ----

def report(session_id: str, transcript: list[dict], otel: list[dict]) -> str:
    lines = [
        f"session: {session_id}",
        f"transcript records: {len(transcript)}",
        f"otel cloudevents:   {len(otel)}",
        "",
        "=== coverage by subtype ===",
    ]
    t_hist = histogram(transcript, transcript_subtype)
    o_hist = histogram(otel, lambda e: e.get("subtype") or "unknown")
    lines.append(render_table(coverage_table(t_hist, o_hist)))

    lines.append("")
    lines.append("=== field shape diff: message.user.prompt ===")
    diff = field_diff_for_user_prompt(transcript, otel)
    if not diff["available"]:
        lines.append(f"  {diff['reason']}")
    else:
        lines.append(f"  shared:          {len(diff['shared_fields'])} fields")
        lines.append(f"  transcript-only: {len(diff['transcript_only_fields'])}")
        for f in diff["transcript_only_fields"][:8]:
            lines.append(f"    + {f}")
        if len(diff["transcript_only_fields"]) > 8:
            lines.append(f"    ... +{len(diff['transcript_only_fields']) - 8} more")
        lines.append(f"  otel-only:       {len(diff['otel_only_fields'])}")
        for f in diff["otel_only_fields"][:8]:
            lines.append(f"    + {f}")
        if len(diff["otel_only_fields"]) > 8:
            lines.append(f"    ... +{len(diff['otel_only_fields']) - 8} more")

    lines.append("")
    lines.append("=== signal unique to otel ===")
    sig = otel_unique_signal(otel)
    lines.append(f"  api_request events:       {sig['api_request_count']}")
    lines.append(f"  metric events:            {sig['metric_event_count']}")
    lines.append(f"  unique request_ids:       {sig['unique_request_ids']}")
    lines.append(f"  total duration_ms:        {sig['duration_ms_total']}")
    lines.append(f"  total cost_usd:           {sig['cost_usd_total']}")
    if sig["query_source_distribution"]:
        lines.append(f"  query_source:             {sig['query_source_distribution']}")
    if sig["model_distribution"]:
        lines.append(f"  model:                    {sig['model_distribution']}")
    if sig["metric_breakdown"]:
        lines.append(f"  metric breakdown:")
        for k, v in sorted(sig["metric_breakdown"].items()):
            lines.append(f"    {k}: {v}")

    return "\n".join(lines)


# ---- self-test ----

def _fake_transcript(session_id: str) -> list[dict]:
    return [
        {
            "id": "t-1", "seq": 1, "session_id": session_id,
            "timestamp": "2026-04-30T10:35:10.748Z",
            "record_type": "user_message",
            "payload": {"subtype": "message.user.prompt", "text": "hello"},
        },
        {
            "id": "t-2", "seq": 2, "session_id": session_id,
            "timestamp": "2026-04-30T10:35:11.000Z",
            "record_type": "assistant_message",
            "payload": {"subtype": "message.assistant.text", "text": "hi"},
        },
        {
            "id": "t-3", "seq": 3, "session_id": session_id,
            "timestamp": "2026-04-30T10:35:11.500Z",
            "record_type": "system_event",
            "payload": {"subtype": "system.turn.complete"},
        },
    ]


def _fake_otel(session_id: str) -> list[dict]:
    return [
        {
            "id": "o-1", "type": "io.arc.event", "subtype": "message.user.prompt",
            "agent": "claude-code", "time": "2026-04-30T10:35:10.748Z",
            "data": {
                "session_id": session_id, "prompt_id": "p-1", "event_sequence": 3,
                "raw": {"attributes": [
                    {"key": "prompt", "value": {"stringValue": "hello"}},
                    {"key": "prompt_length", "value": {"stringValue": "5"}},
                ]},
            },
        },
        {
            "id": "o-2", "type": "io.arc.event", "subtype": "system.api_request",
            "agent": "claude-code", "time": "2026-04-30T10:35:12.005Z",
            "data": {
                "session_id": session_id, "event_sequence": 4,
                "raw": {"attributes": [
                    {"key": "model", "value": {"stringValue": "claude-haiku-4-5-20251001"}},
                    {"key": "query_source", "value": {"stringValue": "generate_session_title"}},
                    {"key": "request_id", "value": {"stringValue": "req_abc"}},
                    {"key": "duration_ms", "value": {"intValue": "1263"}},
                    {"key": "cost_usd", "value": {"doubleValue": 0.000412}},
                ]},
            },
        },
        {
            "id": "o-3", "type": "io.arc.event",
            "subtype": "system.metric.claude_code.token.usage", "agent": "claude-code",
            "time": "2026-04-30T10:35:12.174Z",
            "data": {
                "session_id": session_id, "metric_name": "claude_code.token.usage",
                "metric_kind": "sum", "value": 347,
                "attributes": {"type": "input"},
                "raw": {},
            },
        },
    ]


def smoke_test() -> int:
    session_id = "test-session"
    t = _fake_transcript(session_id)
    o = _fake_otel(session_id)

    failures = 0
    def check(name: str, cond: bool, detail: str = ""):
        nonlocal failures
        if not cond:
            print(f"FAIL: {name} {detail}", file=sys.stderr)
            failures += 1

    # subtype extraction
    check("transcript subtype from payload",
          transcript_subtype(t[0]) == "message.user.prompt")
    check("transcript subtype fallback to record_type",
          transcript_subtype({"record_type": "fallback"}) == "fallback")

    # histograms
    t_hist = histogram(t, transcript_subtype)
    o_hist = histogram(o, lambda e: e["subtype"])
    check("transcript histogram size", len(t_hist) == 3, f"got {dict(t_hist)}")
    check("otel histogram size", len(o_hist) == 3, f"got {dict(o_hist)}")

    # coverage table
    rows = coverage_table(t_hist, o_hist)
    by_subtype = {s: (tt, oo) for s, tt, oo in rows}
    check("overlap on user.prompt", by_subtype.get("message.user.prompt") == (1, 1))
    check("transcript-only assistant.text",
          by_subtype.get("message.assistant.text") == (1, 0))
    check("otel-only api_request",
          by_subtype.get("system.api_request") == (0, 1))

    # field diff
    diff = field_diff_for_user_prompt(t, o)
    check("diff is available", diff["available"])

    # otel unique signal extraction
    sig = otel_unique_signal(o)
    check("api_request counted", sig["api_request_count"] == 1)
    check("metric counted", sig["metric_event_count"] == 1)
    check("query_source surfaced",
          sig["query_source_distribution"] == {"generate_session_title": 1})
    check("cost summed", sig["cost_usd_total"] == 0.000412)
    check("duration summed", sig["duration_ms_total"] == 1263)

    # session_id filter on otel loader
    fake_path = Path("/tmp/_compare_ingest_paths_test.jsonl")
    fake_path.write_text("\n".join(json.dumps(c) for c in o) + "\n")
    loaded = load_otel_cloudevents(fake_path, session_id=session_id)
    other = load_otel_cloudevents(fake_path, session_id="not-this-one")
    check("session filter loads matching", len(loaded) == 3)
    check("session filter excludes others", len(other) == 0)
    fake_path.unlink()

    if failures:
        print(f"\n{failures} failure(s)", file=sys.stderr)
        return 1
    print("smoke test passed.", file=sys.stderr)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="Diff transcript-watcher events vs OTel-translated CloudEvents for one session.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument("--session", help="Claude Code session id (UUID)")
    ap.add_argument("--otel", type=Path, help="path to translated CloudEvents JSONL")
    ap.add_argument("--api", default=DEFAULT_API, help=f"OpenStory API base (default: {DEFAULT_API})")
    ap.add_argument("--test", action="store_true", help="run synthetic smoke test")
    args = ap.parse_args()

    if args.test:
        return smoke_test()

    if not args.session or not args.otel:
        ap.error("--session and --otel are required (or use --test)")

    transcript = fetch_transcript_events(args.session, args.api)
    otel = load_otel_cloudevents(args.otel, session_id=args.session)
    print(report(args.session, transcript, otel))
    return 0


if __name__ == "__main__":
    sys.exit(main())
