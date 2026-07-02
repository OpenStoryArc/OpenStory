"""Live-reloading dev server for the developer-profile view.

A thin, stdlib-only harness for iterating on the profile *as product*. It glues
the pure data layer (`profile_dimensions.build_profile`) to a hand-editable HTML
surface (`profile_view.html`) with hot reload — so you tweak the design, hit
save, and the browser refreshes itself. No npm, no Vite, no build step.

    python3 scripts/profile_view.py                 # serve on http://localhost:8770
    python3 scripts/profile_view.py --port 9000
    python3 scripts/profile_view.py --days 90 --sample 60   # defaults for the view

Routes:
    GET /                      the HTML view (re-read from disk every request)
    GET /api/profile.json      cached profile; ?refresh=1 recomputes,
                               ?days=N&sample=N override the window
    GET /events                SSE stream; emits "reload" when the HTML changes

The profile is cached because computing it fans out hundreds of API calls to
OpenStory. Editing the HTML never recomputes — it just reloads the page against
cached data, keeping the design loop instant. Use Recompute (or ?refresh=1) to
re-pull from the event store.
"""

import argparse
import json
import os
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

import profile_dimensions as pd

HERE = Path(__file__).resolve().parent
HTML_PATH = HERE / "profile_view.html"

# Module-level cache: {(days, sample): payload}. Simple by design.
_CACHE: dict[tuple[int, int], dict] = {}


def get_profile(api: str, days: int, sample: int, refresh: bool) -> dict:
    key = (days, sample)
    if refresh or key not in _CACHE:
        _CACHE[key] = pd.build_profile(api, days, sample)
    return _CACHE[key]


class Handler(BaseHTTPRequestHandler):
    # Injected via partial-style class attributes (set in serve()).
    api = pd.DEFAULT_API
    default_days = 30
    default_sample = 50

    def log_message(self, *a):  # quiet; we print our own lines
        pass

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

        if path == "/" or path == "/index.html":
            try:
                self._send(200, HTML_PATH.read_text(), "text/html; charset=utf-8")
            except OSError as e:
                self._send(500, f"cannot read {HTML_PATH.name}: {e}", "text/plain")
            return

        if path == "/api/profile.json":
            days = int(qs.get("days", [self.default_days])[0])
            sample = int(qs.get("sample", [self.default_sample])[0])
            refresh = qs.get("refresh", ["0"])[0] in ("1", "true")
            t0 = time.time()
            try:
                payload = get_profile(self.api, days, sample, refresh)
            except Exception as e:  # surface to the browser rather than 500-blank
                self._send(200, json.dumps({"empty": True, "days": days, "error": str(e)}),
                           "application/json")
                return
            if refresh:
                print(f"  recomputed days={days} sample={sample} in {time.time()-t0:.1f}s "
                      f"→ {payload.get('archetype', 'empty')}")
            self._send(200, json.dumps(payload), "application/json")
            return

        if path == "/events":
            self._serve_sse()
            return

        self._send(404, "not found", "text/plain")

    def _serve_sse(self):
        """Push 'reload' whenever profile_view.html changes on disk."""
        self.send_response(200)
        self.send_header("Content-Type", "text/event-stream")
        self.send_header("Cache-Control", "no-cache")
        self.send_header("Connection", "keep-alive")
        self.end_headers()
        try:
            last = HTML_PATH.stat().st_mtime
            self.wfile.write(b": connected\n\n")
            self.wfile.flush()
            while True:
                time.sleep(0.4)
                try:
                    mtime = HTML_PATH.stat().st_mtime
                except OSError:
                    mtime = last
                if mtime != last:
                    last = mtime
                    self.wfile.write(b"data: reload\n\n")
                    self.wfile.flush()
                else:
                    self.wfile.write(b": ping\n\n")  # keep-alive
                    self.wfile.flush()
        except (BrokenPipeError, ConnectionResetError):
            pass  # browser navigated away / reloaded


def serve(port: int, api: str, days: int, sample: int) -> int:
    Handler.api = api
    Handler.default_days = days
    Handler.default_sample = sample
    if not HTML_PATH.exists():
        print(f"missing {HTML_PATH} — the view template must sit beside this script", file=sys.stderr)
        return 1
    httpd = ThreadingHTTPServer(("127.0.0.1", port), Handler)
    url = f"http://localhost:{port}"
    print(f"profile view  →  {url}")
    print(f"  data layer: profile_dimensions.build_profile(api={api}, days={days}, sample={sample})")
    print(f"  edit {HTML_PATH.name} and the browser hot-reloads. Ctrl-C to stop.")
    try:
        httpd.serve_forever()
    except KeyboardInterrupt:
        print("\nstopped.")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Live-reloading dev server for the profile view")
    ap.add_argument("--port", type=int, default=8770)
    ap.add_argument("--api", default=pd.DEFAULT_API, help="OpenStory API base URL")
    ap.add_argument("--days", type=int, default=30, help="default window for the view")
    ap.add_argument("--sample", type=int, default=50, help="default deep-fetch sample size")
    args = ap.parse_args()
    return serve(args.port, args.api, args.days, args.sample)


if __name__ == "__main__":
    sys.exit(main())
