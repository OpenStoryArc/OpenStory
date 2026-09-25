#!/usr/bin/env python3
"""Boot-memory harness: how much RSS does `open-story serve` need to boot?

Row B-00 of docs/research/openstory-as-node/2026-09-25-boot-pass-memory.md.

Builds a synthetic store shaped like a fleet member's (N sessions of
translated CloudEvents on disk as `data_dir/*.jsonl`, with a tail of large
agent sessions), boots the real binary against it twice (the first boot
ingests the JSONL into SQLite; the second is the one measured), samples the
process RSS once a second, reads `/api/health` once a second, and reports
peak RSS per boot phase:

  no-listen   nothing answers yet: reconcile + boot pass + working-set reproject
  starting    503, `boot.phase = starting` (bound, replay not begun)
  replaying   503, `boot.phase = replaying`
  serving     200
  settle      one sample taken `--settle-secs` after the first 200

The gate: peak RSS of the measured boot under `--gate-gb`.

Usage:
  python3 scripts/boot_memory.py --fixture DIR              # build (if needed), boot twice, report
  python3 scripts/boot_memory.py --fixture DIR --json out.json --gate-gb 2.0
  python3 scripts/boot_memory.py --fixture DIR --sessions 1259 --large 40 --total-gb 3.0
  python3 scripts/boot_memory.py --test                     # pure-function specs only

The fixture is synthetic: field names follow the translator's CloudEvent
shape (specversion/id/source/type/subtype/time/agent/data.{seq,session_id,
raw,agent_payload}); every string is generated. No real session text or id
is ever read or copied. Keep the fixture out of git.

No third-party dependencies: json, subprocess, urllib, random only.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path

# ── Shape constants ────────────────────────────────────────────────────────

AGENT = "claude-code"
EVENT_TYPE = "io.arc.event"
TS_FORMAT = "%Y-%m-%dT%H:%M:%S.%fZ"
# One agent turn, as the translator emits it for a Claude Code transcript.
TURN = (
    "message.user.prompt",
    "message.assistant.thinking",
    "message.assistant.text",
    "message.assistant.tool_use",
    "message.user.tool_result",
    "message.assistant.tool_use",
    "message.user.tool_result",
    "message.assistant.text",
    "system.turn.complete",
)
TOOL_RESULT = "message.user.tool_result"
# A tool output is carried three times by a translated tool_result event
# (raw.message.content[].content, raw.toolUseResult.stdout, agent_payload.text).
OUTPUT_COPIES = 3
LARGE_SHARE = 0.6  # fraction of the byte budget that the large tail carries

PHASE_ORDER = ("no-listen", "starting", "replaying", "serving", "settle")


# ── Pure functions ─────────────────────────────────────────────────────────


def fmt_ts(dt: datetime) -> str:
    """The translator's timestamp format: millisecond precision, `Z`."""
    return dt.strftime("%Y-%m-%dT%H:%M:%S.") + f"{dt.microsecond // 1000:03d}Z"


def classify(status: int | None, body: dict | None) -> str:
    """Phase of one health sample.

    None status (connection refused) → `no-listen`; 200 → `serving`;
    503 → the body's `boot.phase` (`starting` / `replaying`); anything
    else → `other-<status>`.
    """
    if status is None:
        return "no-answer" if body and body.get("timeout") else "no-listen"
    if status == 200:
        return "serving"
    if status == 503:
        phase = (body or {}).get("boot", {}).get("phase")
        return phase if phase in ("starting", "replaying") else "replaying"
    return f"other-{status}"


def rss_bytes_from_ps(text: str) -> int | None:
    """`ps -o rss= -p PID` prints kilobytes; None when the process is gone."""
    text = text.strip()
    if not text:
        return None
    return int(text.split()[0]) * 1024


def phase_peaks(samples: list[dict]) -> dict[str, dict]:
    """Peak RSS per phase over 1 Hz samples of `{t, rss_bytes, phase}`."""
    seen: dict[str, dict] = {}
    for s in samples:
        cur = seen.get(s["phase"])
        if cur is None:
            seen[s["phase"]] = {
                "peak_rss_bytes": s["rss_bytes"],
                "samples": 1,
                "first_t": s["t"],
                "last_t": s["t"],
            }
        else:
            cur["peak_rss_bytes"] = max(cur["peak_rss_bytes"], s["rss_bytes"])
            cur["samples"] += 1
            cur["last_t"] = s["t"]
    ordered = [p for p in PHASE_ORDER if p in seen] + [p for p in seen if p not in PHASE_ORDER]
    return {p: seen[p] for p in ordered}


def plan_sessions(
    n_sessions: int,
    n_large: int,
    total_bytes: int,
    large_events: int,
    rng: random.Random,
    now: datetime,
) -> list[dict]:
    """Decide each session's id, parent, size, and start time.

    The large tail carries `LARGE_SHARE` of the byte budget split evenly;
    the rest is split evenly over the small sessions. One session in ten
    is a subagent whose events carry its parent's id in `data.session_id`.
    Start times spread over the last 180 days; the last twenty land inside
    the last five days so the working-set reproject has something to do.
    """
    n_small = n_sessions - n_large
    large_each = int(LARGE_SHARE * total_bytes / n_large) if n_large else 0
    small_each = int((total_bytes - large_each * n_large) / n_small) if n_small else 0
    large_idx = set(rng.sample(range(n_sessions), n_large))
    ids = [str(uuid.UUID(int=rng.getrandbits(128), version=4)) for _ in range(n_sessions)]
    plans: list[dict] = []
    for i, sid in enumerate(ids):
        is_large = i in large_idx
        target = large_each if is_large else small_each
        # A subagent's file carries its own id; its events carry the parent's.
        is_sub = i % 10 == 5 and i > 0
        parent_id = None
        if is_sub:
            candidates = [j for j in range(max(0, i - 40), i) if j % 10 != 5]
            parent_id = ids[rng.choice(candidates)]
        days_ago = rng.uniform(0, 5) if i >= n_sessions - 20 else rng.uniform(5, 180)
        plans.append(
            {
                "id": sid,
                "parent_id": parent_id,
                "cwd": f"/home/bm/projects/proj-{rng.randrange(60)}",
                "start": now - timedelta(days=days_ago),
                "events": large_events if is_large else max(len(TURN), round(target / 2600)),
                "target_bytes": target,
                "large": is_large,
            }
        )
    return plans


def make_event(
    rng: random.Random,
    plan: dict,
    seq: int,
    subtype: str,
    at: datetime,
    output_bytes: int,
) -> dict:
    """One translated CloudEvent (field names only; every string synthetic)."""
    own = plan["id"]
    ts = fmt_ts(at)
    role = "user" if subtype.startswith("message.user") else "assistant" if subtype.startswith("message.assistant") else "system"
    is_sidechain = plan["parent_id"] is not None
    raw: dict = {
        "cwd": plan["cwd"],
        "entrypoint": "cli",
        "gitBranch": "main",
        "isSidechain": is_sidechain,
        "parentUuid": None if seq == 1 else _uuid(rng),
        "sessionId": own,
        "timestamp": ts,
        "type": role,
        "userType": "external",
        "uuid": _uuid(rng),
        "version": "2.1.0",
    }
    payload: dict = {
        "_variant": AGENT,
        "meta": {"agent": AGENT},
        "cwd": plan["cwd"],
        "git_branch": "main",
        "is_sidechain": is_sidechain,
        "timestamp": ts,
        "uuid": raw["uuid"],
        "version": "2.1.0",
    }
    if subtype == "message.user.prompt":
        text = words(rng, 14)
        raw["message"] = {"role": "user", "content": text}
        raw["promptId"] = _uuid(rng)
        payload["text"] = text
        payload["user_type"] = "external"
    elif subtype == "message.assistant.thinking":
        text = words(rng, 40)
        _assistant(rng, raw, payload, [{"type": "thinking", "thinking": text, "signature": hexs(rng, 200)}])
        payload["content_types"] = ["thinking"]
    elif subtype == "message.assistant.text":
        text = words(rng, 60)
        _assistant(rng, raw, payload, [{"type": "text", "text": text}])
        payload["content_types"] = ["text"]
    elif subtype == "message.assistant.tool_use":
        cmd = "cargo test -p crate-" + hexs(rng, 6) + " -- " + words(rng, 3)
        tool_id = "toolu_" + hexs(rng, 24)
        _assistant(rng, raw, payload, [{"type": "tool_use", "id": tool_id, "name": "Bash", "input": {"command": cmd, "description": words(rng, 5)}}])
        payload["content_types"] = ["tool_use"]
        payload["tool"] = "Bash"
        payload["args"] = {"command": cmd, "description": words(rng, 5)}
    elif subtype == TOOL_RESULT:
        out = output_text(rng, output_bytes)
        tool_id = "toolu_" + hexs(rng, 24)
        raw["message"] = {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tool_id, "content": out, "is_error": False}]}
        raw["toolUseResult"] = {"stdout": out, "stderr": "", "interrupted": False, "isImage": False}
        raw["sourceToolAssistantUUID"] = _uuid(rng)
        payload["text"] = out
        payload["parent_uuid"] = raw["parentUuid"]
        payload["tool_outcome"] = {"type": "bash", "command": "cargo test", "succeeded": True}
        payload["user_type"] = "external"
    else:  # system.turn.complete and anything else: the envelope alone
        raw["subtype"] = subtype.rsplit(".", 1)[-1]
    return {
        "specversion": "1.0",
        "id": _uuid(rng),
        "source": f"arc://transcript/{own}",
        "type": EVENT_TYPE,
        "subtype": subtype,
        "time": ts,
        "datacontenttype": "application/json",
        "agent": AGENT,
        "host": "bm",
        "user": "bm",
        "data": {
            "seq": seq,
            "session_id": plan["parent_id"] or own,
            "raw": raw,
            "agent_payload": payload,
        },
    }


def _uuid(rng: random.Random) -> str:
    return str(uuid.UUID(int=rng.getrandbits(128), version=4))


SYLLABLES = ("ka", "to", "mi", "re", "sol", "ven", "dor", "lin", "ash", "quo", "zen", "pal", "ur", "ith", "ova")


def words(rng: random.Random, n: int) -> str:
    return " ".join("".join(rng.choice(SYLLABLES) for _ in range(rng.randint(1, 3))) for _ in range(n))


def hexs(rng: random.Random, n: int) -> str:
    return "%0*x" % (n, rng.getrandbits(4 * n))


def output_text(rng: random.Random, n: int) -> str:
    """`n` bytes of tool output: numbered lines of plain ASCII (JSON-safe, so
    its JSON length equals `n`)."""
    if n <= 0:
        return ""
    lines = []
    size = 0
    i = 0
    while size < n:
        line = f"{i:6d}  src/module_{i % 97}.rs:{rng.randrange(1, 900)}: {words(rng, 6)}\n"
        lines.append(line)
        size += len(line)
        i += 1
    return "".join(lines)[:n]


def _usage(rng: random.Random) -> dict:
    return {
        "input_tokens": rng.randrange(1, 40),
        "output_tokens": rng.randrange(20, 900),
        "cache_creation_input_tokens": rng.randrange(0, 4000),
        "cache_read_input_tokens": rng.randrange(10000, 90000),
        "service_tier": "standard",
    }


def _assistant(rng: random.Random, raw: dict, payload: dict, content: list) -> None:
    msg_id = "msg_" + hexs(rng, 24)
    usage = _usage(rng)
    raw["message"] = {
        "id": msg_id,
        "type": "message",
        "role": "assistant",
        "model": "claude-synthetic-1",
        "content": content,
        "stop_reason": "end_turn",
        "usage": usage,
    }
    raw["requestId"] = "req_" + hexs(rng, 24)
    payload["message_id"] = msg_id
    payload["model"] = "claude-synthetic-1"
    payload["stop_reason"] = "end_turn"
    payload["token_usage"] = usage
    payload["parent_uuid"] = raw["parentUuid"]


def session_lines(plan: dict, rng: random.Random):
    """Yield the JSONL lines of one session, sized to the plan's byte target."""
    n = plan["events"]
    kinds = [TURN[i % len(TURN)] for i in range(n)]
    probe = random.Random(rng.getrandbits(32))
    base = {k: len(json.dumps(make_event(probe, plan, 1, k, plan["start"], 0), separators=(",", ":"))) + 1 for k in set(kinds)}
    total_base = sum(base[k] for k in kinds)
    n_out = kinds.count(TOOL_RESULT)
    out_bytes = max(0, (plan["target_bytes"] - total_base) // (n_out * OUTPUT_COPIES)) if n_out else 0
    at = plan["start"]
    for seq, kind in enumerate(kinds, 1):
        at = at + timedelta(seconds=rng.uniform(1, 30))
        yield json.dumps(make_event(rng, plan, seq, kind, at, out_bytes), separators=(",", ":"))


NODE_OWN_JSONL = ("events.jsonl", "presence.jsonl")


def is_session_file(name: str) -> bool:
    """A fixture session file, as opposed to the JSONL the node itself writes
    into its data dir on boot (`events.jsonl`, `presence.jsonl`)."""
    return name.endswith(".jsonl") and name not in NODE_OWN_JSONL


def gate_passes(peak_rss_bytes: int, gate_gb: float) -> bool:
    return peak_rss_bytes < gate_gb * 1_000_000_000


# ── Linux mode (--docker): the same boot inside a container ───────────────

DOCKER_NET = "boot-memory-net"


def parse_docker_stats_mem(text: str) -> int | None:
    """`docker stats --format {{.MemUsage}}` prints `1.234GiB / 2GiB`; the
    used part in bytes, or None when the container is gone."""
    raise NotImplementedError


def parse_memory_stat(text: str) -> dict[str, int]:
    """cgroup v2 `memory.stat` lines (`anon 123`, `file 456`, ...) → {name: bytes}."""
    raise NotImplementedError


def parse_vmrss(status_text: str) -> int | None:
    """`VmRSS:\t  123456 kB` from /proc/<pid>/status → bytes."""
    raise NotImplementedError


def docker_run_args(image: str, root: Path, name: str, nats_name: str, port: int, memory: str) -> list[str]:
    """The `docker run` argv for the measured container: detached, named,
    on DOCKER_NET, memory-limited, the fixture's data dir at /data, an
    empty dir at /watch, port 3002 published on 127.0.0.1:<port>, every
    watcher root pointed at /watch, the bus at the sidecar nats."""
    raise NotImplementedError


def consumer_summary(body: dict | None) -> dict[str, int]:
    """How many consumers sit in each `state` in a health body (B-05):
    e.g. {"pending_start": 5} while replaying, {"running": 5} after."""
    raise NotImplementedError


# ── Side effects: fixture on disk, the boot, the samples ──────────────────


def write_fixture(root: Path, args: argparse.Namespace) -> dict:
    """Build `root/data/*.jsonl` + `root/watch/` once; reuse when the manifest
    matches. Never deletes: a mismatching manifest is an error, pick a new dir."""
    root.mkdir(parents=True, exist_ok=True)
    data = root / "data"
    watch = root / "watch"
    data.mkdir(exist_ok=True)
    watch.mkdir(exist_ok=True)
    manifest_path = root / "manifest.json"
    params = {
        "version": 1,
        "sessions": args.sessions,
        "large": args.large,
        "large_events": args.large_events,
        "total_bytes": int(args.total_gb * 1_000_000_000),
        "seed": args.seed,
    }
    if manifest_path.exists():
        have = json.loads(manifest_path.read_text())
        n_files = sum(1 for f in data.iterdir() if is_session_file(f.name))
        if {k: have.get(k) for k in params} == params and n_files == have.get("files"):
            print(f"fixture: reusing {data} ({have['files']} files, {have['jsonl_bytes'] / 1e9:.2f} GB JSONL)")
            have["root"] = str(root)
            return have
        raise SystemExit(f"fixture: {manifest_path} does not match these parameters; use a fresh --fixture dir")
    if list(data.glob("*.jsonl")):
        raise SystemExit(f"fixture: {data} already holds JSONL without a manifest; use a fresh --fixture dir")
    rng = random.Random(args.seed)
    now = datetime.now(timezone.utc)
    plans = plan_sessions(args.sessions, args.large, params["total_bytes"], args.large_events, rng, now)
    t0 = time.monotonic()
    written = 0
    for i, plan in enumerate(plans, 1):
        srng = random.Random(rng.getrandbits(64))
        with open(data / f"{plan['id']}.jsonl", "w", encoding="utf-8") as fh:
            for line in session_lines(plan, srng):
                fh.write(line)
                fh.write("\n")
                written += len(line) + 1
        if i % 100 == 0 or i == len(plans):
            print(f"fixture: {i}/{len(plans)} sessions, {written / 1e9:.2f} GB, {time.monotonic() - t0:.0f} s", flush=True)
    manifest = dict(params)
    manifest.update(
        {
            "files": len(plans),
            "jsonl_bytes": written,
            "events": sum(p["events"] for p in plans),
            "subagents": sum(1 for p in plans if p["parent_id"]),
            "built_at": now.isoformat(),
            "build_secs": round(time.monotonic() - t0, 1),
        }
    )
    manifest_path.write_text(json.dumps(manifest, indent=2))
    manifest["root"] = str(root)
    return manifest


def free_port(start: int) -> int:
    """First port at or above `start` that nothing listens on (lsof)."""
    port = start
    while port < start + 200:
        if port not in (3002, 4222, 5173, 8222):
            r = subprocess.run(["lsof", "-ti", f":{port}"], capture_output=True, text=True)
            if r.returncode != 0 and not r.stdout.strip():
                return port
        port += 1
    raise SystemExit(f"no free port in {start}..{start + 200}")


def boot_and_sample(
    binary: str, fixture: dict, port: int, nats_port: int, settle_secs: int, timeout_secs: int, log_path: Path
) -> dict:
    root = Path(fixture["root"])
    data = root / "data"
    watch = root / "watch"
    db_existed = (data / "open-story.db").exists()
    env = os.environ.copy()
    env.update(
        {
            "OPEN_STORY_CLAUDE_WATCH_DIR": str(watch),
            "OPEN_STORY_CODEX_WATCH_DIR": str(watch),
            "OPEN_STORY_GROK_WATCH_DIR": str(watch),
            "OPEN_STORY_PI_WATCH_DIR": "",
            "OPEN_STORY_HERMES_WATCH_DIR": "",
            "OPEN_STORY_LOG_FORMAT": "text",
        }
    )
    cmd = [
        binary, "serve", "--manage-nats",
        "--host", "127.0.0.1", "--port", str(port),
        "--data-dir", str(data), "--watch-dir", str(watch),
        "--nats-url", f"nats://127.0.0.1:{nats_port}",
    ]
    print(f"boot: {' '.join(cmd)}", flush=True)
    log = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(cmd, stdout=log, stderr=subprocess.STDOUT, env=env, start_new_session=True)
    url = f"http://127.0.0.1:{port}/api/health"
    samples: list[dict] = []
    t0 = time.monotonic()
    serving_at: float | None = None
    exit_code: int | None = None
    timed_out = False
    last_print = ""
    try:
        while True:
            tick = time.monotonic()
            t = int(round(tick - t0))
            rss = rss_bytes_from_ps(subprocess.run(["ps", "-o", "rss=", "-p", str(proc.pid)], capture_output=True, text=True).stdout)
            if rss is None or proc.poll() is not None:
                exit_code = proc.wait()
                break
            status, body = fetch_health(url)
            phase = classify(status, body)
            if serving_at is not None and tick - serving_at >= settle_secs:
                phase = "settle"
            samples.append({"t": t, "rss_bytes": rss, "phase": phase, "replay": (body or {}).get("boot", {}).get("replay")})
            line = f"  t={t:5d}s  rss={rss / 1e6:8.1f} MB  {phase}"
            if line[-24:] != last_print[-24:] or t % 15 == 0:
                print(line, flush=True)
            last_print = line
            if phase == "settle":
                break
            if phase == "serving" and serving_at is None:
                serving_at = tick
            if tick - t0 > timeout_secs:
                timed_out = True
                break
            time.sleep(max(0.0, 1.0 - (time.monotonic() - tick)))
    finally:
        stop_process_group(proc)
        log.close()
    peaks = phase_peaks(samples)
    return {
        "db_existed_before": db_existed,
        "phases": peaks,
        "peak_rss_bytes": max((s["rss_bytes"] for s in samples), default=0),
        "serving_after_secs": None if serving_at is None else int(round(serving_at - t0)),
        "exit_code": exit_code,
        "timed_out": timed_out,
        "samples": len(samples),
        "log": str(log_path),
    }


def fetch_health(url: str) -> tuple[int | None, dict | None]:
    try:
        with urllib.request.urlopen(url, timeout=0.8) as r:
            return r.status, json.loads(r.read().decode("utf-8", "replace") or "{}")
    except urllib.error.HTTPError as e:
        try:
            body = json.loads(e.read().decode("utf-8", "replace") or "{}")
        except ValueError:
            body = {}
        return e.code, body
    except urllib.error.URLError as e:
        if isinstance(e.reason, TimeoutError) or "timed out" in str(e.reason):
            return None, {"timeout": True}
        return None, None
    except (TimeoutError, OSError):
        return None, {"timeout": True}


def stop_process_group(proc: subprocess.Popen) -> None:
    """SIGTERM the server and its managed nats child (same process group);
    SIGKILL what is left after 15 s."""
    if proc.poll() is not None:
        return
    try:
        os.killpg(os.getpgid(proc.pid), signal.SIGTERM)
    except ProcessLookupError:
        return
    deadline = time.monotonic() + 15
    while proc.poll() is None and time.monotonic() < deadline:
        time.sleep(0.2)
    if proc.poll() is None:
        try:
            os.killpg(os.getpgid(proc.pid), signal.SIGKILL)
        except ProcessLookupError:
            pass
        proc.wait()


def print_report(result: dict) -> None:
    fx = result["fixture"]
    print()
    print(f"fixture   {fx['files']} sessions, {fx['events']} events, {fx['jsonl_bytes'] / 1e9:.2f} GB JSONL, {fx['subagents']} subagents, seed {fx['seed']}")
    print(f"binary    {result['binary']}")
    for b in result["boots"]:
        role = "measured" if b["index"] == result["measured_boot"] else "ingest"
        ended = "timed out" if b["timed_out"] else (f"exited {b['exit_code']}" if b["exit_code"] is not None else "stopped by harness")
        serving = f"serving after {b['serving_after_secs']} s" if b["serving_after_secs"] is not None else "never served"
        print()
        print(f"boot {b['index']} ({role})  db existed before: {'yes' if b['db_existed_before'] else 'no'}  {serving}  {ended}")
        print(f"  {'phase':<10} {'peak RSS':>12}  {'samples':>7}  window")
        for name, p in b["phases"].items():
            print(f"  {name:<10} {p['peak_rss_bytes'] / 1e6:>9.1f} MB  {p['samples']:>7}  {p['first_t']}-{p['last_t']} s")
        print(f"  {'peak':<10} {b['peak_rss_bytes'] / 1e6:>9.1f} MB")
    print()
    verdict = "PASS" if result["pass"] else "FAIL"
    print(f"gate      peak {result['peak_rss_bytes'] / 1e9:.3f} GB {'<' if result['pass'] else '>='} {result['gate_gb']} GB  → {verdict}")


# ── Specs ──────────────────────────────────────────────────────────────────


def test_when_health_is_refused_it_is_no_listen():
    assert classify(None, None) == "no-listen"


def test_when_health_is_503_it_reads_the_boot_phase():
    assert classify(503, {"boot": {"phase": "replaying"}}) == "replaying"
    assert classify(503, {"boot": {"phase": "starting"}}) == "starting"
    assert classify(503, {}) == "replaying"


def test_when_health_is_200_it_is_serving():
    assert classify(200, {"status": "ok"}) == "serving"


def test_when_health_is_another_status_it_names_it():
    assert classify(502, None) == "other-502"


def test_when_health_times_out_it_is_no_answer_not_no_listen():
    assert classify(None, {"timeout": True}) == "no-answer"


def test_when_ps_prints_kilobytes_it_returns_bytes():
    assert rss_bytes_from_ps(" 123456\n") == 123456 * 1024
    assert rss_bytes_from_ps("") is None


def test_when_samples_span_phases_it_reports_a_peak_per_phase():
    samples = [
        {"t": 0, "rss_bytes": 100, "phase": "no-listen"},
        {"t": 1, "rss_bytes": 300, "phase": "no-listen"},
        {"t": 2, "rss_bytes": 250, "phase": "replaying"},
        {"t": 3, "rss_bytes": 900, "phase": "replaying"},
        {"t": 4, "rss_bytes": 400, "phase": "serving"},
        {"t": 34, "rss_bytes": 350, "phase": "settle"},
    ]
    peaks = phase_peaks(samples)
    assert peaks["no-listen"] == {"peak_rss_bytes": 300, "samples": 2, "first_t": 0, "last_t": 1}
    assert peaks["replaying"]["peak_rss_bytes"] == 900
    assert peaks["serving"]["peak_rss_bytes"] == 400
    assert peaks["settle"]["peak_rss_bytes"] == 350
    assert list(peaks) == ["no-listen", "replaying", "serving", "settle"]


def test_when_sessions_are_planned_the_budget_and_shape_hold():
    now = datetime(2026, 9, 25, tzinfo=timezone.utc)
    plans = plan_sessions(1259, 40, 3_000_000_000, 8000, random.Random(7), now)
    assert len(plans) == 1259
    large = [p for p in plans if p["large"]]
    assert len(large) == 40
    assert all(p["events"] == 8000 for p in large)
    total = sum(p["target_bytes"] for p in plans)
    assert abs(total - 3_000_000_000) < 3_000_000, total
    assert len({p["id"] for p in plans}) == 1259, "session ids are unique"
    subagents = [p for p in plans if p["parent_id"]]
    assert 100 <= len(subagents) <= 150, len(subagents)
    ids = {p["id"] for p in plans}
    assert all(p["parent_id"] in ids for p in subagents), "a parent is a planned session"
    recent = [p for p in plans if now - p["start"] < timedelta(days=5)]
    assert len(recent) == 20
    assert all(now - p["start"] <= timedelta(days=180) for p in plans)


def test_when_an_event_is_made_it_is_a_translated_cloudevent():
    plan = {"id": "s-1", "parent_id": None, "cwd": "/home/x/p", "start": datetime(2026, 1, 1, tzinfo=timezone.utc)}
    at = datetime(2026, 1, 1, 0, 0, 1, 250_000, tzinfo=timezone.utc)
    e = make_event(random.Random(1), plan, 7, TOOL_RESULT, at, 1000)
    assert e["specversion"] == "1.0"
    assert e["type"] == EVENT_TYPE
    assert e["subtype"] == TOOL_RESULT
    assert e["agent"] == AGENT
    assert e["time"] == "2026-01-01T00:00:01.250Z"
    uuid.UUID(e["id"])
    assert e["source"] == "arc://transcript/s-1"
    assert e["data"]["seq"] == 7
    assert e["data"]["session_id"] == "s-1"
    assert e["data"]["raw"]["cwd"] == "/home/x/p"
    assert e["data"]["agent_payload"]["cwd"] == "/home/x/p"
    raw_out = e["data"]["raw"]["message"]["content"][0]["content"]
    assert len(raw_out) == 1000
    assert e["data"]["raw"]["toolUseResult"]["stdout"] == raw_out
    assert e["data"]["agent_payload"]["text"] == raw_out


def test_when_a_session_is_a_subagent_its_events_carry_the_parent_id():
    plan = {"id": "child", "parent_id": "parent", "cwd": "/p", "start": datetime(2026, 1, 1, tzinfo=timezone.utc)}
    e = make_event(random.Random(1), plan, 1, "message.user.prompt", plan["start"], 0)
    assert e["data"]["session_id"] == "parent"
    assert e["source"] == "arc://transcript/child"


def test_when_session_lines_are_written_they_meet_the_byte_target():
    plan = {
        "id": "s-2",
        "parent_id": None,
        "cwd": "/p",
        "start": datetime(2026, 1, 1, tzinfo=timezone.utc),
        "events": 90,
        "target_bytes": 400_000,
        "large": False,
    }
    lines = list(session_lines(plan, random.Random(3)))
    assert len(lines) == 90
    total = sum(len(l) + 1 for l in lines)
    assert 0.9 * 400_000 <= total <= 1.1 * 400_000, total
    seqs = [json.loads(l)["data"]["seq"] for l in lines]
    assert seqs == list(range(1, 91))
    times = [json.loads(l)["time"] for l in lines]
    assert times == sorted(times)
    kinds = [json.loads(l)["subtype"] for l in lines]
    assert kinds[: len(TURN)] == list(TURN)


def test_when_the_node_wrote_its_own_jsonl_they_are_not_session_files():
    assert is_session_file("0c1d5e6a-1b2c-4d3e-8f90-0a1b2c3d4e5f.jsonl")
    assert not is_session_file("events.jsonl")
    assert not is_session_file("presence.jsonl")
    assert not is_session_file("manifest.json")


def test_when_docker_stats_prints_mem_usage_it_returns_used_bytes():
    assert parse_docker_stats_mem("1.5GiB / 2GiB\n") == int(1.5 * 1024**3)
    assert parse_docker_stats_mem("512MiB / 2GiB") == 512 * 1024**2
    assert parse_docker_stats_mem("900kB / 2GiB") == 900_000
    assert parse_docker_stats_mem("") is None


def test_when_memory_stat_is_read_it_maps_names_to_bytes():
    text = "anon 4177920000\nfile 1048576000\nkernel 1234\nfile_mapped 5\n"
    stat = parse_memory_stat(text)
    assert stat["anon"] == 4177920000
    assert stat["file"] == 1048576000
    assert stat["file_mapped"] == 5


def test_when_proc_status_is_read_it_returns_vmrss_bytes():
    assert parse_vmrss("Name:\topen-story\nVmRSS:\t  123456 kB\nThreads:\t9\n") == 123456 * 1024
    assert parse_vmrss("Name:\tx\n") is None


def test_when_the_container_is_planned_its_argv_is_bounded_and_isolated():
    args = docker_run_args("open-story:boot-memory", Path("/fx"), "bm-os-3300", "bm-nats-3300", 3300, "2g")
    joined = " ".join(args)
    assert args[:3] == ["docker", "run", "-d"]
    assert "--name bm-os-3300" in joined
    assert f"--network {DOCKER_NET}" in joined
    assert "--memory 2g" in joined and "--memory-swap 2g" in joined
    assert "-v /fx/data:/data" in joined
    assert "-v /fx/watch:/watch" in joined
    assert "-p 127.0.0.1:3300:3002" in joined
    for var in ("OPEN_STORY_CLAUDE_WATCH_DIR", "OPEN_STORY_CODEX_WATCH_DIR", "OPEN_STORY_GROK_WATCH_DIR"):
        assert f"-e {var}=/watch" in joined
    assert "-e OPEN_STORY_PI_WATCH_DIR=" in joined and "-e OPEN_STORY_HERMES_WATCH_DIR=" in joined
    assert args[args.index("open-story:boot-memory") + 1 :] == [
        "serve", "--host", "0.0.0.0", "--port", "3002", "--data-dir", "/data",
        "--watch-dir", "/watch", "--nats-url", "nats://bm-nats-3300:4222",
    ]


def test_when_consumers_are_summarised_states_are_counted():
    body = {"consumers": {
        "persist": {"alive": False, "state": "pending_start"},
        "patterns": {"alive": False, "state": "pending_start"},
        "broadcast": {"alive": True, "state": "running"},
    }}
    assert consumer_summary(body) == {"pending_start": 2, "running": 1}
    assert consumer_summary({"consumers": {"old": {"alive": True}}}) == {"alive": 1}
    assert consumer_summary(None) == {}


def test_when_the_gate_is_evaluated_it_compares_bytes_to_gigabytes():
    assert gate_passes(1_999_999_999, 2.0)
    assert not gate_passes(2_000_000_001, 2.0)


def run_tests() -> int:
    tests = [(n, f) for n, f in sorted(globals().items()) if n.startswith("test_when_") and callable(f)]
    failed = 0
    for name, fn in tests:
        try:
            fn()
            print(f"ok   {name}")
        except Exception as e:  # noqa: BLE001 — a spec runner reports, it does not hide
            failed += 1
            print(f"FAIL {name}: {type(e).__name__}: {e}")
    print(f"{len(tests) - failed}/{len(tests)} specs passed")
    return 1 if failed else 0


# ── CLI ────────────────────────────────────────────────────────────────────


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--test", action="store_true", help="run the pure-function specs and exit")
    ap.add_argument("--fixture", help="directory for the synthetic store (created; keep out of git)")
    ap.add_argument("--sessions", type=int, default=1259)
    ap.add_argument("--large", type=int, default=40, help="sessions in the large tail")
    ap.add_argument("--large-events", type=int, default=8000, help="events per large session")
    ap.add_argument("--total-gb", type=float, default=3.0, help="JSONL bytes across all sessions")
    ap.add_argument("--seed", type=int, default=2026)
    ap.add_argument("--binary", default=os.environ.get("OPEN_STORY_BIN", "open-story"))
    ap.add_argument("--port", type=int, default=3300, help="first HTTP port to try (must be > 3200)")
    ap.add_argument("--nats-port", type=int, default=4300, help="first NATS port to try (must be > 4300)")
    ap.add_argument("--settle-secs", type=int, default=30)
    ap.add_argument("--timeout-secs", type=int, default=3600, help="give up on a boot after this long")
    ap.add_argument("--boots", type=int, default=2, help="1 = ingest only; 2 = ingest then measure")
    ap.add_argument("--gate-gb", type=float, default=2.0)
    ap.add_argument("--json", dest="json_path", help="write the result as JSON here")
    args = ap.parse_args()

    if args.test:
        return run_tests()
    if not args.fixture:
        ap.error("--fixture DIR is required (or --test)")
    if args.port <= 3200 or args.nats_port <= 4300 - 1:
        ap.error("--port must be above 3200 and --nats-port at or above 4300 (the live node owns 3002/4222)")

    fixture = write_fixture(Path(args.fixture), args)
    port = free_port(args.port)
    nats_port = free_port(args.nats_port)
    print(f"ports: http {port}, nats {nats_port}")
    boots = []
    for i in range(1, args.boots + 1):
        log_path = Path(args.fixture) / f"boot-{i}.log"
        b = boot_and_sample(args.binary, fixture, port, nats_port, args.settle_secs, args.timeout_secs, log_path)
        b["index"] = i
        boots.append(b)
        if b["serving_after_secs"] is None:
            print(f"boot {i}: never reached serving (see {log_path}); stopping", flush=True)
            break
    measured = boots[-1]
    result = {
        "measured_at": datetime.now(timezone.utc).isoformat(),
        "binary": args.binary,
        "fixture": fixture,
        "boots": boots,
        "measured_boot": measured["index"],
        "peak_rss_bytes": measured["peak_rss_bytes"],
        "gate_gb": args.gate_gb,
        "pass": measured["serving_after_secs"] is not None and gate_passes(measured["peak_rss_bytes"], args.gate_gb),
    }
    print_report(result)
    if args.json_path:
        Path(args.json_path).write_text(json.dumps(result, indent=2, default=str))
        print(f"json      {args.json_path}")
    return 0 if result["pass"] else 1


if __name__ == "__main__":
    sys.exit(main())
