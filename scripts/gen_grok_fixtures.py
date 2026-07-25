#!/usr/bin/env python3
"""Generate deterministic Grok Build ACP `updates.jsonl` fixtures for tests.

These are synthetic but wire-faithful: same envelope shape as real
`~/.grok/sessions/.../updates.jsonl` lines from Grok Build's ACP stream.

Usage:
  python3 scripts/gen_grok_fixtures.py
  python3 scripts/gen_grok_fixtures.py --out rs/tests/fixtures/grok
  python3 scripts/gen_grok_fixtures.py --test

Exit 0 on success / green --test.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_OUT = ROOT / "rs" / "tests" / "fixtures" / "grok"

SESSION = "019f6cb5-f7e4-7bc1-bb25-aaaaaaaaaaaa"
HOST_TS = 1_700_000_000  # fixed unix seconds for determinism


def line(
    *,
    method: str,
    session_update: dict,
    event_id: str,
    timestamp: int | None = None,
    extra_params_meta: dict | None = None,
) -> dict:
    params_meta = {"eventId": event_id}
    if extra_params_meta:
        params_meta.update(extra_params_meta)
    return {
        "timestamp": timestamp if timestamp is not None else HOST_TS,
        "method": method,
        "params": {
            "sessionId": SESSION,
            "update": session_update,
            "_meta": params_meta,
        },
    }


def user_chunk(text: str, event_id: str, prompt_index: int = 0) -> dict:
    return line(
        method="session/update",
        session_update={
            "sessionUpdate": "user_message_chunk",
            "content": {"type": "text", "text": text},
            "_meta": {"modelId": "grok-4.5", "promptIndex": prompt_index},
        },
        event_id=event_id,
        extra_params_meta={"agentTimestampMs": HOST_TS * 1000},
    )


def thought_chunk(text: str, event_id: str, prompt_id: str) -> dict:
    return line(
        method="session/update",
        session_update={
            "sessionUpdate": "agent_thought_chunk",
            "content": {"type": "text", "text": text},
        },
        event_id=event_id,
        extra_params_meta={
            "promptId": prompt_id,
            "updateType": "AgentThoughtChunk",
            "totalTokens": 1000,
        },
    )


def message_chunk(text: str, event_id: str, prompt_id: str) -> dict:
    return line(
        method="session/update",
        session_update={
            "sessionUpdate": "agent_message_chunk",
            "content": {"type": "text", "text": text},
        },
        event_id=event_id,
        extra_params_meta={
            "promptId": prompt_id,
            "updateType": "AgentMessageChunk",
            "totalTokens": 1200,
        },
    )


def tool_call(
    *,
    call_id: str,
    name: str,
    raw_input: dict,
    event_id: str,
    prompt_id: str,
    kind: str = "other",
    read_only: bool = True,
) -> dict:
    return line(
        method="session/update",
        session_update={
            "sessionUpdate": "tool_call",
            "toolCallId": call_id,
            "title": name,
            "rawInput": raw_input,
            "_meta": {
                "x.ai/tool": {
                    "version": 1,
                    "name": name,
                    "kind": kind,
                    "namespace": "grok_build",
                    "label": name,
                    "read_only": read_only,
                }
            },
        },
        event_id=event_id,
        extra_params_meta={
            "promptId": prompt_id,
            "updateType": "ToolCall",
            "updateParams": {
                "toolCallId": call_id,
                "title": name,
                "kind": "Other",
                "status": "Pending",
            },
        },
    )


def tool_completed(
    *,
    call_id: str,
    output: str | dict,
    event_id: str,
    is_error: bool = False,
) -> dict:
    if isinstance(output, str):
        raw_output = {"type": "Text", "Content": {"content": output}}
    else:
        raw_output = output
    update = {
        "sessionUpdate": "tool_call_update",
        "toolCallId": call_id,
        "status": "completed",
        "rawOutput": raw_output,
    }
    if is_error:
        update["isError"] = True
    return line(
        method="session/update",
        session_update=update,
        event_id=event_id,
        extra_params_meta={"updateType": "ToolCallUpdate"},
    )


def tool_in_progress(call_id: str, event_id: str) -> dict:
    return line(
        method="session/update",
        session_update={
            "sessionUpdate": "tool_call_update",
            "toolCallId": call_id,
            "status": "in_progress",
            "content": [
                {
                    "type": "content",
                    "content": {"type": "text", "text": "running…"},
                }
            ],
        },
        event_id=event_id,
    )


def turn_completed(
    *,
    prompt_id: str,
    event_id: str,
    stop_reason: str = "end_turn",
    input_tokens: int = 100,
    output_tokens: int = 20,
) -> dict:
    return line(
        method="_x.ai/session/update",
        session_update={
            "sessionUpdate": "turn_completed",
            "prompt_id": prompt_id,
            "stop_reason": stop_reason,
            "usage": {
                "inputTokens": input_tokens,
                "outputTokens": output_tokens,
                "totalTokens": input_tokens + output_tokens,
                "cachedReadTokens": 0,
                "reasoningTokens": 5,
                "modelCalls": 1,
                "apiDurationMs": 500,
                "modelUsage": {
                    "grok-4.5": {
                        "inputTokens": input_tokens,
                        "outputTokens": output_tokens,
                        "totalTokens": input_tokens + output_tokens,
                        "cachedReadTokens": 0,
                        "reasoningTokens": 5,
                        "modelCalls": 1,
                        "apiDurationMs": 500,
                    }
                },
                "numTurns": 1,
            },
        },
        event_id=event_id,
    )


def write_jsonl(path: Path, rows: list[dict]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, separators=(",", ":"), ensure_ascii=False) + "\n")


def scenario_01_text_only() -> list[dict]:
    """User asks a question; Grok answers without tools."""
    pid = "prompt-01"
    return [
        user_chunk("What is a TUI?", f"{SESSION}-01", prompt_index=0),
        thought_chunk("User wants a short definition of TUI.", f"{SESSION}-02", pid),
        message_chunk(
            "A TUI is a text user interface that runs in the terminal.",
            f"{SESSION}-03",
            pid,
        ),
        turn_completed(prompt_id=pid, event_id=f"{SESSION}-04"),
    ]


def scenario_02_single_tool() -> list[dict]:
    """User asks; Grok lists a directory then answers."""
    pid = "prompt-02"
    call = "call-list-01"
    return [
        user_chunk("List the project root.", f"{SESSION}-10", prompt_index=1),
        thought_chunk("I should list the directory.", f"{SESSION}-11", pid),
        message_chunk("I'll list the root for you.", f"{SESSION}-12", pid),
        tool_call(
            call_id=call,
            name="list_dir",
            raw_input={"target_directory": "/workspace/demo"},
            event_id=f"{SESSION}-13",
            prompt_id=pid,
            kind="list",
        ),
        tool_in_progress(call, f"{SESSION}-14"),
        tool_completed(
            call_id=call,
            output="- /workspace/demo/\n  - README.md\n  - src/\n",
            event_id=f"{SESSION}-15",
        ),
        message_chunk(
            "The project root has README.md and a src/ directory.",
            f"{SESSION}-16",
            pid,
        ),
        turn_completed(
            prompt_id=pid, event_id=f"{SESSION}-17", input_tokens=200, output_tokens=40
        ),
    ]


def scenario_03_multi_tool() -> list[dict]:
    """Read a file, then run a command — two apply steps before end_turn."""
    pid = "prompt-03"
    c1, c2 = "call-read-01", "call-bash-01"
    return [
        user_chunk("What's in Cargo.toml and the git status?", f"{SESSION}-20"),
        thought_chunk("Read Cargo.toml then run git status.", f"{SESSION}-21", pid),
        tool_call(
            call_id=c1,
            name="read_file",
            raw_input={"target_file": "/workspace/demo/Cargo.toml"},
            event_id=f"{SESSION}-22",
            prompt_id=pid,
            kind="read",
        ),
        tool_completed(
            call_id=c1,
            output='[package]\nname = "demo"\nversion = "0.1.0"\n',
            event_id=f"{SESSION}-23",
        ),
        tool_call(
            call_id=c2,
            name="run_terminal_command",
            raw_input={
                "command": "git status --short",
                "description": "Show short git status",
            },
            event_id=f"{SESSION}-24",
            prompt_id=pid,
            kind="execute",
            read_only=False,
        ),
        tool_completed(
            call_id=c2,
            output=" M Cargo.toml\n?? src/main.rs\n",
            event_id=f"{SESSION}-25",
        ),
        message_chunk(
            "Cargo.toml names package demo; git shows a modified Cargo.toml and untracked main.rs.",
            f"{SESSION}-26",
            pid,
        ),
        turn_completed(
            prompt_id=pid, event_id=f"{SESSION}-27", input_tokens=400, output_tokens=60
        ),
    ]


def scenario_04_error_recovery() -> list[dict]:
    """Tool fails once; model recovers with a successful read."""
    pid = "prompt-04"
    bad, good = "call-read-missing", "call-read-ok"
    return [
        user_chunk("Open missing.rs please.", f"{SESSION}-30"),
        thought_chunk("Try reading the path the user gave.", f"{SESSION}-31", pid),
        tool_call(
            call_id=bad,
            name="read_file",
            raw_input={"target_file": "/workspace/demo/missing.rs"},
            event_id=f"{SESSION}-32",
            prompt_id=pid,
            kind="read",
        ),
        tool_completed(
            call_id=bad,
            output="Error: File not found: /workspace/demo/missing.rs",
            event_id=f"{SESSION}-33",
            is_error=True,
        ),
        message_chunk(
            "That path doesn't exist. Checking src/main.rs instead.",
            f"{SESSION}-34",
            pid,
        ),
        tool_call(
            call_id=good,
            name="read_file",
            raw_input={"target_file": "/workspace/demo/src/main.rs"},
            event_id=f"{SESSION}-35",
            prompt_id=pid,
            kind="read",
        ),
        tool_completed(
            call_id=good,
            output='fn main() {\n    println!("hello");\n}\n',
            event_id=f"{SESSION}-36",
        ),
        message_chunk(
            "Found main.rs with a hello world entrypoint.",
            f"{SESSION}-37",
            pid,
        ),
        turn_completed(
            prompt_id=pid, event_id=f"{SESSION}-38", input_tokens=300, output_tokens=50
        ),
    ]


def scenario_05_edit_and_test() -> list[dict]:
    """Edit a file then run tests — creative + verificatory for Story verbs."""
    pid = "prompt-05"
    edit, test = "call-edit-01", "call-test-01"
    return [
        user_chunk("Bump the version and run tests.", f"{SESSION}-40"),
        thought_chunk("Edit Cargo.toml then cargo test.", f"{SESSION}-41", pid),
        tool_call(
            call_id=edit,
            name="search_replace",
            raw_input={
                "file_path": "/workspace/demo/Cargo.toml",
                "old_string": 'version = "0.1.0"',
                "new_string": 'version = "0.1.1"',
            },
            event_id=f"{SESSION}-42",
            prompt_id=pid,
            kind="edit",
            read_only=False,
        ),
        tool_completed(
            call_id=edit,
            output="The file /workspace/demo/Cargo.toml has been updated.",
            event_id=f"{SESSION}-43",
        ),
        tool_call(
            call_id=test,
            name="run_terminal_command",
            raw_input={
                "command": "cargo test -q",
                "description": "Run cargo tests",
            },
            event_id=f"{SESSION}-44",
            prompt_id=pid,
            kind="execute",
            read_only=False,
        ),
        tool_completed(
            call_id=test,
            output="running 3 tests\nall tests passed\n",
            event_id=f"{SESSION}-45",
        ),
        message_chunk(
            "Bumped version to 0.1.1; cargo test passed.",
            f"{SESSION}-46",
            pid,
        ),
        turn_completed(
            prompt_id=pid, event_id=f"{SESSION}-47", input_tokens=500, output_tokens=80
        ),
    ]


SCENARIOS = {
    "scenario_01_text_only.jsonl": scenario_01_text_only,
    "scenario_02_single_tool.jsonl": scenario_02_single_tool,
    "scenario_03_multi_tool.jsonl": scenario_03_multi_tool,
    "scenario_04_error_recovery.jsonl": scenario_04_error_recovery,
    "scenario_05_edit_and_test.jsonl": scenario_05_edit_and_test,
}


def write_provenance(out: Path) -> None:
    text = f"""# Grok Build golden fixtures — provenance

Synthetic but wire-faithful ACP `updates.jsonl` lines generated by
`scripts/gen_grok_fixtures.py`. Shapes match real Grok Build sessions under
`~/.grok/sessions/{{urlencoded-cwd}}/{{session-id}}/updates.jsonl`
(observed 2026-07-16 against Grok Build open-source tree).

Fixed session id used in all scenarios: `{SESSION}`

| File | Intent |
|------|--------|
| `scenario_01_text_only.jsonl` | User prompt → thought → text → turn_completed (no tools) |
| `scenario_02_single_tool.jsonl` | list_dir apply + in_progress (skipped) + completed |
| `scenario_03_multi_tool.jsonl` | read_file + run_terminal_command chain |
| `scenario_04_error_recovery.jsonl` | failed read then successful recovery |
| `scenario_05_edit_and_test.jsonl` | search_replace + cargo test (Story verb coverage) |
| `sample_updates.jsonl` | Minimized extract from a real session (hand-curated) |

Regenerate scenarios (not sample_updates):

```
python3 scripts/gen_grok_fixtures.py
```
"""
    (out / "PROVENANCE.md").write_text(text, encoding="utf-8")


def generate(out: Path) -> list[Path]:
    written = []
    for name, factory in SCENARIOS.items():
        path = out / name
        write_jsonl(path, factory())
        written.append(path)
    write_provenance(out)
    return written


def self_test() -> None:
    """Lightweight --test: every scenario has user + turn_completed and valid JSON."""
    import tempfile

    with tempfile.TemporaryDirectory() as td:
        paths = generate(Path(td))
        assert len(paths) == len(SCENARIOS)
        for path in paths:
            rows = [json.loads(l) for l in path.read_text().splitlines() if l.strip()]
            assert rows, path
            kinds = [r["params"]["update"]["sessionUpdate"] for r in rows]
            assert "user_message_chunk" in kinds, path
            assert "turn_completed" in kinds, path
            assert all(r["params"]["sessionId"] == SESSION for r in rows)
            # methods for turn_completed
            for r in rows:
                if r["params"]["update"]["sessionUpdate"] == "turn_completed":
                    assert r["method"] == "_x.ai/session/update"
    print("gen_grok_fixtures.py --test: ok")


def main(argv: list[str] | None = None) -> int:
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--out", type=Path, default=DEFAULT_OUT, help="fixture directory")
    p.add_argument("--test", action="store_true", help="run self-test and exit")
    args = p.parse_args(argv)
    if args.test:
        self_test()
        return 0
    written = generate(args.out)
    for path in written:
        print(f"wrote {path.relative_to(ROOT)} ({sum(1 for _ in path.open())} lines)")
    print(f"wrote {args.out.relative_to(ROOT) / 'PROVENANCE.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
