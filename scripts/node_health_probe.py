#!/usr/bin/env python3
"""Node health probe: one presence card for a running OpenStory node.

Experiment 1 from docs/research/openstory-as-node/2026-09-23-openstory-as-node.md.
Reads the tier 0 signals a node can already expose and folds them into one
verdict. Every fetch is optional. A missing endpoint is a finding, never a
crash. This is what a `node_health` MCP hand would return once the server
computes it in-process; until then the probe computes it from outside.

Sources read:
  server   GET /health, /api/health, /api/watchers, /api/fleet, /metrics
  nats     GET /varz, /jsz?streams=true&config=true, /leafz, /connz on :8222
  host     ps RSS of `open-story serve`, size of --data-dir on disk
  config   nats_leaf_url and stale_threshold_secs from --config

Usage:
  python3 scripts/node_health_probe.py                      # human card
  python3 scripts/node_health_probe.py --json               # one JSON object
  python3 scripts/node_health_probe.py --config data/config.toml --data-dir data
  python3 scripts/node_health_probe.py --test               # fixtures only

No third-party dependencies: urllib, json, subprocess, os only.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from datetime import datetime, timezone

# ── Thresholds ─────────────────────────────────────────────────────────────

DEFAULT_CAPS = {
    "events": 1_073_741_824,  # rs/bus/src/nats_bus.rs EVENTS_MAX_BYTES (1 GiB)
    "local": 1_073_741_824,  # same cap, home-only stream
    "patterns": 268_435_456,  # rs/bus/src/nats_bus.rs (256 MiB)
}
STREAM_WARN_PCT = 70.0
STREAM_CRIT_PCT = 90.0
WATCHER_WARN_SECS = 300  # config stale_threshold_secs default
WATCHER_CRIT_SECS = 3600

LEVELS = {"ok": 0, "info": 0, "warn": 1, "critical": 2}


# ── Pure functions ─────────────────────────────────────────────────────────


def finding(level: str, code: str, message: str, **extra) -> dict:
    d = {"level": level, "code": code, "message": message}
    d.update(extra)
    return d


def pct_of_cap(bytes_used: int, cap: int) -> float:
    """Percent of a byte cap used, floored to 2 places so it never overstates.

    1073700552 / 1 GiB is 99.996%; rounding would print 100.00 and claim a
    full stream that still has headroom. cap<=0 means unlimited.
    """
    if cap is None or cap <= 0:
        return 0.0
    return math.floor(10000.0 * bytes_used / cap) / 100.0


def level_for_pct(pct: float, warn: float = STREAM_WARN_PCT, crit: float = STREAM_CRIT_PCT) -> str:
    if pct >= crit:
        return "critical"
    if pct >= warn:
        return "warn"
    return "ok"


def parse_iso(ts: str | None) -> datetime | None:
    if not ts:
        return None
    try:
        if ts.endswith("Z"):
            ts = ts[:-1] + "+00:00"
        return datetime.fromisoformat(ts)
    except ValueError:
        return None


def age_seconds(ts: str | None, now: datetime) -> float | None:
    dt = parse_iso(ts)
    if dt is None:
        return None
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=timezone.utc)
    return max(0.0, (now - dt).total_seconds())


def level_for_age(age: float | None, warn: int = WATCHER_WARN_SECS, crit: int = WATCHER_CRIT_SECS) -> str:
    if age is None:
        return "info"
    if age > crit:
        return "critical"
    if age > warn:
        return "warn"
    return "ok"


def parse_streams(jsz: dict | None, caps: dict) -> list[dict]:
    """Flatten /jsz stream_detail into [{name, bytes, messages, cap, pct, cap_source}]."""
    if not jsz:
        return []
    out = []
    for acc in jsz.get("account_details") or []:
        for s in acc.get("stream_detail") or []:
            name = s.get("name", "?")
            state = s.get("state") or {}
            cfg = s.get("config") or {}
            cap = cfg.get("max_bytes")
            cap_source = "jsz.config"
            if cap is None:
                cap = caps.get(name)
                cap_source = "default" if cap else "none"
            if cap is not None and cap < 0:
                cap = None
                cap_source = "unlimited"
            used = int(state.get("bytes") or 0)
            out.append(
                {
                    "name": name,
                    "bytes": used,
                    "messages": int(state.get("messages") or 0),
                    "cap": cap,
                    "cap_source": cap_source,
                    "pct": pct_of_cap(used, cap) if cap else None,
                    "first_ts": state.get("first_ts"),
                    "last_ts": state.get("last_ts"),
                    "discard": cfg.get("discard"),
                }
            )
    return out


def stream_findings(streams: list[dict]) -> list[dict]:
    out = []
    for s in streams:
        if s["pct"] is None:
            continue
        lvl = level_for_pct(s["pct"])
        if lvl == "ok":
            continue
        note = ""
        if s.get("discard") == "old":
            note = " (discard=old: oldest messages are being evicted, not refused)"
        out.append(
            finding(
                lvl,
                "stream_cap",
                f"stream {s['name']} at {s['pct']}% of cap "
                f"({s['bytes']:,} / {s['cap']:,} bytes, {s['messages']:,} msgs){note}",
                stream=s["name"],
                pct=s["pct"],
            )
        )
    return out


def parse_watchers(api: dict | None, now: datetime) -> list[dict]:
    if not api:
        return []
    out = []
    for w in api.get("watchers") or []:
        last = w.get("last_event_at")
        counters = w.get("counters") or {}
        out.append(
            {
                "actor": w.get("actor", "?"),
                "last_event_at": last,
                "age_secs": age_seconds(last, now),
                "publish_failures": int(counters.get("publish_failures") or 0),
                "events_emitted": int(counters.get("cloud_events_emitted") or 0),
            }
        )
    return out


def watcher_findings(watchers: list[dict]) -> list[dict]:
    out = []
    for w in watchers:
        age = w["age_secs"]
        lvl = level_for_age(age)
        if age is None:
            out.append(
                finding(
                    "info",
                    "watcher_age_unknown",
                    f"watcher {w['actor']}: no last_event_at yet (never emitted since boot)",
                    actor=w["actor"],
                )
            )
        elif lvl != "ok":
            out.append(
                finding(
                    lvl,
                    "watcher_stale",
                    f"watcher {w['actor']}: last event {int(age)}s ago",
                    actor=w["actor"],
                    age_secs=int(age),
                )
            )
        if w["publish_failures"] > 0:
            out.append(
                finding(
                    "warn",
                    "watcher_publish_failures",
                    f"watcher {w['actor']}: {w['publish_failures']} publish failures since boot",
                    actor=w["actor"],
                    count=w["publish_failures"],
                )
            )
    return out


def leaf_state(leafz: dict | None) -> dict:
    if not leafz:
        return {"available": False, "connected": False, "count": 0, "upstreams": []}
    leafs = leafz.get("leafs") or []
    ups = [
        {"ip": l.get("ip"), "port": l.get("port"), "rtt": l.get("rtt"), "is_spoke": l.get("is_spoke")}
        for l in leafs
    ]
    return {
        "available": True,
        "connected": len(leafs) > 0,
        "count": int(leafz.get("leafnodes") or len(leafs)),
        "upstreams": ups,
    }


def leaf_findings(leaf: dict, leaf_configured: bool, leaf_host: str | None) -> list[dict]:
    out = []
    if leaf_configured and not leaf["connected"]:
        why = "NATS /leafz reports 0 leaf connections" if leaf["available"] else "/leafz unreachable"
        out.append(
            finding(
                "critical",
                "leaf_not_connected",
                f"nats_leaf_url is set (hub {leaf_host or '?'}) but {why}: "
                "this node is solo; fleet will not see its sessions",
                hub=leaf_host,
            )
        )
    elif not leaf_configured and leaf["connected"]:
        out.append(
            finding("info", "leaf_unexpected", "leaf connected but no nats_leaf_url in config")
        )
    return out


def redact_leaf_url(url: str) -> str | None:
    """nats://TOKEN@host:port -> host:port. Never echo the token."""
    if not url:
        return None
    m = re.match(r"^[a-z]+://(?:[^@/]+@)?([^/]+)", url.strip())
    return m.group(1) if m else "?"


def parse_config(text: str | None) -> dict:
    """Minimal TOML line scan for the two keys the probe needs."""
    cfg = {"nats_leaf_url": "", "stale_threshold_secs": None, "metrics_enabled": None}
    if not text:
        return cfg
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        m = re.match(r'^(\w+)\s*=\s*(.+?)\s*$', line)
        if not m:
            continue
        k, v = m.group(1), m.group(2)
        v = v.split("#")[0].strip().strip('"')
        if k == "nats_leaf_url":
            cfg["nats_leaf_url"] = v
        elif k == "stale_threshold_secs":
            try:
                cfg["stale_threshold_secs"] = int(v)
            except ValueError:
                pass
        elif k == "metrics_enabled":
            cfg["metrics_enabled"] = v.lower() == "true"
    return cfg


def verdict(findings: list[dict]) -> str:
    worst = 0
    for f in findings:
        worst = max(worst, LEVELS.get(f["level"], 0))
    return {0: "ok", 1: "warn", 2: "critical"}[worst]


def assess(
    *,
    health: dict | None,
    api_health: dict | None,
    watchers_api: dict | None,
    fleet: dict | None,
    metrics_text: str | None,
    varz: dict | None,
    jsz: dict | None,
    leafz: dict | None,
    connz: dict | None,
    fetch_errors: dict,
    config: dict,
    caps: dict,
    server_rss_kb: int | None,
    store_bytes: int | None,
    now: datetime,
) -> dict:
    """Fold every signal into one report. Pure: takes data, returns data."""
    findings: list[dict] = []

    # Missing endpoints are findings, never crashes.
    for name, err in fetch_errors.items():
        if err is None:
            continue
        if name in ("metrics",):
            findings.append(finding("info", "endpoint_missing", f"no /metrics on this node ({err}); metrics_enabled is off"))
        elif name == "api_health":
            findings.append(finding("warn", "endpoint_missing", f"no /api/health on this build ({err})"))
        elif name.startswith("nats_"):
            findings.append(finding("info", "endpoint_missing", f"NATS monitor {name[5:]} unavailable ({err})"))
        else:
            findings.append(finding("info", "endpoint_missing", f"{name} unavailable ({err})"))

    streams = parse_streams(jsz, caps)
    findings += stream_findings(streams)

    watchers = parse_watchers(watchers_api, now)
    findings += watcher_findings(watchers)

    leaf = leaf_state(leafz)
    leaf_url = config.get("nats_leaf_url") or ""
    leaf_host = redact_leaf_url(leaf_url)
    findings += leaf_findings(leaf, bool(leaf_url), leaf_host)

    if api_health:
        bus = (api_health.get("bus") or {}).get("connected")
        if bus is False:
            findings.append(finding("critical", "bus_disconnected", "/api/health reports bus.connected=false"))
        proj = api_health.get("projections") or {}
        if proj.get("fresh") is False:
            findings.append(finding("warn", "projections_stale", f"projections {proj.get('count')} < sessions {proj.get('sessions')}; run reproject"))

    nats_mem = varz.get("mem") if varz else None
    slow = varz.get("slow_consumers") if varz else None
    if slow:
        findings.append(finding("warn", "nats_slow_consumers", f"NATS reports {slow} slow consumers since start"))

    report = {
        "probe_at": now.isoformat(),
        "verdict": verdict(findings),
        "findings": findings,
        "server": {
            "liveness": health,
            "version": (api_health or {}).get("version"),
            "store_backend": ((api_health or {}).get("store") or {}).get("backend"),
            "sessions": ((api_health or {}).get("store") or {}).get("sessions"),
            "bus_connected": ((api_health or {}).get("bus") or {}).get("connected"),
            "projections": (api_health or {}).get("projections"),
            "rss_kb": server_rss_kb,
            "metrics_lines": len(metrics_text.splitlines()) if metrics_text else 0,
        },
        "watchers": watchers,
        "streams": streams,
        "nats": {
            "version": varz.get("version") if varz else None,
            "uptime": varz.get("uptime") if varz else None,
            "mem_bytes": nats_mem,
            "connections": varz.get("connections") if varz else None,
            "slow_consumers": slow,
            "jetstream_bytes": jsz.get("bytes") if jsz else None,
        },
        "leaf": {**leaf, "configured": bool(leaf_url), "hub": leaf_host},
        "fleet": {
            "person": ((fleet or {}).get("person") or {}).get("display_name"),
            "principals": len((fleet or {}).get("principals") or []),
        },
        "store_bytes_on_disk": store_bytes,
    }
    return report


# ── Side effects: fetch, ps, disk ──────────────────────────────────────────


def fetch(url: str, timeout: float = 4.0, as_json: bool = True):
    """Return (payload, None) or (None, error_string). Never raises."""
    try:
        with urllib.request.urlopen(url, timeout=timeout) as r:
            body = r.read().decode("utf-8", "replace")
            if not as_json:
                return body, None
            return json.loads(body), None
    except urllib.error.HTTPError as e:
        return None, f"HTTP {e.code}"
    except urllib.error.URLError as e:
        return None, f"unreachable: {e.reason}"
    except (json.JSONDecodeError, TimeoutError, OSError) as e:
        return None, f"{type(e).__name__}: {e}"


def find_server_pid(pattern: str = "open-story serve") -> int | None:
    try:
        out = subprocess.run(["pgrep", "-f", pattern], capture_output=True, text=True, timeout=5)
        pids = [int(p) for p in out.stdout.split() if p.strip().isdigit()]
        # pgrep -f also matches this probe's own argv if it mentions the pattern; skip self.
        pids = [p for p in pids if p != os.getpid()]
        return pids[0] if pids else None
    except (OSError, subprocess.SubprocessError, ValueError):
        return None


def rss_kb(pid: int) -> int | None:
    try:
        out = subprocess.run(["ps", "-o", "rss=", "-p", str(pid)], capture_output=True, text=True, timeout=5)
        v = out.stdout.strip()
        return int(v) if v.isdigit() else None
    except (OSError, subprocess.SubprocessError):
        return None


def dir_bytes(path: str) -> int | None:
    if not path or not os.path.isdir(path):
        return None
    total = 0
    for root, _dirs, files in os.walk(path):
        for f in files:
            try:
                total += os.lstat(os.path.join(root, f)).st_size
            except OSError:
                pass
    return total


def read_text(path: str | None) -> str | None:
    if not path:
        return None
    try:
        with open(path, encoding="utf-8") as fh:
            return fh.read()
    except OSError:
        return None


def probe(args) -> dict:
    api = args.api.rstrip("/")
    nats = args.nats_monitor.rstrip("/")
    errs: dict = {}

    def get(name, url, as_json=True):
        payload, err = fetch(url, as_json=as_json)
        errs[name] = err
        return payload

    health = get("health", f"{api}/health")
    api_health = get("api_health", f"{api}/api/health")
    watchers_api = get("watchers", f"{api}/api/watchers")
    fleet = get("fleet", f"{api}/api/fleet")
    metrics_text = get("metrics", f"{api}/metrics", as_json=False)
    varz = get("nats_varz", f"{nats}/varz")
    jsz = get("nats_jsz", f"{nats}/jsz?streams=true&config=true")
    leafz = get("nats_leafz", f"{nats}/leafz")
    connz = get("nats_connz", f"{nats}/connz")

    config = parse_config(read_text(args.config))
    caps = dict(DEFAULT_CAPS)
    if args.events_cap:
        caps["events"] = args.events_cap

    pid = args.pid or find_server_pid()
    server_rss = rss_kb(pid) if pid else None
    store_bytes = dir_bytes(args.data_dir) if args.data_dir else None

    report = assess(
        health=health,
        api_health=api_health,
        watchers_api=watchers_api,
        fleet=fleet,
        metrics_text=metrics_text,
        varz=varz,
        jsz=jsz,
        leafz=leafz,
        connz=connz,
        fetch_errors=errs,
        config=config,
        caps=caps,
        server_rss_kb=server_rss,
        store_bytes=store_bytes,
        now=datetime.now(timezone.utc),
    )
    report["server"]["pid"] = pid
    report["sources"] = {"api": api, "nats_monitor": nats, "config": args.config, "data_dir": args.data_dir}
    return report


# ── Rendering ──────────────────────────────────────────────────────────────


def human(report: dict) -> str:
    def gb(b):
        return "-" if b is None else f"{b / 1e9:.2f} GB"

    s = report["server"]
    n = report["nats"]
    lf = report["leaf"]
    lines = [
        f"OpenStory node health  {report['probe_at'][:19]}Z   verdict: {report['verdict'].upper()}",
        "",
        f"  server   version={s.get('version')} store={s.get('store_backend')} sessions={s.get('sessions')} "
        f"bus_connected={s.get('bus_connected')} pid={s.get('pid')} rss={'-' if s.get('rss_kb') is None else str(round(s['rss_kb'] / 1024)) + ' MB'}",
        f"  nats     version={n.get('version')} uptime={n.get('uptime')} mem={gb(n.get('mem_bytes'))} conns={n.get('connections')} slow_consumers={n.get('slow_consumers')}",
        f"  leaf     configured={lf['configured']} connected={lf['connected']} hub={lf.get('hub')}",
        f"  fleet    person={report['fleet']['person']} principals={report['fleet']['principals']}",
        f"  store    on_disk={gb(report.get('store_bytes_on_disk'))}",
        "",
        "  streams",
    ]
    for st in report["streams"]:
        pct = "-" if st["pct"] is None else f"{st['pct']:6.2f}%"
        cap = "unlimited" if st["cap"] is None else f"{st['cap'] / 2**20:.0f} MiB"
        lines.append(f"    {st['name']:<10} {st['bytes'] / 2**20:9.1f} MiB / {cap:<10} {pct}  msgs={st['messages']:,}")
    if not report["streams"]:
        lines.append("    (no /jsz data)")
    lines.append("")
    lines.append("  watchers")
    for w in report["watchers"]:
        age = "unknown" if w["age_secs"] is None else f"{int(w['age_secs'])}s"
        lines.append(f"    {w['actor']:<60} age={age:<8} publish_failures={w['publish_failures']}")
    if not report["watchers"]:
        lines.append("    (no /api/watchers data)")
    lines.append("")
    lines.append("  findings")
    for f in report["findings"]:
        lines.append(f"    [{f['level']:<8}] {f['message']}")
    if not report["findings"]:
        lines.append("    none")
    return "\n".join(lines)


# ── Fixtures and self-test ─────────────────────────────────────────────────

FIX_VARZ = {
    "server_id": "NAJ6JE6KHZ", "version": "2.14.3", "uptime": "3h4m7s", "mem": 723877888,
    "connections": 8, "leafnodes": 0, "slow_consumers": 6, "in_msgs": 18053,
}
FIX_JSZ = {
    "streams": 5, "messages": 7189, "bytes": 1134513886,
    "account_details": [{"name": "$G", "stream_detail": [
        {"name": "events", "config": {"max_bytes": 1073741824, "retention": "limits", "discard": "old"},
         "state": {"messages": 6453, "bytes": 1073700552, "first_seq": 169683, "last_seq": 176149}},
        {"name": "patterns", "config": {"max_bytes": 268435456, "retention": "limits", "discard": "old"},
         "state": {"messages": 736, "bytes": 60813334}},
        {"name": "ui", "config": {"max_bytes": -1, "retention": "interest", "discard": "old"},
         "state": {"messages": 0, "bytes": 0}},
    ]}],
}
# /jsz?streams=true without config=true: no max_bytes, so the default cap applies.
FIX_JSZ_NOCONFIG = {
    "account_details": [{"stream_detail": [
        {"name": "events", "state": {"messages": 100, "bytes": 751_619_277}},  # 70.0% of 1 GiB
    ]}],
}
# Real /leafz shapes: the spoke fixture is from rs/server/src/admin.rs tests.
FIX_LEAFZ_CONNECTED = {
    "server_id": "NBJ7H2AIFQHMIOQ", "leafnodes": 1,
    "leafs": [{"id": 5, "name": "NBHHLYXLZTUAU4W", "is_spoke": True, "ip": "100.77.40.95", "port": 7422, "rtt": "38.985481ms"}],
}
FIX_LEAFZ_NONE = {"server_id": "NAJ6JE6KHZ", "leafnodes": 0, "leafs": []}
FIX_HEALTH = {"role": "full", "status": "ok"}
FIX_API_HEALTH = {
    "bus": {"connected": True}, "projections": {"count": 2448, "fresh": True, "sessions": 2447},
    "status": "ok", "store": {"backend": "sqlite", "sessions": 2447}, "version": "0.4.0", "watchers": 3,
}
FIX_WATCHERS = {"watchers": [
    {"actor": "claude-code:/home/u/.claude/projects", "last_event_at": "2026-09-24T01:06:04.527Z",
     "counters": {"cloud_events_emitted": 27450, "publish_failures": 10}},
    {"actor": "codex:/home/u/.codex/sessions", "last_event_at": None,
     "counters": {"cloud_events_emitted": 0, "publish_failures": 1}},
    {"actor": "grok:/home/u/.grok/sessions", "last_event_at": "2026-09-23T20:11:54.911Z",
     "counters": {"cloud_events_emitted": 3927, "publish_failures": 0}},
]}
FIX_FLEET = {"person": {"display_name": "You"}, "principals": [{"display_name": "Maxs-Air"}]}
FIX_CONFIG_LEAF = 'port = 3002\nnats_leaf_url = "nats://SECRET@debian-16gb-ash-1:7422"\nstale_threshold_secs = 300\nmetrics_enabled = false\n'
FIX_CONFIG_SOLO = 'port = 3002\nnats_leaf_url = ""\n'
FIX_NOW = datetime(2026, 9, 24, 1, 7, 0, tzinfo=timezone.utc)


def _base_kwargs(**over):
    kw = dict(
        health=FIX_HEALTH, api_health=FIX_API_HEALTH, watchers_api=FIX_WATCHERS, fleet=FIX_FLEET,
        metrics_text=None, varz=FIX_VARZ, jsz=FIX_JSZ, leafz=FIX_LEAFZ_NONE, connz=None,
        fetch_errors={"metrics": "HTTP 404"}, config=parse_config(FIX_CONFIG_SOLO), caps=dict(DEFAULT_CAPS),
        server_rss_kb=222464, store_bytes=11_000_000_000, now=FIX_NOW,
    )
    kw.update(over)
    return kw


def run_tests() -> int:
    checks = 0

    def ok(cond, msg):
        nonlocal checks
        checks += 1
        if not cond:
            print(f"FAIL: {msg}")
            sys.exit(1)
        print(f"ok   {msg}")

    # 1. percent math with the live numbers
    p = pct_of_cap(1073700552, 1073741824)
    ok(abs(p - 99.99) < 0.01, f"events 1073700552/1 GiB = {p}%")
    ok(pct_of_cap(60813334, 268435456) == 22.65, "patterns 60813334/256 MiB = 22.65%")
    ok(pct_of_cap(1, 0) == 0.0 and pct_of_cap(1, None) == 0.0, "cap 0/None = unlimited = 0%")

    # 2. stream thresholds
    ok(level_for_pct(69.99) == "ok" and level_for_pct(70.0) == "warn" and level_for_pct(90.0) == "critical",
       "stream levels: <70 ok, 70 warn, 90 critical")
    streams = parse_streams(FIX_JSZ, DEFAULT_CAPS)
    ev = next(s for s in streams if s["name"] == "events")
    ok(ev["cap"] == 1073741824 and ev["cap_source"] == "jsz.config", "events cap read from jsz config")
    ui = next(s for s in streams if s["name"] == "ui")
    ok(ui["cap"] is None and ui["pct"] is None, "max_bytes -1 means unlimited, no pct")
    sf = stream_findings(streams)
    ok(len(sf) == 1 and sf[0]["level"] == "critical" and sf[0]["stream"] == "events", "only events fires, and it is critical")
    ok("discard=old" in sf[0]["message"], "discard=old eviction note attached")
    noconf = parse_streams(FIX_JSZ_NOCONFIG, DEFAULT_CAPS)
    ok(noconf[0]["cap_source"] == "default" and noconf[0]["pct"] == 70.0, "no config -> default cap, 751619277 B = 70.00%")
    ok(stream_findings(noconf)[0]["level"] == "warn", "70.00% is warn (boundary inclusive)")
    custom = parse_streams(FIX_JSZ_NOCONFIG, {"events": 2 * 1073741824})
    ok(custom[0]["pct"] == 35.0, "--events-cap 2 GiB halves the percent to 35.00%")

    # 3. watcher ages
    ws = parse_watchers(FIX_WATCHERS, FIX_NOW)
    cc = next(w for w in ws if w["actor"].startswith("claude-code"))
    ok(int(cc["age_secs"]) == 55, f"claude-code age = {int(cc['age_secs'])}s (55 expected)")
    gk = next(w for w in ws if w["actor"].startswith("grok"))
    ok(int(gk["age_secs"]) == 17705 and level_for_age(gk["age_secs"]) == "critical", "grok age 17705s -> critical (>3600)")
    ok(level_for_age(300) == "ok" and level_for_age(301) == "warn" and level_for_age(3601) == "critical", "age levels: 300 ok, 301 warn, 3601 critical")
    ok(level_for_age(None) == "info", "unknown age is info, not a failure")
    wf = watcher_findings(ws)
    codes = sorted(f["code"] for f in wf)
    ok(codes == ["watcher_age_unknown", "watcher_publish_failures", "watcher_publish_failures", "watcher_stale"],
       f"watcher findings = {codes}")

    # 4. leaf logic and token redaction
    cfg = parse_config(FIX_CONFIG_LEAF)
    ok(cfg["nats_leaf_url"].startswith("nats://") and cfg["stale_threshold_secs"] == 300 and cfg["metrics_enabled"] is False,
       "config parse: leaf url, stale threshold, metrics flag")
    ok(redact_leaf_url(cfg["nats_leaf_url"]) == "debian-16gb-ash-1:7422", "leaf url redacted to host:port")
    lf = leaf_findings(leaf_state(FIX_LEAFZ_NONE), True, "hub:7422")
    ok(len(lf) == 1 and lf[0]["level"] == "critical" and lf[0]["code"] == "leaf_not_connected", "leaf configured + 0 leafs = critical")
    ok(leaf_findings(leaf_state(FIX_LEAFZ_CONNECTED), True, "hub:7422") == [], "leaf configured + spoke connected = no finding")
    ok(leaf_findings(leaf_state(FIX_LEAFZ_NONE), False, None) == [], "solo node with 0 leafs = no finding")
    ok(leaf_state(FIX_LEAFZ_CONNECTED)["upstreams"][0]["ip"] == "100.77.40.95", "admin.rs leafz fixture parses upstream ip")

    # 5. whole-report verdicts
    r = assess(**_base_kwargs())
    ok(r["verdict"] == "critical", "live-shaped fixture: verdict critical (events at cap + grok stale)")
    ok(any(f["code"] == "endpoint_missing" and "/metrics" in f["message"] for f in r["findings"]), "missing /metrics is an info finding")
    ok(any(f["code"] == "nats_slow_consumers" for f in r["findings"]), "6 slow consumers -> warn finding")
    r2 = assess(**_base_kwargs(config=parse_config(FIX_CONFIG_LEAF)))
    ok(any(f["code"] == "leaf_not_connected" for f in r2["findings"]) and "SECRET" not in json.dumps(r2), "leaf configured on solo NATS fires, token never appears in report")
    healthy_jsz = {"account_details": [{"stream_detail": [{"name": "events", "config": {"max_bytes": 1073741824}, "state": {"bytes": 1000, "messages": 1}}]}]}
    healthy_watch = {"watchers": [{"actor": "claude-code:x", "last_event_at": "2026-09-24T01:06:30Z", "counters": {}}]}
    r3 = assess(**_base_kwargs(jsz=healthy_jsz, watchers_api=healthy_watch, varz={**FIX_VARZ, "slow_consumers": 0}, fetch_errors={}))
    ok(r3["verdict"] == "ok" and r3["findings"] == [], "healthy fixture: verdict ok, no findings")
    r4 = assess(**_base_kwargs(api_health=None, jsz=None, varz=None, leafz=None,
                                fetch_errors={"api_health": "HTTP 404", "nats_varz": "unreachable", "nats_jsz": "unreachable", "nats_leafz": "unreachable"}))
    ok(r4["verdict"] == "critical" and any("no /api/health on this build" in f["message"] for f in r4["findings"]),
       "old build without /api/health and no :8222: findings, no crash (critical from the stale grok watcher)")
    ok(assess(**_base_kwargs(api_health={**FIX_API_HEALTH, "bus": {"connected": False}}))["findings"][-1]["code"] != "", "bus.connected=false handled")
    ok(any(f["code"] == "bus_disconnected" for f in assess(**_base_kwargs(api_health={**FIX_API_HEALTH, "bus": {"connected": False}}))["findings"]),
       "bus.connected=false -> critical bus_disconnected")
    ok(verdict([]) == "ok" and verdict([finding("info", "x", "y")]) == "ok" and verdict([finding("warn", "x", "y")]) == "warn", "verdict folds levels")
    txt = human(r)
    ok("verdict: CRITICAL" in txt and "events" in txt, "human card renders verdict and streams")

    print(f"\n{checks} assertions passed")
    return 0


# ── CLI ────────────────────────────────────────────────────────────────────


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--api", default="http://localhost:3002", help="OpenStory API base URL")
    ap.add_argument("--nats-monitor", default="http://localhost:8222", help="NATS HTTP monitoring base URL")
    ap.add_argument("--config", default=None, help="path to data/config.toml (reads nats_leaf_url, stale_threshold_secs)")
    ap.add_argument("--data-dir", default=None, help="data dir to size on disk")
    ap.add_argument("--pid", type=int, default=None, help="server pid for RSS (default: pgrep -f 'open-story serve')")
    ap.add_argument("--events-cap", type=int, default=None, help="override events stream cap in bytes when /jsz has no config")
    ap.add_argument("--json", action="store_true", help="emit one JSON object")
    ap.add_argument("--test", action="store_true", help="run fixture self-tests and exit")
    args = ap.parse_args(argv)

    if args.test:
        return run_tests()

    report = probe(args)
    if args.json:
        print(json.dumps(report, indent=2, default=str))
    else:
        print(human(report))
    return {"ok": 0, "warn": 1, "critical": 2}[report["verdict"]]


if __name__ == "__main__":
    sys.exit(main())
