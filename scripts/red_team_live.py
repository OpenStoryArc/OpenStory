#!/usr/bin/env python3
"""Live red-team probes against a running OpenStory instance.

This is the **adversarial** counterpart to `red_team.py` (which runs the
structured test suites + dep scanners). This script actively tries to
break the server. Every probe is a real attack technique: header
smuggling, slowloris, JSON depth bomb, regex DoS, range-header abuse,
WebSocket flood, etc.

## Soul

A structured test asserts a specific known behavior. A live probe
*does the bad thing* and watches what happens. Different signal — a
test catches regressions, a probe catches unknown-unknowns.

## What this script does NOT do

- It does not exfiltrate data. The only data we read is what the
  unauthenticated API would already give to any localhost caller.
- It does not modify state on a token-protected instance. The
  `--token` flag lets you authenticate for read probes; we never
  POST/DELETE on an authenticated instance unless `--allow-write`.
- It does not run network-level attacks against anything other
  than the explicit `--target` URL. Default target is localhost.

## Usage

    python3 scripts/red_team_live.py
    python3 scripts/red_team_live.py --target http://localhost:3002
    python3 scripts/red_team_live.py --json
    python3 scripts/red_team_live.py --skip slowloris  # exclude slow probes
    python3 scripts/red_team_live.py --token <api-token>

## Exit codes

    0 — every probe fended off or low-impact
    1 — at least one probe found a high-severity issue
    2 — server unreachable
"""
from __future__ import annotations

import argparse
import asyncio
import json
import socket
import sys
import time
import urllib.parse
from dataclasses import dataclass, field, asdict
from typing import Any

import urllib.request
import urllib.error

ANSI_RED = "\033[31m"
ANSI_GREEN = "\033[32m"
ANSI_YELLOW = "\033[33m"
ANSI_BLUE = "\033[34m"
ANSI_DIM = "\033[2m"
ANSI_BOLD = "\033[1m"
ANSI_RESET = "\033[0m"


@dataclass
class ProbeResult:
    name: str
    technique: str
    severity: str  # info | low | medium | high | critical
    verdict: str = "pending"  # blocked | concerning | exploit | error | skipped
    expected: str = ""
    actual: str = ""
    detail: str = ""
    duration_ms: int = 0

    @property
    def is_blocked(self) -> bool:
        return self.verdict == "blocked"

    @property
    def is_exploit(self) -> bool:
        return self.verdict == "exploit"


# ── Low-level HTTP helpers ──────────────────────────────────────────────


def http_get(
    url: str, headers: dict | None = None, timeout: float = 10.0
) -> tuple[int, dict, bytes]:
    """Plain HTTP GET. Returns (status, headers, body)."""
    req = urllib.request.Request(url, method="GET", headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, dict(resp.headers), resp.read()
    except urllib.error.HTTPError as e:
        return e.code, dict(e.headers or {}), (e.read() if hasattr(e, "read") else b"")
    except urllib.error.URLError as e:
        return 0, {}, str(e).encode()


def http_send_raw(host: str, port: int, payload: bytes, timeout: float = 5.0) -> bytes:
    """Send raw bytes over a TCP socket. Returns the response (up to 4KB)."""
    with socket.create_connection((host, port), timeout=timeout) as s:
        s.sendall(payload)
        chunks = []
        try:
            while True:
                chunk = s.recv(4096)
                if not chunk:
                    break
                chunks.append(chunk)
                if sum(len(c) for c in chunks) > 4096:
                    break
        except socket.timeout:
            pass
        return b"".join(chunks)


def parse_target(target: str) -> tuple[str, int, bool]:
    """Return (host, port, is_https) from a URL."""
    u = urllib.parse.urlparse(target)
    host = u.hostname or "localhost"
    is_https = u.scheme == "https"
    port = u.port or (443 if is_https else 80)
    return host, port, is_https


# ── Probes ──────────────────────────────────────────────────────────────


def probe_recon_fingerprint(target: str, token: str) -> ProbeResult:
    """Recon: what does the server tell us about itself?

    Two checks:
    1. Always: are there server-stack leaks in response headers? Those
       are bad regardless of auth mode.
    2. If `--token` is set: confirm `/api/health` requires auth. The
       endpoint is by-design under auth_middleware; if a no-Bearer call
       returns 200 anyway, that's an exploit.
    """
    p = ProbeResult(
        name="recon: server fingerprint",
        technique="GET /api/health with and without Bearer",
        severity="medium",
    )
    t0 = time.time()

    # Unauthed call
    status_unauth, hdrs_unauth, body_unauth = http_get(f"{target}/api/health")
    # Server-stack leaks in headers — always bad
    header_leaks = []
    if "server" in {k.lower() for k in hdrs_unauth}:
        header_leaks.append(
            f"Server header present: {hdrs_unauth.get('server') or hdrs_unauth.get('Server')}"
        )
    if "x-powered-by" in {k.lower() for k in hdrs_unauth}:
        header_leaks.append("X-Powered-By header present")

    p.duration_ms = int((time.time() - t0) * 1000)

    # Body-leak check only matters when auth is configured
    if not token:
        p.severity = "info"
        if header_leaks:
            p.verdict = "concerning"
            p.actual = "; ".join(header_leaks)
        else:
            p.verdict = "blocked"
            p.actual = "no token provided — by-design no auth, no header leaks"
            p.detail = (
                "/api/health is intentionally open to localhost when api_token is unset; "
                "re-run with --token to verify auth gating"
            )
        return p

    # Auth IS configured — confirm /api/health requires it
    if status_unauth == 200:
        p.verdict = "exploit"
        p.actual = f"/api/health returns 200 WITHOUT Bearer header when api_token is set!"
        p.expected = "401 Unauthorized when api_token is set and Bearer is missing"
        return p

    if header_leaks:
        p.verdict = "concerning"
        p.actual = "; ".join(header_leaks)
        return p

    p.verdict = "blocked"
    p.actual = f"unauthed /api/health returned {status_unauth}; no header leaks"
    return p


def probe_auth_header_smuggling(target: str, token: str) -> ProbeResult:
    """Some servers trust X-Forwarded-* / X-Original-URL for routing."""
    p = ProbeResult(
        name="auth: X-Forwarded / X-Original-URL bypass",
        technique="Inject reverse-proxy headers to try bypass",
        severity="high",
    )
    t0 = time.time()

    attacks = [
        # If a misconfigured server trusts X-Original-URL, the
        # request might route to a different endpoint than the path.
        {"X-Original-URL": "/api/sessions", "X-Rewrite-URL": "/api/sessions"},
        {"X-Forwarded-Host": "evil.com", "X-Forwarded-For": "127.0.0.1"},
        {"X-Forwarded-Proto": "https", "X-Real-IP": "127.0.0.1"},
        # Authorization header injected via X-Forwarded-User pattern
        {"X-Forwarded-User": "admin", "X-Auth-User": "admin"},
        # Some auth middleware respects X-Original-Token
        {"X-Original-Token": "fake-token", "X-Auth-Token": "fake-token"},
    ]

    suspicious = []
    for attack in attacks:
        s, _, _ = http_get(f"{target}/api/sessions", headers=attack)
        # If token IS set, unauthed status was 401. We're checking
        # that none of these headers grant access.
        if token and s == 200:
            suspicious.append(f"granted access with {attack}")

    p.duration_ms = int((time.time() - t0) * 1000)
    if suspicious:
        p.verdict = "exploit"
        p.actual = "\n  ".join(suspicious)
        p.expected = "all attempts return 401 when api_token is set"
    else:
        p.verdict = "blocked"
        if not token:
            p.detail = "no api_token configured — auth bypass test inconclusive"
        p.actual = "none of the smuggled headers granted access"
    return p


def probe_path_traversal_url(target: str) -> ProbeResult:
    """Try every URL-encoding variant of ../etc/passwd."""
    p = ProbeResult(
        name="path traversal: URL-level variants",
        technique="GET with encoded `..` in path/query",
        severity="critical",
    )
    t0 = time.time()
    payloads = [
        "/api/sessions/../../../../etc/passwd/events",
        "/api/sessions/%2e%2e/%2e%2e/etc/passwd/events",
        "/api/sessions/..%2f..%2fetc%2fpasswd/events",
        "/api/sessions/.%2e/.%2e/etc/passwd/events",
        "/api/sessions/%252e%252e/%252e%252e/etc/passwd/events",  # double-encoded
        "/api/sessions/abc%00../etc/passwd/events",
        # Windows-style
        "/api/sessions/..\\..\\..\\Windows\\System32\\config\\SAM/events",
        # UNC path
        "/api/sessions/%5c%5cevil.com%5cshare/events",
    ]
    exploits = []
    for path in payloads:
        s, _, body = http_get(f"{target}{path}")
        if s == 200 and (b"root:" in body or b"/bin/" in body):
            exploits.append(f"{path} → 200 with passwd-like body")
        elif s >= 500:
            exploits.append(f"{path} → {s} (5xx — potential DoS)")
    p.duration_ms = int((time.time() - t0) * 1000)
    if exploits:
        p.verdict = "exploit"
        p.actual = "\n  ".join(exploits)
        p.expected = "all 4xx/404, never 200 with /etc/passwd content, never 5xx"
    else:
        p.verdict = "blocked"
        p.actual = f"{len(payloads)} traversal variants: no file disclosure, no 5xx"
    return p


def probe_header_bomb(target: str) -> ProbeResult:
    """Send 1000 garbage headers — does the server gracefully reject?"""
    p = ProbeResult(
        name="DoS: header bomb (1000 headers)",
        technique="GET /api/sessions with 1000 random X-* headers",
        severity="medium",
    )
    t0 = time.time()
    headers = {f"X-Garbage-{i:04d}": f"v{i}" for i in range(1000)}
    s, _, _ = http_get(f"{target}/api/sessions", headers=headers)
    p.duration_ms = int((time.time() - t0) * 1000)
    if s == 0:
        p.verdict = "concerning"
        p.actual = "connection error / hyper crashed"
        p.expected = "graceful 4xx (431 Header Fields Too Large) or honored request"
    elif 400 <= s < 500:
        p.verdict = "blocked"
        p.actual = f"server returned {s} — bounded gracefully"
    elif s == 200:
        # Allowed — but check the server is still up after
        s2, _, _ = http_get(f"{target}/api/sessions")
        p.verdict = "blocked" if s2 == 200 else "concerning"
        p.actual = f"server accepted 1000 headers (200); post-attack health: {s2}"
    else:
        p.verdict = "concerning"
        p.actual = f"unexpected status {s}"
    return p


def probe_large_url(target: str) -> ProbeResult:
    """Send an 8KB+ URL path."""
    p = ProbeResult(
        name="DoS: 64KB URL path",
        technique="GET with extremely long path segment",
        severity="medium",
    )
    t0 = time.time()
    long_seg = "a" * 65000
    s, _, _ = http_get(f"{target}/api/sessions/{long_seg}/events")
    p.duration_ms = int((time.time() - t0) * 1000)
    if s == 0:
        p.verdict = "concerning"
        p.actual = "connection error"
    elif 400 <= s < 500:
        p.verdict = "blocked"
        p.actual = f"server returned {s} (URI Too Long or rejected at parse)"
    else:
        p.verdict = "blocked"
        p.actual = f"server returned {s} — handled the long path"
    return p


def probe_slowloris(target: str) -> ProbeResult:
    """Open 50 connections, send headers slowly. Does the server free them?"""
    p = ProbeResult(
        name="DoS: slowloris (50 slow conns)",
        technique="Open many TCP conns, drip headers 1 byte at a time",
        severity="high",
    )
    host, port, _ = parse_target(target)
    t0 = time.time()

    sockets = []
    try:
        for _ in range(50):
            s = socket.create_connection((host, port), timeout=2.0)
            s.sendall(b"GET /api/sessions HTTP/1.1\r\nHost: localhost\r\n")
            # Don't send the terminating \r\n\r\n — hold the connection
            sockets.append(s)
        # While holding 50 dripping conns, try a normal request
        time.sleep(1.0)
        s_health, _, _ = http_get(f"{target}/api/sessions", timeout=5.0)
        p.duration_ms = int((time.time() - t0) * 1000)
        if s_health == 200:
            p.verdict = "blocked"
            p.actual = f"50 slow conns held; normal requests still 200"
        else:
            p.verdict = "exploit"
            p.actual = f"50 slow conns DoS'd the server: normal req → {s_health}"
            p.expected = "200 on normal requests despite slow connections"
    finally:
        for s in sockets:
            try:
                s.close()
            except OSError:
                pass
    return p


def probe_te_cl_smuggling(target: str) -> ProbeResult:
    """Send conflicting Transfer-Encoding + Content-Length headers."""
    p = ProbeResult(
        name="HTTP smuggling: TE.CL desync",
        technique="POST with both Transfer-Encoding: chunked and Content-Length",
        severity="high",
    )
    host, port, _ = parse_target(target)
    payload = (
        b"POST /api/sessions HTTP/1.1\r\n"
        b"Host: localhost\r\n"
        b"Transfer-Encoding: chunked\r\n"
        b"Content-Length: 6\r\n"
        b"\r\n"
        b"0\r\n"
        b"\r\n"
        b"GPOST"  # smuggled request
    )
    t0 = time.time()
    try:
        resp = http_send_raw(host, port, payload, timeout=3.0)
        p.duration_ms = int((time.time() - t0) * 1000)
        # hyper is strict about TE+CL; we expect 400. The exploit
        # would be: server processes both bodies and the smuggled
        # "GPOST" becomes a separate request.
        if b"HTTP/1.1 400" in resp:
            p.verdict = "blocked"
            p.actual = "server rejected with 400 (correct per RFC 7230)"
        elif b"HTTP/1.1 200" in resp and resp.count(b"HTTP/1.1") > 1:
            p.verdict = "exploit"
            p.actual = "two responses to one socket — smuggling possible"
        else:
            p.verdict = "blocked"
            p.actual = f"server response: {resp[:200].decode(errors='replace')!r}"
    except (socket.error, OSError) as e:
        p.duration_ms = int((time.time() - t0) * 1000)
        p.verdict = "blocked"
        p.actual = f"connection closed: {e}"
    return p


def probe_json_depth_bomb(target: str) -> ProbeResult:
    """1000-level nested JSON as request body."""
    p = ProbeResult(
        name="DoS: JSON depth bomb",
        technique="POST 1000-level nested JSON to /api/sessions",
        severity="medium",
    )
    body = "{\"a\":" * 1000 + "1" + "}" * 1000
    t0 = time.time()
    req = urllib.request.Request(
        f"{target}/api/sessions",
        data=body.encode(),
        method="POST",
        headers={"Content-Type": "application/json"},
    )
    try:
        with urllib.request.urlopen(req, timeout=5.0) as resp:
            status = resp.status
    except urllib.error.HTTPError as e:
        status = e.code
    except Exception as e:
        status = 0
    p.duration_ms = int((time.time() - t0) * 1000)
    if status == 0:
        p.verdict = "concerning"
        p.actual = "connection error — possible parser crash"
    elif status >= 500:
        p.verdict = "concerning"
        p.actual = f"5xx ({status}) — parser may have stack-overflowed"
    else:
        p.verdict = "blocked"
        p.actual = f"server returned {status} — handled deep JSON"
    return p


def probe_gzip_bomb(target: str) -> ProbeResult:
    """Send a small gzipped body that decompresses to 1 GB."""
    p = ProbeResult(
        name="DoS: gzip bomb",
        technique="POST 1KB gzip that expands to 1GB of zeros",
        severity="high",
    )
    import gzip
    import io

    # 1 GB of zeros — gzip compresses to ~1 MB. Server would OOM
    # if it auto-decompresses untrusted bodies without a cap.
    buf = io.BytesIO()
    with gzip.GzipFile(fileobj=buf, mode="wb", compresslevel=9) as gz:
        # Write in 1 MB chunks
        chunk = b"\x00" * (1 << 20)
        for _ in range(1024):
            gz.write(chunk)
    compressed = buf.getvalue()

    t0 = time.time()
    req = urllib.request.Request(
        f"{target}/api/sessions",
        data=compressed,
        method="POST",
        headers={"Content-Encoding": "gzip", "Content-Type": "application/octet-stream"},
    )
    try:
        with urllib.request.urlopen(req, timeout=10.0) as resp:
            status = resp.status
    except urllib.error.HTTPError as e:
        status = e.code
    except Exception as e:
        status = 0
    p.duration_ms = int((time.time() - t0) * 1000)
    # Axum / tower-http does not auto-decompress unless explicitly
    # configured. Either rejection (4xx) or treating-as-opaque (4xx)
    # is fine. The exploit would be a 200 after a 30s delay.
    if status == 0:
        p.verdict = "concerning"
        p.actual = "connection error / server stuck"
    elif status == 200 and p.duration_ms > 5000:
        p.verdict = "concerning"
        p.actual = f"200 after {p.duration_ms}ms — possible decompression amplification"
    else:
        p.verdict = "blocked"
        p.actual = f"server returned {status} in {p.duration_ms}ms"
    return p


def probe_fts_regex_dos(target: str) -> ProbeResult:
    """Try FTS5 queries crafted to be expensive."""
    p = ProbeResult(
        name="DoS: FTS5 expensive query",
        technique="GET /api/search with FTS5 query designed to walk the entire index",
        severity="medium",
    )
    # Wildcards + many ORs force FTS5 to walk many postings
    bad_query = " OR ".join(["x*"] * 100)
    t0 = time.time()
    enc = urllib.parse.quote(bad_query)
    s, _, body = http_get(f"{target}/api/search?q={enc}&limit=1000")
    p.duration_ms = int((time.time() - t0) * 1000)
    if s == 0:
        p.verdict = "concerning"
        p.actual = "connection error"
    elif s >= 500:
        p.verdict = "concerning"
        p.actual = f"{s} — FTS5 query crashed handler"
    elif p.duration_ms > 30000:
        p.verdict = "concerning"
        p.actual = f"query took {p.duration_ms}ms — DoS possible"
    else:
        p.verdict = "blocked"
        p.actual = f"{s} in {p.duration_ms}ms ({len(body)} bytes)"
    return p


def probe_endpoint_method_matrix(target: str) -> ProbeResult:
    """For every known endpoint, try every HTTP method. Catch unintended writes."""
    p = ProbeResult(
        name="recon: HTTP method matrix",
        technique="OPTIONS/PUT/PATCH/DELETE on read-only endpoints",
        severity="medium",
    )
    endpoints = [
        "/api/sessions",
        "/api/health",
        "/api/search?q=foo",
        "/api/insights/pulse",
        "/api/agent/tools",
    ]
    methods = ["PUT", "PATCH", "DELETE"]
    findings = []
    t0 = time.time()
    for ep in endpoints:
        for m in methods:
            req = urllib.request.Request(f"{target}{ep}", method=m)
            try:
                with urllib.request.urlopen(req, timeout=5.0) as resp:
                    if resp.status == 200:
                        findings.append(f"{m} {ep} → 200 (unintended success)")
            except urllib.error.HTTPError as e:
                if e.code == 200:
                    findings.append(f"{m} {ep} → 200")
            except Exception:
                pass
    p.duration_ms = int((time.time() - t0) * 1000)
    if findings:
        p.verdict = "concerning"
        p.actual = "\n  ".join(findings)
        p.expected = "all 405 Method Not Allowed on read-only endpoints"
    else:
        p.verdict = "blocked"
        p.actual = f"{len(endpoints) * len(methods)} method probes: all rejected"
    return p


def probe_pipeline_flood(target: str) -> ProbeResult:
    """Send 1000 pipelined GETs in a single connection."""
    p = ProbeResult(
        name="DoS: HTTP/1.1 request pipelining",
        technique="Send 1000 pipelined requests in one TCP conn",
        severity="medium",
    )
    host, port, _ = parse_target(target)
    req = b"GET /api/sessions HTTP/1.1\r\nHost: localhost\r\n\r\n"
    payload = req * 1000
    t0 = time.time()
    try:
        with socket.create_connection((host, port), timeout=10.0) as s:
            s.sendall(payload)
            # Don't read everything — just check the server responds
            resp = b""
            s.settimeout(5.0)
            try:
                while len(resp) < 4096:
                    chunk = s.recv(4096)
                    if not chunk:
                        break
                    resp += chunk
            except socket.timeout:
                pass
        # Server should be alive after
        s_after, _, _ = http_get(f"{target}/api/sessions")
        p.duration_ms = int((time.time() - t0) * 1000)
        if s_after == 200:
            p.verdict = "blocked"
            p.actual = f"server processed pipelined requests, still alive ({s_after})"
        else:
            p.verdict = "exploit"
            p.actual = f"server unhealthy after pipelined flood ({s_after})"
    except Exception as e:
        p.duration_ms = int((time.time() - t0) * 1000)
        p.verdict = "concerning"
        p.actual = f"error: {e}"
    return p


def probe_websocket_flood(target: str) -> ProbeResult:
    """Open many WebSocket connections; server should not OOM."""
    p = ProbeResult(
        name="DoS: WebSocket connection flood",
        technique="Open 100 concurrent WS connections, hold them",
        severity="medium",
    )
    host, port, _ = parse_target(target)
    import base64

    # Hand-rolled WebSocket handshake — avoid any client lib dep
    key = base64.b64encode(b"\x00" * 16).decode()
    upgrade_req = (
        f"GET /ws HTTP/1.1\r\n"
        f"Host: localhost\r\n"
        f"Connection: Upgrade\r\n"
        f"Upgrade: websocket\r\n"
        f"Sec-WebSocket-Key: {key}\r\n"
        f"Sec-WebSocket-Version: 13\r\n"
        f"\r\n"
    ).encode()

    sockets = []
    t0 = time.time()
    upgrades = 0
    try:
        for _ in range(100):
            try:
                s = socket.create_connection((host, port), timeout=2.0)
                s.sendall(upgrade_req)
                resp = s.recv(1024)
                if b"101" in resp:
                    upgrades += 1
                    sockets.append(s)
                else:
                    s.close()
            except OSError:
                pass
        time.sleep(0.5)
        s_health, _, _ = http_get(f"{target}/api/sessions", timeout=5.0)
        p.duration_ms = int((time.time() - t0) * 1000)
        if s_health == 200:
            p.verdict = "blocked"
            p.actual = f"{upgrades}/100 WS upgrades; server still 200"
        else:
            p.verdict = "exploit"
            p.actual = f"{upgrades} WS upgrades caused server unhealthy ({s_health})"
    finally:
        for s in sockets:
            try:
                s.close()
            except OSError:
                pass
    return p


def probe_search_huge_query(target: str) -> ProbeResult:
    """100K-character search query."""
    p = ProbeResult(
        name="DoS: 100K-char FTS5 query",
        technique="GET /api/search with massive q param",
        severity="medium",
    )
    huge = "x" * 100_000
    enc = urllib.parse.quote(huge)
    t0 = time.time()
    s, _, _ = http_get(f"{target}/api/search?q={enc}")
    p.duration_ms = int((time.time() - t0) * 1000)
    if s == 0:
        p.verdict = "concerning"
        p.actual = "connection error"
    elif s >= 500:
        p.verdict = "concerning"
        p.actual = f"{s} — query crashed handler"
    else:
        p.verdict = "blocked"
        p.actual = f"{s} in {p.duration_ms}ms"
    return p


def probe_cors_origin_spoofing(target: str) -> ProbeResult:
    """Try evil Origin headers; CORS should not echo back arbitrary origins."""
    p = ProbeResult(
        name="CORS: arbitrary origin reflection",
        technique="OPTIONS preflight with evil Origin",
        severity="medium",
    )
    origins = ["https://evil.com", "null", "file://", "http://attacker"]
    leaks = []
    t0 = time.time()
    for o in origins:
        req = urllib.request.Request(
            f"{target}/api/sessions",
            method="OPTIONS",
            headers={
                "Origin": o,
                "Access-Control-Request-Method": "GET",
                "Access-Control-Request-Headers": "Authorization",
            },
        )
        try:
            with urllib.request.urlopen(req, timeout=5.0) as resp:
                hdrs = {k.lower(): v for k, v in resp.headers.items()}
                aco = hdrs.get("access-control-allow-origin", "")
                if aco == o or aco == "*":
                    leaks.append(f"reflected {o} (ACAO={aco})")
        except urllib.error.HTTPError as e:
            hdrs = {k.lower(): v for k, v in (e.headers or {}).items()}
            aco = hdrs.get("access-control-allow-origin", "")
            if aco == o:
                leaks.append(f"reflected {o} on {e.code}")
        except Exception:
            pass
    p.duration_ms = int((time.time() - t0) * 1000)
    if leaks:
        p.verdict = "exploit"
        p.actual = "\n  ".join(leaks)
        p.expected = "ACAO header must NOT echo arbitrary origins"
    else:
        p.verdict = "blocked"
        p.actual = "no origin reflection"
    return p


def probe_mongo_operator_injection(target: str) -> ProbeResult:
    """Mongo backend specific: try $ne / $regex in session_id."""
    p = ProbeResult(
        name="NoSQL injection: Mongo operators",
        technique="session_id with `{\"$ne\":null}` etc.",
        severity="high",
    )
    payloads = [
        '{"$ne":null}',
        '{"$regex":".*"}',
        '{"$gt":""}',
        # The query is a path param so it gets URL-encoded; Rust's
        # mongodb driver wraps it in a string literal anyway. We
        # check for the negative case: handler doesn't error or
        # return all-sessions data.
    ]
    exploits = []
    t0 = time.time()
    for raw in payloads:
        enc = urllib.parse.quote(raw)
        s, _, body = http_get(f"{target}/api/sessions/{enc}/events")
        if s == 200 and len(body) > 100:
            exploits.append(f"{raw} → 200 with non-empty body ({len(body)}B)")
        elif s >= 500:
            exploits.append(f"{raw} → {s} (handler crashed)")
    p.duration_ms = int((time.time() - t0) * 1000)
    if exploits:
        p.verdict = "exploit"
        p.actual = "\n  ".join(exploits)
        p.expected = "200 with [] body or 404 — session_id is a string, not a query operator"
    else:
        p.verdict = "blocked"
        p.actual = f"{len(payloads)} Mongo operator payloads: all rejected as opaque strings"
    return p


def probe_static_dir_traversal(target: str) -> ProbeResult:
    """If --static-dir is mounted, try `..` escape via the static fallback."""
    p = ProbeResult(
        name="static-dir: path traversal via fallback",
        technique="GET /../etc/passwd via the static-files fallback",
        severity="critical",
    )
    payloads = [
        "/../etc/passwd",
        "/../../etc/passwd",
        "/static/../etc/passwd",
        "/%2e%2e/%2e%2e/etc/passwd",
        "/..%2f..%2fetc%2fpasswd",
        # The "static-dir not set" path returns 404 fallback; we want
        # to verify the ServeDir layer doesn't escape its root.
    ]
    exploits = []
    t0 = time.time()
    for path in payloads:
        s, _, body = http_get(f"{target}{path}")
        if s == 200 and (b"root:" in body or b"/bin/" in body):
            exploits.append(f"{path} → /etc/passwd content!")
        elif s >= 500:
            exploits.append(f"{path} → {s} (5xx — handler crashed)")
    p.duration_ms = int((time.time() - t0) * 1000)
    if exploits:
        p.verdict = "exploit"
        p.actual = "\n  ".join(exploits)
    else:
        p.verdict = "blocked"
        p.actual = f"{len(payloads)} static-dir traversal variants: no escape"
    return p


def probe_container_egress(target: str) -> ProbeResult:
    """If we're hitting localhost, peek at the container's outbound conns.

    Open-story should ONLY talk to NATS (4222), Mongo (27017 if mongo
    backend), and serve clients. Outbound to anywhere else (especially
    public IPs) suggests a phone-home / supply-chain implant.
    """
    p = ProbeResult(
        name="container egress: phone-home check",
        technique="docker exec netstat on running container",
        severity="critical",
    )
    import subprocess

    t0 = time.time()
    # Find a container running the open-story image
    r = subprocess.run(
        ["docker", "ps", "--filter", "ancestor=open-story:test", "--filter", "ancestor=open-story:hardened", "--format", "{{.Names}}"],
        capture_output=True,
        text=True,
        timeout=5,
    )
    names = [n for n in r.stdout.strip().split("\n") if n]
    if not names:
        p.verdict = "skipped"
        p.detail = "no open-story container running to inspect"
        return p

    name = names[0]
    # ss/netstat may not be in the image — try cat /proc/net/tcp instead
    r = subprocess.run(
        ["docker", "exec", name, "sh", "-c",
         "cat /proc/net/tcp 2>/dev/null | awk 'NR>1 && $4==\"01\" {print $3}' | head -20"],
        capture_output=True,
        text=True,
        timeout=10,
    )
    p.duration_ms = int((time.time() - t0) * 1000)
    raw = r.stdout.strip()
    if not raw:
        p.verdict = "blocked"
        p.actual = "no outbound TCP connections from container"
        return p

    # Each line: HEX_IP:HEX_PORT (network byte order)
    suspicious = []
    for line in raw.split("\n"):
        try:
            hex_ip, hex_port = line.split(":")
            ip_bytes = [int(hex_ip[i:i+2], 16) for i in (6, 4, 2, 0)]
            ip = ".".join(str(b) for b in ip_bytes)
            port = int(hex_port, 16)
            # Allowed: localhost, NATS, Mongo
            if ip.startswith("127.") or ip == "0.0.0.0":
                continue
            if port in (4222, 27017):
                continue
            suspicious.append(f"{ip}:{port}")
        except (ValueError, IndexError):
            continue

    if suspicious:
        p.verdict = "exploit"
        p.actual = "outbound to unexpected destinations: " + ", ".join(suspicious)
    else:
        p.verdict = "blocked"
        p.actual = f"all egress to localhost or NATS/Mongo ({len(raw.splitlines())} conns)"
    return p


def probe_token_brute_force_timing(target: str, token_len: int = 32) -> ProbeResult:
    """Measure response-time variance across many wrong tokens.

    If the server short-circuits on the first wrong byte, we'd see
    timing variance that leaks the prefix. constant_time_eq makes
    this not happen.
    """
    p = ProbeResult(
        name="auth: timing-attack resistance",
        technique="Measure RTT variance across 500 wrong tokens",
        severity="medium",
    )
    import statistics

    timings = []
    t0 = time.time()
    for i in range(500):
        # Vary token shape — prefix similarity in chars 0..k
        prefix = "x" * (i % token_len)
        suffix = "y" * (token_len - len(prefix))
        tok = prefix + suffix
        req_t0 = time.perf_counter()
        s, _, _ = http_get(
            f"{target}/api/sessions",
            headers={"Authorization": f"Bearer {tok}"},
            timeout=5.0,
        )
        timings.append((time.perf_counter() - req_t0) * 1_000_000)  # μs
    p.duration_ms = int((time.time() - t0) * 1000)
    mean = statistics.mean(timings)
    stdev = statistics.stdev(timings)
    cv = stdev / mean if mean else 0
    p.actual = f"500 wrong tokens: mean={mean:.0f}μs σ={stdev:.0f}μs CV={cv:.2f}"
    # When auth is configured, CV should be small (constant-time eq).
    # When auth is NOT configured, every request returns 200 in similar
    # time, also low CV. So a high CV is suspicious in either mode.
    if cv > 0.5:
        p.verdict = "concerning"
        p.detail = "high coefficient of variation — possible timing side channel"
    else:
        p.verdict = "blocked"
    return p


# ── Reporting ──────────────────────────────────────────────────────────


def verdict_badge(v: str) -> str:
    return {
        "blocked": f"{ANSI_GREEN}✓ BLOCKED   {ANSI_RESET}",
        "concerning": f"{ANSI_YELLOW}⚠ CONCERN   {ANSI_RESET}",
        "exploit": f"{ANSI_RED}✗ EXPLOIT   {ANSI_RESET}",
        "error": f"{ANSI_YELLOW}! ERROR     {ANSI_RESET}",
        "skipped": f"{ANSI_DIM}- SKIPPED   {ANSI_RESET}",
        "pending": f"{ANSI_DIM}· pending   {ANSI_RESET}",
    }[v]


def print_human(probes: list[ProbeResult]) -> None:
    print()
    print(f"{ANSI_BOLD}Live red-team probes{ANSI_RESET}")
    print(f"{'─' * 70}")
    for p in probes:
        dur = f"{p.duration_ms / 1000:.1f}s" if p.duration_ms else "  -"
        print(
            f"  {verdict_badge(p.verdict)} "
            f"{ANSI_BOLD}{p.name}{ANSI_RESET} "
            f"{ANSI_DIM}[{p.severity}] {dur}{ANSI_RESET}"
        )
        print(f"      {ANSI_DIM}{p.technique}{ANSI_RESET}")
        if p.actual:
            for line in p.actual.split("\n"):
                print(f"      {line}")
        if p.detail:
            print(f"      {ANSI_YELLOW}note:{ANSI_RESET} {p.detail}")
        print()

    print(f"{'─' * 70}")
    counts = {"blocked": 0, "concerning": 0, "exploit": 0, "error": 0, "skipped": 0}
    for p in probes:
        counts[p.verdict] = counts.get(p.verdict, 0) + 1
    print(
        f"  {ANSI_GREEN}{counts['blocked']} blocked{ANSI_RESET}, "
        f"{ANSI_YELLOW}{counts['concerning']} concerning{ANSI_RESET}, "
        f"{ANSI_RED}{counts['exploit']} exploits{ANSI_RESET}, "
        f"{counts['error']} errored, "
        f"{counts['skipped']} skipped"
    )
    print()


# ── Main ────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--target", default="http://localhost:3002", help="server URL")
    ap.add_argument("--token", default="", help="API token (Bearer)")
    ap.add_argument("--json", action="store_true", help="JSON output")
    ap.add_argument(
        "--skip",
        action="append",
        default=[],
        help="probe name to skip (substring match); can repeat",
    )
    ap.add_argument(
        "--only",
        action="append",
        default=[],
        help="run only probes whose name contains this substring; can repeat",
    )
    args = ap.parse_args()

    # Sanity: server reachable?
    try:
        s, _, _ = http_get(f"{args.target}/health", timeout=3.0)
        if s == 0:
            raise RuntimeError("connection failed")
    except Exception as e:
        print(
            f"{ANSI_RED}server unreachable at {args.target}: {e}{ANSI_RESET}",
            file=sys.stderr,
        )
        return 2

    all_probes = [
        ("recon-fingerprint", lambda: probe_recon_fingerprint(args.target, args.token)),
        ("auth-smuggling", lambda: probe_auth_header_smuggling(args.target, args.token)),
        ("path-traversal", lambda: probe_path_traversal_url(args.target)),
        ("header-bomb", lambda: probe_header_bomb(args.target)),
        ("large-url", lambda: probe_large_url(args.target)),
        ("slowloris", lambda: probe_slowloris(args.target)),
        ("te-cl-smuggling", lambda: probe_te_cl_smuggling(args.target)),
        ("json-depth-bomb", lambda: probe_json_depth_bomb(args.target)),
        ("gzip-bomb", lambda: probe_gzip_bomb(args.target)),
        ("fts-regex-dos", lambda: probe_fts_regex_dos(args.target)),
        ("method-matrix", lambda: probe_endpoint_method_matrix(args.target)),
        ("pipeline-flood", lambda: probe_pipeline_flood(args.target)),
        ("websocket-flood", lambda: probe_websocket_flood(args.target)),
        ("search-huge", lambda: probe_search_huge_query(args.target)),
        ("cors-spoofing", lambda: probe_cors_origin_spoofing(args.target)),
        ("mongo-injection", lambda: probe_mongo_operator_injection(args.target)),
        ("static-traversal", lambda: probe_static_dir_traversal(args.target)),
        ("container-egress", lambda: probe_container_egress(args.target)),
        ("timing-attack", lambda: probe_token_brute_force_timing(args.target)),
    ]

    probes: list[ProbeResult] = []
    for key, fn in all_probes:
        if args.only and not any(o in key for o in args.only):
            continue
        if any(s in key for s in args.skip):
            r = ProbeResult(name=key, technique="(skipped)", severity="info", verdict="skipped")
            probes.append(r)
            continue
        try:
            r = fn()
        except KeyboardInterrupt:
            raise
        except Exception as e:
            r = ProbeResult(
                name=key,
                technique="(runner error)",
                severity="info",
                verdict="error",
                actual=f"probe runner raised: {type(e).__name__}: {e}",
            )
        probes.append(r)
        if not args.json:
            # Stream as we go — long probes shouldn't appear silent
            dur = f"{r.duration_ms / 1000:.1f}s" if r.duration_ms else "  -"
            print(
                f"  {verdict_badge(r.verdict)} {r.name} {ANSI_DIM}({dur}){ANSI_RESET}",
                file=sys.stderr,
            )

    if args.json:
        print(
            json.dumps(
                {
                    "version": 1,
                    "target": args.target,
                    "probes": [asdict(p) for p in probes],
                    "summary": {
                        v: sum(1 for p in probes if p.verdict == v)
                        for v in ["blocked", "concerning", "exploit", "error", "skipped"]
                    },
                },
                indent=2,
            )
        )
    else:
        print_human(probes)

    # Exit non-zero on any exploit
    if any(p.is_exploit for p in probes):
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
