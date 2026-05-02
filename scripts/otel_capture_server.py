#!/usr/bin/env python3
"""Phase 1 of the OTel ingest prototype: a tiny OTLP/HTTP+JSON receiver.

Captures every Claude Code OTel batch (logs, metrics, traces) to a JSONL file,
one batch per line, exactly as it arrived on the wire. Phase 2 (translator)
reads the JSONL and produces CloudEvents.

Stdlib only — no aiohttp, no opentelemetry-sdk, no proto compilation. We accept
JSON-encoded OTLP and reject protobuf with a useful error, because protobuf
needs a schema and that's a Phase-2-or-later concern. JSON is what we want
anyway: human-readable captures we can grep, diff, and feed to a script.

Usage:
    # Terminal A — start the capture server
    python3 scripts/otel_capture_server.py

    # Terminal B — point Claude Code at it
    export CLAUDE_CODE_ENABLE_TELEMETRY=1
    export OTEL_LOGS_EXPORTER=otlp
    export OTEL_METRICS_EXPORTER=otlp
    export OTEL_EXPORTER_OTLP_PROTOCOL=http/json
    export OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
    export OTEL_LOG_USER_PROMPTS=1
    export OTEL_LOG_TOOL_DETAILS=1
    export OTEL_LOGS_EXPORT_INTERVAL=1000
    claude

Each captured line looks like:
    {"captured_at": "...", "signal": "logs", "payload": {...full OTLP envelope...}}

Run --test for a smoke check that doesn't need a real Claude Code session.
"""

import argparse
import json
import sys
import time
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from typing import Optional


SIGNAL_BY_PATH = {
    "/v1/logs": "logs",
    "/v1/metrics": "metrics",
    "/v1/traces": "traces",
}


def signal_from_path(path: str) -> str:
    for prefix, signal in SIGNAL_BY_PATH.items():
        if path.startswith(prefix):
            return signal
    return "unknown"


def attr_lookup(attrs, key: str) -> Optional[str]:
    """Pull a string-ish value out of an OTLP attributes list."""
    if not attrs:
        return None
    for a in attrs:
        if a.get("key") == key:
            v = a.get("value", {}) or {}
            for kind in ("stringValue", "intValue", "boolValue", "doubleValue"):
                if kind in v:
                    return str(v[kind])
    return None


def summarize(payload: dict) -> dict:
    """Cheap one-line summary for live tailing — does not parse the full envelope."""
    resource_groups = 0
    records = 0
    event_names: set[str] = set()
    services: set[str] = set()

    triples = (
        ("resourceLogs", "scopeLogs", "logRecords"),
        ("resourceMetrics", "scopeMetrics", "metrics"),
        ("resourceSpans", "scopeSpans", "spans"),
    )

    for resource_key, scope_key, item_key in triples:
        groups = payload.get(resource_key) or []
        resource_groups += len(groups)
        for rg in groups:
            svc = attr_lookup((rg.get("resource") or {}).get("attributes"), "service.name")
            if svc:
                services.add(svc)
            for sg in rg.get(scope_key) or []:
                items = sg.get(item_key) or []
                records += len(items)
                for item in items:
                    name = item.get("eventName") or attr_lookup(item.get("attributes"), "event.name")
                    if name:
                        event_names.add(name)

    return {
        "resource_groups": resource_groups,
        "records": records,
        "event_names": sorted(event_names),
        "services": sorted(services),
    }


def make_handler(out_path: Path, verbose: bool):
    class OTLPHandler(BaseHTTPRequestHandler):
        def log_message(self, fmt, *args):
            if verbose:
                super().log_message(fmt, *args)

        def _ok(self):
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(b'{"partialSuccess":{}}')

        def _bad(self, status: int, msg: str):
            self.send_response(status)
            self.send_header("Content-Type", "text/plain; charset=utf-8")
            self.end_headers()
            self.wfile.write(msg.encode("utf-8"))

        def do_POST(self):
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length) if length else b""
            ctype = self.headers.get("Content-Type", "")

            if "application/json" not in ctype:
                self._bad(
                    415,
                    "this prototype receives JSON-encoded OTLP only.\n"
                    f"got Content-Type={ctype!r}.\n"
                    "set OTEL_EXPORTER_OTLP_PROTOCOL=http/json on the agent.\n",
                )
                return

            try:
                payload = json.loads(body) if body else {}
            except json.JSONDecodeError as e:
                self._bad(400, f"invalid JSON: {e}\n")
                return

            envelope = {
                "captured_at": datetime.now(timezone.utc).isoformat(),
                "signal": signal_from_path(self.path),
                "path": self.path,
                "payload": payload,
            }

            with out_path.open("a", encoding="utf-8") as f:
                f.write(json.dumps(envelope, separators=(",", ":")))
                f.write("\n")

            counts = summarize(payload)
            svc = ",".join(counts["services"]) or "-"
            evs = ",".join(counts["event_names"]) or "-"
            print(
                f"[{envelope['captured_at']}] {envelope['signal']:7s} "
                f"groups={counts['resource_groups']} records={counts['records']:3d} "
                f"svc={svc} events={evs}",
                file=sys.stderr,
                flush=True,
            )
            self._ok()

        def do_GET(self):
            if self.path == "/healthz":
                self.send_response(200)
                self.send_header("Content-Type", "text/plain")
                self.end_headers()
                self.wfile.write(b"ok\n")
                return
            self._bad(404, "this is an OTLP/HTTP receiver. POST /v1/logs etc.\n")

    return OTLPHandler


def run_server(host: str, port: int, out_path: Path, verbose: bool) -> None:
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.touch()

    handler = make_handler(out_path, verbose)
    srv = ThreadingHTTPServer((host, port), handler)
    print(f"otlp/http+json receiver listening on http://{host}:{port}", file=sys.stderr)
    print(f"  signals: /v1/logs  /v1/metrics  /v1/traces", file=sys.stderr)
    print(f"  writing: {out_path}", file=sys.stderr)
    print(f"  ctrl-c to stop", file=sys.stderr)
    try:
        srv.serve_forever()
    except KeyboardInterrupt:
        print("\nshutting down...", file=sys.stderr)
        srv.shutdown()
        print(f"captured to {out_path}", file=sys.stderr)


def smoke_test() -> int:
    """Sanity check: feed a synthetic OTLP/JSON envelope through summarize()."""
    fake_logs = {
        "resourceLogs": [{
            "resource": {"attributes": [
                {"key": "service.name", "value": {"stringValue": "claude-code"}},
                {"key": "service.version", "value": {"stringValue": "test"}},
            ]},
            "scopeLogs": [{
                "scope": {"name": "claude-code"},
                "logRecords": [
                    {
                        "timeUnixNano": "1700000000000000000",
                        "body": {"stringValue": "user_prompt"},
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "user_prompt"}},
                            {"key": "prompt.id", "value": {"stringValue": "abc-123"}},
                            {"key": "prompt.length", "value": {"intValue": 42}},
                        ],
                    },
                    {
                        "timeUnixNano": "1700000001000000000",
                        "body": {"stringValue": "tool_result"},
                        "attributes": [
                            {"key": "event.name", "value": {"stringValue": "tool_result"}},
                            {"key": "tool_use_id", "value": {"stringValue": "toolu_xyz"}},
                        ],
                    },
                ],
            }],
        }],
    }

    summary = summarize(fake_logs)
    expected = {
        "resource_groups": 1,
        "records": 2,
        "event_names": ["tool_result", "user_prompt"],
        "services": ["claude-code"],
    }
    if summary != expected:
        print("FAIL: summary mismatch", file=sys.stderr)
        print(f"  expected: {expected}", file=sys.stderr)
        print(f"  got:      {summary}", file=sys.stderr)
        return 1

    if signal_from_path("/v1/logs") != "logs":
        print("FAIL: signal_from_path", file=sys.stderr)
        return 1
    if signal_from_path("/v1/metrics/extra") != "metrics":
        print("FAIL: signal_from_path prefix", file=sys.stderr)
        return 1
    if signal_from_path("/something") != "unknown":
        print("FAIL: signal_from_path unknown", file=sys.stderr)
        return 1

    print("smoke test passed.", file=sys.stderr)
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(
        description="OTLP/HTTP+JSON capture server for Claude Code OTel prototyping.",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    ap.add_argument(
        "--out",
        type=Path,
        default=Path("captures") / f"otel-{int(time.time())}.jsonl",
        help="output JSONL path (default: captures/otel-<ts>.jsonl)",
    )
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=4318)
    ap.add_argument("--verbose", action="store_true", help="log every HTTP request")
    ap.add_argument("--test", action="store_true", help="run smoke test and exit")
    args = ap.parse_args()

    if args.test:
        return smoke_test()

    run_server(args.host, args.port, args.out, args.verbose)
    return 0


if __name__ == "__main__":
    sys.exit(main())
