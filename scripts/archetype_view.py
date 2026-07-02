"""Live-reloading dev server for the archetype report (the product view).

Same stdlib-only, SSE hot-reload harness as profile_view.py, pointed at the
citation-traceable archetype report. Edit archetype_view.html, hit save, the
browser refreshes itself. No npm, no build step.

    python3 scripts/archetype_view.py                # http://localhost:8771
    python3 scripts/archetype_view.py --days 90 --sample 60

Routes:
    GET /                    the HTML view (re-read from disk each request)
    GET /api/archetype.json  cached report; ?refresh=1 recomputes; ?days=&sample=
    GET /events              SSE; emits "reload" when the HTML changes
"""

import argparse
import json
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

import archetype_report as ar

HERE = Path(__file__).resolve().parent
HTML_PATH = HERE / "archetype_view.html"
_CACHE: dict[tuple[int, int], dict] = {}
_records_cache: dict[str, list] = {}  # sid → records, for lazy event drill-down


def get_report(api, days, sample, refresh):
    key = (days, sample)
    if refresh or key not in _CACHE:
        _CACHE[key] = ar.build_payload(api, days, sample)
    return _CACHE[key]


class Handler(BaseHTTPRequestHandler):
    api = ar.pd.DEFAULT_API
    default_days = 30
    default_sample = 50

    def log_message(self, *a):
        pass

    def _latest_sid(self):
        sessions = (ar.pd._get(self.api, "/api/sessions") or {}).get("sessions", [])
        return max(sessions, key=lambda s: s.get("last_event", ""))["session_id"]

    def _send(self, code, body, ctype):
        data = body.encode() if isinstance(body, str) else body
        self.send_response(code)
        self.send_header("Content-Type", ctype)
        self.send_header("Content-Length", str(len(data)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        parsed = urlparse(self.path)
        path, qs = parsed.path, parse_qs(parsed.query)

        if path in ("/", "/index.html"):
            try:
                self._send(200, HTML_PATH.read_text(), "text/html; charset=utf-8")
            except OSError as e:
                self._send(500, f"cannot read {HTML_PATH.name}: {e}", "text/plain")
            return

        if path == "/api/archetype.json":
            days = int(qs.get("days", [self.default_days])[0])
            sample = int(qs.get("sample", [self.default_sample])[0])
            refresh = qs.get("refresh", ["0"])[0] in ("1", "true")
            t0 = time.time()
            try:
                payload = get_report(self.api, days, sample, refresh)
            except Exception as e:
                self._send(200, json.dumps({"empty": True, "days": days, "error": str(e)}),
                           "application/json")
                return
            if refresh:
                print(f"  recomputed days={days} sample={sample} in {time.time()-t0:.1f}s "
                      f"→ {payload.get('style', {}).get('claim', 'empty')}")
            self._send(200, json.dumps(payload), "application/json")
            return

        if path == "/api/trace.json":
            sid = qs.get("session", [None])[0]
            try:
                if not sid:
                    sid = self._latest_sid()
                self._send(200, json.dumps(ar.mt.trace_json(self.api, sid)), "application/json")
            except Exception as e:
                self._send(200, json.dumps({"error": str(e)}), "application/json")
            return

        if path == "/api/event.json":
            sid = qs.get("session", [None])[0] or self._latest_sid()
            seq = int(qs.get("seq", ["-1"])[0])
            try:
                recs = _records_cache.get(sid)
                if recs is None:
                    recs = ar.mt.records_for(self.api, sid)
                    _records_cache[sid] = recs
                self._send(200, json.dumps(ar.mt.event_detail(recs, seq)), "application/json")
            except Exception as e:
                self._send(200, json.dumps({"error": str(e)}), "application/json")
            return

        if path == "/events":
            self._serve_sse()
            return

        self._send(404, "not found", "text/plain")

    def _serve_sse(self):
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.end_headers()
        try:
            last = HTML_PATH.stat().st_mtime
            self.wfile.write(b": connected\n\n"); self.wfile.flush()
            while True:
                time.sleep(0.4)
                try:
                    mtime = HTML_PATH.stat().st_mtime
                except OSError:
                    mtime = last
                if mtime != last:
                    last = mtime
                    self.wfile.write(b"data: reload\n\n")
                else:
                    self.wfile.write(b": ping\n\n")
                self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass


def serve(port, api, days, sample):
    Handler.api = api
    Handler.default_days = days
    Handler.default_sample = sample
    if not HTML_PATH.exists():
        print(f"missing {HTML_PATH}", file=sys.stderr); return 1
    httpd = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    print(f"archetype view  →  http://localhost:{port}")
    print(f"  data layer: archetype_report.build_payload(api={api}, days={days}, sample={sample})")
    print(f"  edit {HTML_PATH.name} and the browser hot-reloads. Ctrl-C to stop.")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped.")
    return 0


def main():
    ap = argparse.ArgumentParser(description="Live-reloading dev server for the archetype report")
    ap.add_argument("--port", type=int, default=8771)
    ap.add_argument("--api", default=ar.pd.DEFAULT_API)
    ap.add_argument("--days", type=int, default=30)
    ap.add_argument("--sample", type=int, default=50)
    args = ap.parse_args()
    return serve(args.port, args.api, args.days, args.sample)


if __name__ == "__main__":
    sys.exit(main())
