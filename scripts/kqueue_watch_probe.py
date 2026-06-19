#!/usr/bin/env python3
"""kqueue_watch_probe.py — prove kqueue sees file appends FSEvents misses.

Why this exists
---------------
On macOS, OpenStory's watcher uses `notify`'s FSEvents backend. We measured
(via /api/watchers) that FSEvents delivered only 2 events for a Codex rollout
file that was actively appended for 47 minutes — it silently dropped Codex's
held-open, buffered-append writes, while catching Claude Code's. Backfill +
a manual `touch` recovered everything, proving the reader is correct: the
*only* thing missing was the kernel change-notification.

This is the "shift prototyping left" step: before rebuilding the Rust watcher
around a kqueue hybrid, validate that kqueue (`EVFILT_VNODE`) actually fires
on the Codex write pattern — down to the microsecond, no polling.

Modes
-----
  --watch PATH [--seconds N]
      Register a kqueue VNODE watch on PATH and print every event with a
      monotonic + wall-clock timestamp until N seconds elapse.

  --simulate PATH [--lines N --interval S --fsync]
      Mimic Codex: open PATH once, append N JSON lines S seconds apart,
      flushing each, holding the fd open the whole time. (--fsync forces
      a kernel sync per line; Codex flushes, may or may not fsync.)

  --demo [--lines N --interval S]
      The money shot: spawn the Codex-style writer in a thread, kqueue-watch
      the same file concurrently, and report how many vnode events fired vs
      lines written. ~1 event per write = kqueue catches what FSEvents drops.

  --test
      Pure-logic self-tests (no filesystem / no kqueue), for CI.

Usage
-----
  python3 scripts/kqueue_watch_probe.py --demo
  python3 scripts/kqueue_watch_probe.py --watch ~/.codex/sessions/.../rollout-X.jsonl --seconds 60
  python3 scripts/kqueue_watch_probe.py --test
"""
from __future__ import annotations

import argparse
import json
import os
import select
import sys
import tempfile
import threading
import time


# ---------------------------------------------------------------------------
# Pure helpers (testable without a filesystem or kqueue)
# ---------------------------------------------------------------------------

# VNODE note flags we care about, by name — decoded for human-readable output.
VNODE_NOTES = {
    "WRITE": getattr(select, "KQ_NOTE_WRITE", 0),
    "EXTEND": getattr(select, "KQ_NOTE_EXTEND", 0),
    "ATTRIB": getattr(select, "KQ_NOTE_ATTRIB", 0),
    "DELETE": getattr(select, "KQ_NOTE_DELETE", 0),
    "RENAME": getattr(select, "KQ_NOTE_RENAME", 0),
    "LINK": getattr(select, "KQ_NOTE_LINK", 0),
    "REVOKE": getattr(select, "KQ_NOTE_REVOKE", 0),
}


def watch_fflags() -> int:
    """The combined fflags mask a content-watcher should register."""
    return (
        VNODE_NOTES["WRITE"]
        | VNODE_NOTES["EXTEND"]
        | VNODE_NOTES["DELETE"]
        | VNODE_NOTES["RENAME"]
        | VNODE_NOTES["ATTRIB"]
    )


def decode_fflags(fflags: int) -> list[str]:
    """Human-readable list of which VNODE notes are set in fflags."""
    return [name for name, bit in VNODE_NOTES.items() if bit and (fflags & bit)]


def codex_line(i: int) -> str:
    """One JSONL line shaped like a Codex rollout event."""
    return json.dumps(
        {
            "timestamp": f"2026-01-01T00:00:{i % 60:02d}.000Z",
            "type": "response_item",
            "payload": {"type": "function_call", "name": "shell", "seq": i},
        }
    )


# ---------------------------------------------------------------------------
# Edges (real kqueue / filesystem)
# ---------------------------------------------------------------------------

def simulate_codex(path: str, lines: int, interval: float, fsync: bool) -> None:
    """Append `lines` JSONL records to `path`, holding the fd open throughout."""
    # Held-open append fd = the Codex pattern that defeats FSEvents.
    with open(path, "a", buffering=1) as f:  # line-buffered
        for i in range(lines):
            f.write(codex_line(i) + "\n")
            f.flush()
            if fsync:
                os.fsync(f.fileno())
            time.sleep(interval)


def watch(path: str, seconds: float, *, quiet: bool = False) -> int:
    """kqueue-watch `path` for `seconds`; print each event. Returns event count."""
    fd = os.open(path, os.O_RDONLY)
    kq = select.kqueue()
    ev = select.kevent(
        fd,
        filter=select.KQ_FILTER_VNODE,
        flags=select.KQ_EV_ADD | select.KQ_EV_CLEAR,
        fflags=watch_fflags(),
    )
    kq.control([ev], 0, 0)  # register, no wait

    start = time.monotonic()
    count = 0
    try:
        while True:
            remaining = seconds - (time.monotonic() - start)
            if remaining <= 0:
                break
            # Block up to `remaining`s for events; return as soon as any fire.
            triggered = kq.control(None, 8, remaining)
            now = time.monotonic() - start
            for t in triggered:
                count += 1
                if not quiet:
                    notes = ",".join(decode_fflags(t.fflags)) or f"raw={t.fflags}"
                    print(f"  +{now:7.4f}s  kqueue VNODE event: [{notes}]")
    finally:
        kq.close()
        os.close(fd)
    return count


def demo(lines: int, interval: float) -> int:
    """Run the Codex-style writer + kqueue watcher concurrently; report parity."""
    tmp = tempfile.NamedTemporaryFile(suffix=".jsonl", delete=False)
    path = tmp.name
    tmp.write(b"")  # ensure the file exists to open for watching
    tmp.close()

    print(f"file: {path}")
    print(f"writing {lines} lines, {interval}s apart, fd held open (Codex pattern)")
    print(f"watching via kqueue EVFILT_VNODE [{','.join(n for n, b in VNODE_NOTES.items() if b)}]\n")

    writer = threading.Thread(
        target=simulate_codex, args=(path, lines, interval, False), daemon=True
    )
    # Watch a hair longer than the write run so the last event lands.
    watch_seconds = lines * interval + 1.0
    writer.start()
    events = watch(path, watch_seconds)

    print(f"\nlines written: {lines}")
    print(f"kqueue events fired: {events}")
    verdict = "PASS — kqueue sees the appends FSEvents drops" if events >= lines else (
        "PARTIAL — some appends coalesced (still event-driven, no polling)"
        if events > 0 else "FAIL — kqueue saw nothing"
    )
    print(f"verdict: {verdict}")
    os.unlink(path)
    return 0 if events > 0 else 1


# ---------------------------------------------------------------------------
# Self-tests (pure, no kqueue)
# ---------------------------------------------------------------------------

def _run_tests() -> int:
    failures = 0

    def check(name, cond):
        nonlocal failures
        print(f"  {'ok  ' if cond else 'FAIL'} {name}")
        if not cond:
            failures += 1

    check("watch mask includes WRITE", watch_fflags() & VNODE_NOTES["WRITE"])
    check("watch mask includes EXTEND", watch_fflags() & VNODE_NOTES["EXTEND"])
    check("watch mask includes DELETE", watch_fflags() & VNODE_NOTES["DELETE"])
    w = VNODE_NOTES["WRITE"]
    check("decode_fflags finds WRITE", "WRITE" in decode_fflags(w))
    check("decode_fflags ignores unset", "DELETE" not in decode_fflags(w))
    check("decode_fflags combo", set(decode_fflags(w | VNODE_NOTES["EXTEND"])) >= {"WRITE", "EXTEND"})
    check("codex_line is valid json", json.loads(codex_line(3))["payload"]["seq"] == 3)
    check("codex_line distinct per i", codex_line(1) != codex_line(2))

    print(f"\n{'PASS' if failures == 0 else 'FAIL'}: {failures} failures")
    return 1 if failures else 0


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("--watch", metavar="PATH", help="kqueue-watch a file and print events")
    p.add_argument("--simulate", metavar="PATH", help="append Codex-style lines to a file")
    p.add_argument("--demo", action="store_true", help="run writer + watcher together and report parity")
    p.add_argument("--seconds", type=float, default=60.0, help="watch duration (default 60)")
    p.add_argument("--lines", type=int, default=20, help="lines to write (default 20)")
    p.add_argument("--interval", type=float, default=0.3, help="seconds between writes (default 0.3)")
    p.add_argument("--fsync", action="store_true", help="fsync each line (stronger than flush)")
    p.add_argument("--test", action="store_true", help="run self-tests and exit")
    args = p.parse_args(argv)

    if args.test:
        return _run_tests()
    if sys.platform not in ("darwin",) and not args.simulate:
        print(f"note: kqueue is BSD/macOS only (platform={sys.platform}); --simulate still works")
    if args.demo:
        return demo(args.lines, args.interval)
    if args.simulate:
        simulate_codex(args.simulate, args.lines, args.interval, args.fsync)
        print(f"wrote {args.lines} lines to {args.simulate}")
        return 0
    if args.watch:
        n = watch(os.path.expanduser(args.watch), args.seconds)
        print(f"\ntotal kqueue events: {n}")
        return 0
    p.print_help()
    return 1


if __name__ == "__main__":
    sys.exit(main())
