#!/usr/bin/env python3
"""Dump per-subject message counts for a JetStream stream via raw NATS protocol.

No client library needed — speaks the NATS text protocol over a TCP socket and
issues a JetStream `$JS.API.STREAM.INFO.<stream>` request with a subjects_filter,
then aggregates the returned subject map by the project segment of the OpenStory
subject convention: events.{project}.{session}.{leg}.

Usage:
    python3 scripts/nats_stream_subjects.py [--url nats://127.0.0.1:4222] \
        [--stream events] [--filter 'events.>'] [--by-project] [--raw]

This is a debugging tool for the distributed (leaf-node) bus: it answers
"whose sessions are actually on the hub?" by showing the project ids present in
the synced stream, which the HTTP monitor (/jsz) does not expose.
"""
import argparse
import json
import socket
from collections import Counter
from urllib.parse import urlparse


def js_stream_info(url: str, stream: str, subjects_filter: str) -> dict:
    u = urlparse(url)
    host, port = u.hostname or "127.0.0.1", u.port or 4222
    sock = socket.create_connection((host, port), timeout=5)
    f = sock.makefile("rwb")

    def readline():
        return f.readline().decode("utf-8", "replace")

    # Server greets with INFO ...
    info = readline()
    if not info.startswith("INFO"):
        raise RuntimeError(f"unexpected greeting: {info!r}")

    connect = {"verbose": False, "pedantic": False, "lang": "py-raw", "version": "0"}
    if u.username:
        connect["auth_token" if not u.password else "user"] = u.username
    if u.password:
        connect["pass"] = u.password
    f.write(f"CONNECT {json.dumps(connect)}\r\n".encode())
    f.write(b"PING\r\n")
    f.flush()
    # expect PONG (skip any +OK)
    line = readline()
    while line.startswith("+OK"):
        line = readline()
    if not line.startswith("PONG"):
        raise RuntimeError(f"no PONG after connect: {line!r}")

    inbox = "_INBOX.streamsubs"
    f.write(f"SUB {inbox} 1\r\n".encode())
    body = json.dumps({"subjects_filter": subjects_filter}).encode()
    subj = f"$JS.API.STREAM.INFO.{stream}"
    f.write(f"PUB {subj} {inbox} {len(body)}\r\n".encode())
    f.write(body + b"\r\n")
    f.flush()

    # read until we get the MSG reply
    while True:
        line = readline()
        if not line:
            raise RuntimeError("connection closed before reply")
        if line.startswith("PING"):
            f.write(b"PONG\r\n")
            f.flush()
            continue
        if line.startswith("MSG"):
            parts = line.split()
            nbytes = int(parts[-1])
            payload = f.read(nbytes)
            f.read(2)  # trailing \r\n
            sock.close()
            return json.loads(payload.decode("utf-8", "replace"))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--url", default="nats://127.0.0.1:4222")
    ap.add_argument("--stream", default="events")
    ap.add_argument("--filter", default="events.>")
    ap.add_argument("--by-project", action="store_true", help="aggregate by project segment")
    ap.add_argument("--raw", action="store_true", help="print full subject map")
    args = ap.parse_args()

    resp = js_stream_info(args.url, args.stream, args.filter)
    state = resp.get("state", {})
    subjects = state.get("subjects") or {}
    print(f"stream={args.stream} messages={state.get('messages')} subjects={len(subjects)}")
    if not subjects:
        print("  (no subjects — stream empty or filter matched nothing)")
        return
    if args.raw:
        for k, v in sorted(subjects.items()):
            print(f"  {v:>6}  {k}")
        return
    # default + --by-project: roll up by the {project} segment
    proj = Counter()
    for k, v in subjects.items():
        parts = k.split(".")
        proj[parts[1] if len(parts) > 1 else "?"] += v
    print("messages by PROJECT id:")
    for p, c in proj.most_common():
        print(f"  {c:>6}  {p}")


if __name__ == "__main__":
    main()
