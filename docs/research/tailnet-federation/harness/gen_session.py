#!/usr/bin/env python3
"""Generate a minimal raw Claude Code transcript JSONL for the tailnet harness.

Mirrors the schema in rs/tests/helpers/synth.rs::transcript_line so the
open-story watcher translates it into CloudEvents exactly like a real session.
One file = one session, keyed by the `sessionId` field.

Usage: gen_session.py <session_id> > <session_id>.jsonl
"""
import json
import sys
import uuid

CWD = "/home/dev/project"
VER = "2.3.0"
TS = "2025-01-10T14:00:{:02d}.000Z"


def line(obj):
    return json.dumps(obj, separators=(",", ":"))


def session(session_id):
    rows = []
    seq = 0

    def ts():
        nonlocal seq
        t = TS.format(seq)
        seq += 1
        return t

    base = lambda u, parent, typ: {
        "type": typ, "uuid": u, "sessionId": session_id,
        **({"parentUuid": parent} if parent else {}),
        "cwd": CWD, "version": VER, "gitBranch": "main", "timestamp": ts(),
    }

    p = str(uuid.uuid4())
    r = base(p, None, "user")
    r["message"] = {"role": "user", "content": "Write integration tests for the payment flow"}
    rows.append(r)

    tu = str(uuid.uuid4()); tool_id = "toolu_" + tu[:12]
    r = base(tu, p, "assistant")
    r["message"] = {"role": "assistant", "model": "claude-sonnet-4-6", "id": "msg_" + tu,
                    "content": [{"type": "tool_use", "id": tool_id, "name": "Bash",
                                 "input": {"command": "cargo test"}}],
                    "usage": {"input_tokens": 1000, "output_tokens": 200},
                    "stop_reason": "tool_use"}
    rows.append(r)

    tr = str(uuid.uuid4())
    r = base(tr, tu, "user")
    r["message"] = {"role": "user", "content": [
        {"type": "tool_result", "tool_use_id": tool_id, "content": "42 tests passed"}]}
    rows.append(r)

    at = str(uuid.uuid4())
    r = base(at, tr, "assistant")
    r["message"] = {"role": "assistant", "model": "claude-sonnet-4-6", "id": "msg_" + at,
                    "content": [{"type": "text",
                                 "text": "Here is the implementation. The tests pass."}],
                    "usage": {"input_tokens": 2000, "output_tokens": 500},
                    "stop_reason": "end_turn"}
    rows.append(r)

    st = str(uuid.uuid4())
    r = base(st, at, "system")
    r["subtype"] = "turn_duration"; r["durationMs"] = 4200
    rows.append(r)

    return "\n".join(line(r) for r in rows) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit("usage: gen_session.py <session_id>")
    sys.stdout.write(session(sys.argv[1]))
