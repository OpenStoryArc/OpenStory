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
    raise NotImplementedError


def classify(status: int | None, body: dict | None) -> str:
    """Phase of one health sample.

    None status (connection refused) → `no-listen`; 200 → `serving`;
    503 → the body's `boot.phase` (`starting` / `replaying`); anything
    else → `other-<status>`.
    """
    raise NotImplementedError


def rss_bytes_from_ps(text: str) -> int | None:
    """`ps -o rss= -p PID` prints kilobytes; None when the process is gone."""
    raise NotImplementedError


def phase_peaks(samples: list[dict]) -> dict[str, dict]:
    """Peak RSS per phase over 1 Hz samples of `{t, rss_bytes, phase}`."""
    raise NotImplementedError


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
    raise NotImplementedError


def make_event(
    rng: random.Random,
    plan: dict,
    seq: int,
    subtype: str,
    at: datetime,
    output_bytes: int,
) -> dict:
    """One translated CloudEvent (field names only; every string synthetic)."""
    raise NotImplementedError


def session_lines(plan: dict, rng: random.Random):
    """Yield the JSONL lines of one session, sized to the plan's byte target."""
    raise NotImplementedError


def gate_passes(peak_rss_bytes: int, gate_gb: float) -> bool:
    raise NotImplementedError


# ── Side effects: fixture on disk, the boot, the samples ──────────────────


def write_fixture(root: Path, args: argparse.Namespace) -> dict:
    raise NotImplementedError


def free_port(start: int) -> int:
    raise NotImplementedError


def boot_and_sample(
    binary: str, fixture: dict, port: int, nats_port: int, settle_secs: int, timeout_secs: int, log_path: Path
) -> dict:
    raise NotImplementedError


def print_report(result: dict) -> None:
    raise NotImplementedError


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
    raise NotImplementedError


if __name__ == "__main__":
    sys.exit(main())
