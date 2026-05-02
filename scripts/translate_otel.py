#!/usr/bin/env python3
"""Phase 2 of the OTel ingest prototype: OTLP/JSON → CloudEvent translator.

Reads the JSONL produced by `otel_capture_server.py` (one OTLP batch envelope
per line) and emits CloudEvents shaped like the rest of the OpenStory pipeline:

    {
      "id":      <deterministic uuid5>,
      "type":    "io.arc.event",
      "subtype": "message.user.prompt",
      "source":  "otel://com.anthropic.claude_code.events",
      "agent":   "claude-code",
      "time":    "2026-04-30T10:35:10.748Z",
      "data": {
        "session_id":      "...",
        "prompt_id":       "...",
        "event_sequence":  3,
        "event_name":      "user_prompt",
        "raw":             { ...the LogRecord, untouched... }
      }
    }

Sovereignty rule: `data.raw` is the OTLP record exactly as captured. The flat
fields above `raw` are convenience projections for downstream consumers — never
the source of truth. Translators must not mutate raw shape (per
docs/soul/patterns.md "Don't mutate raw or normalize agent-specific fields").

Privacy escape hatch: `--strip-pii` drops user.email / user.account_id /
user.account_uuid from both the flat projection and the raw copy. The hashed
user.id stays — enough to recognize "same human" without holding the email.

Usage:
    python3 scripts/translate_otel.py captures/otel-probe.jsonl
    python3 scripts/translate_otel.py captures/otel-probe.jsonl --strip-pii
    python3 scripts/translate_otel.py --test
    python3 scripts/translate_otel.py --validate captures/otel-probe.jsonl
"""

import argparse
import json
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Iterator, Optional


# Namespace for deterministic uuid5. Change → all event ids change → use only
# if you intend to invalidate downstream dedup. Generated once with uuid.uuid4.
NAMESPACE = uuid.UUID("4f0a6c3b-2a1d-4f7e-9a5b-3c0d8e7f1a2b")


# event.name → CloudEvent subtype.
# Confirmed via live captures = observed in captures/otel-probe.jsonl.
# Documented = listed in code.claude.com/docs/en/monitoring-usage.md, mapped
# but not yet seen in real data — update once observed.
SUBTYPE_MAP = {
    # confirmed via live capture
    "user_prompt":             "message.user.prompt",
    "api_request":             "system.api_request",
    "mcp_server_connection":   "system.mcp_connection",
    # documented, not yet observed
    "api_error":               "system.error",
    "api_request_body":        "system.api_request_body",
    "api_response_body":       "system.api_response_body",
    "api_retries_exhausted":   "system.error",
    "tool_result":             "message.user.tool_result",
    "tool_decision":           "system.tool_decision",
    "hook_execution_start":    "system.hook",
    "hook_execution_complete": "system.hook",
    "skill_activated":         "system.skill_activated",
    "at_mention":              "system.at_mention",
    "permission_mode_changed": "system.permission_mode_changed",
    "auth":                    "system.auth",
    "compaction":              "system.compact",
    "plugin_installed":        "system.plugin_installed",
    "internal_error":          "system.error",
}


# Attribute keys to remove when --strip-pii is on. Hashed user.id is kept on
# purpose — it's a stable per-human identifier without being PII.
PII_KEYS = {"user.email", "user.account_id", "user.account_uuid"}


def value_of(otel_value: dict) -> Any:
    """Unwrap an OTLP AnyValue. JSON encoding spec puts int64 in stringValue/intValue
    as a string to dodge JS overflow — coerce back to int where possible."""
    if not otel_value:
        return None
    for kind in ("stringValue", "intValue", "doubleValue", "boolValue"):
        if kind in otel_value:
            v = otel_value[kind]
            if kind == "intValue" and isinstance(v, str):
                try:
                    return int(v)
                except ValueError:
                    return v
            return v
    if "arrayValue" in otel_value:
        return [value_of(av) for av in otel_value["arrayValue"].get("values", [])]
    if "kvlistValue" in otel_value:
        return {kv["key"]: value_of(kv.get("value", {}))
                for kv in otel_value["kvlistValue"].get("values", [])}
    if "bytesValue" in otel_value:
        return otel_value["bytesValue"]  # already base64-encoded string in JSON
    return None


def attrs_to_dict(attrs: Optional[list], *, strip_pii: bool = False) -> dict:
    """OTLP attributes list → flat dict. Optionally drop PII keys."""
    out: dict = {}
    for a in attrs or []:
        key = a.get("key")
        if not key:
            continue
        if strip_pii and key in PII_KEYS:
            continue
        out[key] = value_of(a.get("value", {}))
    return out


def map_subtype(event_name: str) -> str:
    if event_name in SUBTYPE_MAP:
        return SUBTYPE_MAP[event_name]
    # Unknown event_name → preserve in fallback subtype rather than dropping.
    # New names that show up in captures should be promoted into SUBTYPE_MAP.
    return f"system.otel.{event_name}"


def deterministic_id(*parts: Any) -> str:
    """Same parts → same uuid. Lets us re-translate captures idempotently."""
    seed = "|".join("" if p is None else str(p) for p in parts)
    return str(uuid.uuid5(NAMESPACE, seed))


def time_from_nanos(nanos: Any) -> Optional[str]:
    if not nanos:
        return None
    try:
        n = int(nanos)
    except (TypeError, ValueError):
        return None
    return datetime.fromtimestamp(n / 1e9, tz=timezone.utc).isoformat()


def strip_pii_from_record(record: dict) -> dict:
    """Return a copy of an OTLP LogRecord with PII attributes removed.
    Only edits the top-level attributes list; body and other fields untouched."""
    cleaned = dict(record)
    cleaned["attributes"] = [
        a for a in (record.get("attributes") or [])
        if a.get("key") not in PII_KEYS
    ]
    return cleaned


def strip_pii_from_metric(metric: dict) -> dict:
    """Same idea but metrics nest data points under sum/gauge/histogram."""
    cleaned = json.loads(json.dumps(metric))  # deep copy via roundtrip
    for kind in ("sum", "gauge", "histogram"):
        block = cleaned.get(kind)
        if not block:
            continue
        for dp in block.get("dataPoints") or []:
            dp["attributes"] = [
                a for a in (dp.get("attributes") or [])
                if a.get("key") not in PII_KEYS
            ]
    return cleaned


def translate_log_record(
    record: dict,
    resource_attrs: dict,
    scope: dict,
    *,
    strip_pii: bool = False,
) -> dict:
    flat = attrs_to_dict(record.get("attributes"), strip_pii=strip_pii)
    event_name = flat.get("event.name") or "unknown"
    session_id = flat.get("session.id") or "unknown-session"
    sequence = flat.get("event.sequence", 0)
    event_time = flat.get("event.timestamp") or time_from_nanos(record.get("timeUnixNano"))
    raw = strip_pii_from_record(record) if strip_pii else record

    return {
        "id": deterministic_id(session_id, event_name, sequence),
        "type": "io.arc.event",
        "subtype": map_subtype(event_name),
        "source": f"otel://{scope.get('name', 'unknown-scope')}",
        "agent": resource_attrs.get("service.name", "unknown"),
        "time": event_time,
        "data": {
            "session_id": session_id,
            "prompt_id": flat.get("prompt.id"),
            "event_sequence": sequence,
            "event_name": event_name,
            "raw": raw,
        },
    }


def translate_metric(
    metric: dict,
    resource_attrs: dict,
    scope: dict,
    *,
    strip_pii: bool = False,
) -> Iterator[dict]:
    """One CloudEvent per metric data point. Subtype carries the metric name so
    downstream consumers can filter without parsing data.raw."""
    name = metric.get("name", "unknown.metric")
    unit = metric.get("unit", "")
    raw = strip_pii_from_metric(metric) if strip_pii else metric

    for kind in ("sum", "gauge", "histogram"):
        block = metric.get(kind)
        if not block:
            continue
        for dp in block.get("dataPoints") or []:
            dp_attrs = attrs_to_dict(dp.get("attributes"), strip_pii=strip_pii)
            session_id = dp_attrs.get("session.id") or "unknown-session"
            time_nanos = dp.get("timeUnixNano")
            value = dp.get("asDouble")
            if value is None:
                value = dp.get("asInt")
                if isinstance(value, str):
                    try:
                        value = int(value)
                    except ValueError:
                        pass

            yield {
                "id": deterministic_id(session_id, name, time_nanos, dp_attrs.get("type", "")),
                "type": "io.arc.event",
                "subtype": f"system.metric.{name}",
                "source": f"otel://{scope.get('name', 'unknown-scope')}",
                "agent": resource_attrs.get("service.name", "unknown"),
                "time": time_from_nanos(time_nanos),
                "data": {
                    "session_id": session_id,
                    "metric_name": name,
                    "metric_kind": kind,
                    "metric_unit": unit,
                    "value": value,
                    "attributes": dp_attrs,
                    "raw": raw,
                },
            }


def translate_envelope(envelope: dict, *, strip_pii: bool = False) -> Iterator[dict]:
    payload = envelope.get("payload") or {}

    for rg in payload.get("resourceLogs") or []:
        resource_attrs = attrs_to_dict((rg.get("resource") or {}).get("attributes"))
        for sg in rg.get("scopeLogs") or []:
            scope = sg.get("scope") or {}
            for r in sg.get("logRecords") or []:
                yield translate_log_record(r, resource_attrs, scope, strip_pii=strip_pii)

    for rg in payload.get("resourceMetrics") or []:
        resource_attrs = attrs_to_dict((rg.get("resource") or {}).get("attributes"))
        for sg in rg.get("scopeMetrics") or []:
            scope = sg.get("scope") or {}
            for metric in sg.get("metrics") or []:
                yield from translate_metric(metric, resource_attrs, scope, strip_pii=strip_pii)


def translate_file(in_path: Path, out_path: Path, *, strip_pii: bool = False) -> tuple[int, int]:
    envelopes = 0
    emitted = 0
    with in_path.open("r", encoding="utf-8") as fin, out_path.open("w", encoding="utf-8") as fout:
        for line in fin:
            line = line.strip()
            if not line:
                continue
            envelope = json.loads(line)
            envelopes += 1
            for ce in translate_envelope(envelope, strip_pii=strip_pii):
                fout.write(json.dumps(ce, separators=(",", ":")))
                fout.write("\n")
                emitted += 1
    return envelopes, emitted


# --- validation: invariants any captured file should satisfy after translation ---

REQUIRED_FIELDS = ("id", "type", "subtype", "source", "agent", "time", "data")
REQUIRED_DATA_FIELDS = ("session_id", "raw")


def validate_file(in_path: Path) -> int:
    """Translate and check invariants — does NOT write anything to disk."""
    fail = 0
    seen_ids: set[str] = set()
    subtypes: dict[str, int] = {}
    sessions: set[str] = set()
    pii_leaks = 0

    with in_path.open("r", encoding="utf-8") as fin:
        for ln, line in enumerate(fin, 1):
            line = line.strip()
            if not line:
                continue
            envelope = json.loads(line)
            for ce in translate_envelope(envelope, strip_pii=False):
                for f in REQUIRED_FIELDS:
                    if f not in ce:
                        print(f"line {ln}: missing field {f!r}", file=sys.stderr)
                        fail += 1
                if ce.get("type") != "io.arc.event":
                    print(f"line {ln}: type != io.arc.event", file=sys.stderr)
                    fail += 1
                for f in REQUIRED_DATA_FIELDS:
                    if f not in (ce.get("data") or {}):
                        print(f"line {ln}: missing data.{f}", file=sys.stderr)
                        fail += 1
                ev_id = ce.get("id")
                if ev_id in seen_ids:
                    # uuid5 collision implies two records with identical
                    # (session, event_name, sequence) — should not happen.
                    print(f"line {ln}: duplicate id {ev_id}", file=sys.stderr)
                    fail += 1
                seen_ids.add(ev_id)
                subtypes[ce["subtype"]] = subtypes.get(ce["subtype"], 0) + 1
                sessions.add(ce["data"].get("session_id"))

            # PII presence in capture (informational, not a failure)
            for rg in (envelope.get("payload") or {}).get("resourceLogs") or []:
                for sg in rg.get("scopeLogs") or []:
                    for r in sg.get("logRecords") or []:
                        for a in r.get("attributes") or []:
                            if a.get("key") in PII_KEYS:
                                pii_leaks += 1

    print(f"unique events:     {len(seen_ids)}", file=sys.stderr)
    print(f"unique sessions:   {len(sessions)} ({sorted(sessions)})", file=sys.stderr)
    print(f"subtype histogram: {sorted(subtypes.items(), key=lambda kv: -kv[1])}", file=sys.stderr)
    print(f"pii attribute hits in raw (informational): {pii_leaks}", file=sys.stderr)
    if fail:
        print(f"FAIL: {fail} invariant violation(s)", file=sys.stderr)
    else:
        print("OK: all invariants pass", file=sys.stderr)
    return 1 if fail else 0


# --- smoke test: synthetic fixtures, no I/O ---

def _otel_attr(key: str, val) -> dict:
    if isinstance(val, bool):
        return {"key": key, "value": {"boolValue": val}}
    if isinstance(val, int):
        return {"key": key, "value": {"intValue": str(val)}}  # JSON encoding uses string
    if isinstance(val, float):
        return {"key": key, "value": {"doubleValue": val}}
    return {"key": key, "value": {"stringValue": str(val)}}


def _log_fixture(event_name: str = "user_prompt", **extra) -> dict:
    base_attrs = {
        "session.id": "sess-1",
        "user.id": "hashed-user",
        "user.email": "user@example.com",
        "user.account_id": "acct-1",
        "user.account_uuid": "acct-uuid-1",
        "event.name": event_name,
        "event.sequence": 3,
        "event.timestamp": "2026-04-30T10:35:10.748Z",
        "prompt.id": "p-abc",
    }
    base_attrs.update(extra)

    return {
        "captured_at": "2026-04-30T00:00:00Z",
        "signal": "logs",
        "path": "/v1/logs",
        "payload": {
            "resourceLogs": [{
                "resource": {"attributes": [
                    _otel_attr("service.name", "claude-code"),
                    _otel_attr("service.version", "2.1.123"),
                ]},
                "scopeLogs": [{
                    "scope": {"name": "com.anthropic.claude_code.events", "version": "2.1.123"},
                    "logRecords": [{
                        "timeUnixNano": "1777545310748000000",
                        "body": {"stringValue": f"claude_code.{event_name}"},
                        "attributes": [_otel_attr(k, v) for k, v in base_attrs.items()],
                    }],
                }],
            }],
        },
    }


def _metric_fixture() -> dict:
    return {
        "captured_at": "2026-04-30T00:00:00Z",
        "signal": "metrics",
        "path": "/v1/metrics",
        "payload": {
            "resourceMetrics": [{
                "resource": {"attributes": [_otel_attr("service.name", "claude-code")]},
                "scopeMetrics": [{
                    "scope": {"name": "com.anthropic.claude_code.events"},
                    "metrics": [{
                        "name": "claude_code.token.usage",
                        "unit": "tokens",
                        "sum": {
                            "dataPoints": [{
                                "attributes": [
                                    _otel_attr("session.id", "sess-1"),
                                    _otel_attr("user.email", "user@example.com"),
                                    _otel_attr("type", "input"),
                                ],
                                "timeUnixNano": "1777545312174000000",
                                "asDouble": 347,
                            }],
                        },
                    }],
                }],
            }],
        },
    }


def smoke_test() -> int:
    failures = 0

    def check(name: str, cond: bool, detail: str = ""):
        nonlocal failures
        if not cond:
            print(f"FAIL: {name} {detail}", file=sys.stderr)
            failures += 1

    # 1. user_prompt translates to message.user.prompt with raw preserved
    ces = list(translate_envelope(_log_fixture()))
    check("one event from one log record", len(ces) == 1, f"got {len(ces)}")
    ce = ces[0]
    check("type", ce["type"] == "io.arc.event")
    check("subtype mapping", ce["subtype"] == "message.user.prompt")
    check("agent from service.name", ce["agent"] == "claude-code")
    check("session_id surfaced", ce["data"]["session_id"] == "sess-1")
    check("prompt_id surfaced", ce["data"]["prompt_id"] == "p-abc")
    check("event_sequence is int", ce["data"]["event_sequence"] == 3)
    check("time from event.timestamp", ce["time"] == "2026-04-30T10:35:10.748Z")

    # 2. raw is the original LogRecord, untouched (sovereignty rule)
    raw_keys = {a["key"] for a in ce["data"]["raw"]["attributes"]}
    check("raw retains user.email by default", "user.email" in raw_keys)
    check("raw retains body field", "body" in ce["data"]["raw"])

    # 3. --strip-pii drops PII from the flat projection AND from raw
    ces_clean = list(translate_envelope(_log_fixture(), strip_pii=True))
    raw_keys_clean = {a["key"] for a in ces_clean[0]["data"]["raw"]["attributes"]}
    check("strip_pii removes user.email from raw", "user.email" not in raw_keys_clean)
    check("strip_pii removes user.account_id", "user.account_id" not in raw_keys_clean)
    check("strip_pii keeps user.id (hashed)", "user.id" in raw_keys_clean)
    check("strip_pii keeps session.id", "session.id" in raw_keys_clean)

    # 4. Determinism — same inputs → same id
    id1 = deterministic_id("sess-1", "user_prompt", 3)
    id2 = deterministic_id("sess-1", "user_prompt", 3)
    id3 = deterministic_id("sess-1", "user_prompt", 4)
    check("deterministic id stable", id1 == id2)
    check("different sequence → different id", id1 != id3)

    # 5. Unknown event names get a fallback subtype, not dropped
    ces_unknown = list(translate_envelope(_log_fixture(event_name="some_new_event")))
    check("unknown event preserved", ces_unknown[0]["subtype"] == "system.otel.some_new_event")

    # 6. api_request retains the rich token/cost/duration fields in raw
    rich = _log_fixture(
        event_name="api_request",
        model="claude-haiku-4-5-20251001",
        input_tokens=347,
        output_tokens=13,
        cost_usd=0.000412,
        duration_ms=1263,
        query_source="generate_session_title",
    )
    rich_ce = next(translate_envelope(rich))
    check("api_request subtype", rich_ce["subtype"] == "system.api_request")
    rich_attrs = {a["key"]: value_of(a["value"]) for a in rich_ce["data"]["raw"]["attributes"]}
    check("api_request preserves model", rich_attrs.get("model") == "claude-haiku-4-5-20251001")
    check("api_request preserves cost_usd", rich_attrs.get("cost_usd") == 0.000412)
    check("api_request preserves query_source", rich_attrs.get("query_source") == "generate_session_title")

    # 7. Metrics — one CloudEvent per data point
    metric_ces = list(translate_envelope(_metric_fixture()))
    check("one metric event per data point", len(metric_ces) == 1)
    mce = metric_ces[0]
    check("metric subtype carries name", mce["subtype"] == "system.metric.claude_code.token.usage")
    check("metric value preserved", mce["data"]["value"] == 347)
    check("metric session_id from data point attrs", mce["data"]["session_id"] == "sess-1")
    check("metric kind tracked", mce["data"]["metric_kind"] == "sum")

    # 8. --strip-pii on metrics
    metric_clean = list(translate_envelope(_metric_fixture(), strip_pii=True))
    dp_keys = {a["key"] for a in metric_clean[0]["data"]["raw"]["sum"]["dataPoints"][0]["attributes"]}
    check("strip_pii removes user.email from metric raw", "user.email" not in dp_keys)

    if failures:
        print(f"\n{failures} failure(s)", file=sys.stderr)
        return 1
    print("smoke test passed.", file=sys.stderr)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="OTLP/JSON capture → CloudEvents JSONL.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument("input", nargs="?", type=Path, help="input JSONL produced by otel_capture_server.py")
    ap.add_argument("--out", type=Path, help="output path (default: <input>.cloudevents.jsonl)")
    ap.add_argument("--strip-pii", action="store_true", help="drop user.email/account_id/account_uuid from output")
    ap.add_argument("--test", action="store_true", help="run synthetic smoke test and exit")
    ap.add_argument("--validate", type=Path, help="translate and check invariants without writing output")
    args = ap.parse_args()

    if args.test:
        return smoke_test()

    if args.validate:
        return validate_file(args.validate)

    if not args.input:
        ap.error("input file required (or use --test / --validate)")

    out = args.out or args.input.with_suffix(".cloudevents.jsonl")
    envelopes, emitted = translate_file(args.input, out, strip_pii=args.strip_pii)
    print(f"read {envelopes} envelopes, emitted {emitted} cloudevents → {out}", file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
