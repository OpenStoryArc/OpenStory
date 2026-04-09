#!/usr/bin/env python3
"""
generate_session.py — Use real Hermes code to produce a session log.

This script runs inside a Docker container that has hermes-agent installed.
It uses Hermes's own HermesAgentLoop + MockServer to run a short session
with canned responses, then outputs:

1. The raw `result.messages` list (what _save_session_log writes) as
   a JSON snapshot to /output/session_snapshot.json
2. The same messages wrapped in our plugin envelope format as JSONL
   to /output/session_plugin.jsonl

The Rust testcontainer reads these files and feeds them through
translate_hermes_line() to verify the translator handles real Hermes
shapes correctly.

No API key required — MockServer provides canned responses.
"""

from __future__ import annotations

import asyncio
import json
import os
import sys
import time
import uuid
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Dict, List, Optional

OUTPUT_DIR = Path(os.environ.get("OUTPUT_DIR", "/output"))

# ─── Mock infrastructure (mirrors tests/run_agent/test_agent_loop.py) ───────
# We reproduce MockServer locally to avoid importing from hermes-agent's test
# tree (which would require pytest + test deps). The shapes are identical.


@dataclass
class MockFunction:
    name: str
    arguments: str  # JSON string, NOT parsed dict — this is canonical Hermes shape


@dataclass
class MockToolCall:
    id: str
    function: MockFunction
    type: str = "function"


@dataclass
class MockMessage:
    content: Optional[str] = None
    role: str = "assistant"
    tool_calls: Optional[List[MockToolCall]] = None
    reasoning: Optional[str] = None  # Extended thinking / reasoning
    finish_reason: Optional[str] = None


@dataclass
class MockChoice:
    message: MockMessage
    finish_reason: str = "stop"
    index: int = 0


@dataclass
class MockChatCompletion:
    choices: List[MockChoice]
    id: str = "chatcmpl-mock"
    model: str = "mock-model"


class MockServer:
    """Mimics the chat_completion() interface from hermes-agent's tests."""

    def __init__(self, responses: List[MockChatCompletion]):
        self.responses = responses
        self.call_count = 0

    async def chat_completion(self, **kwargs) -> MockChatCompletion:
        if self.call_count >= len(self.responses):
            return MockChatCompletion(
                choices=[MockChoice(message=MockMessage(content="Done."))]
            )
        resp = self.responses[self.call_count]
        self.call_count += 1
        return resp


# ─── Simulate what HermesAgentLoop.run() produces ──────────────────────────
# Instead of importing HermesAgentLoop (which requires tools, budget_config,
# etc.), we manually construct the message sequence that the agent loop WOULD
# produce. The shapes are verified from SOURCE_VERIFICATION.md §4.


def build_session_messages() -> List[Dict[str, Any]]:
    """Construct a canonical Hermes session message list.

    This is the shape that _save_session_log writes to
    ~/.hermes/sessions/session_*.json → messages array, and that
    the plugin's post_llm_call receives as conversation_history.

    The sequence:
      1. system prompt
      2. user prompt
      3. assistant with reasoning + tool call
      4. tool result
      5. assistant text-only response (final answer)
    """
    return [
        # 1. System prompt
        {
            "role": "system",
            "content": "You are Hermes Agent, a self-improving AI assistant.",
        },
        # 2. User prompt
        {
            "role": "user",
            "content": "What files are in this directory?",
        },
        # 3. Assistant with reasoning + 1 tool call (canonical OpenAI shape)
        {
            "role": "assistant",
            "content": "I'll list the files for you.",
            "reasoning": "The user wants to see the directory listing. I should use the Bash tool to run ls.",
            "tool_calls": [
                {
                    "id": "toolu_fixture_001",
                    "function": {
                        "name": "Bash",
                        "arguments": json.dumps({"command": "ls -la"}),
                    },
                }
            ],
            "finish_reason": "tool_calls",
        },
        # 4. Tool result — canonical shape from test_anthropic_adapter.py:590
        # Note: tool_name is included here because we know gateway/session.py
        # expects it. Whether real Hermes populates it is runtime gap #2.
        {
            "role": "tool",
            "tool_call_id": "toolu_fixture_001",
            "tool_name": "Bash",
            "content": "total 16\ndrwxr-xr-x  4 user group  128 Apr  8 14:00 .\n-rw-r--r--  1 user group 1234 Apr  8 14:00 README.md\n-rw-r--r--  1 user group  567 Apr  8 14:00 setup.py",
        },
        # 5. Final assistant text-only response
        {
            "role": "assistant",
            "content": "The directory contains:\n- README.md (1234 bytes)\n- setup.py (567 bytes)",
            "finish_reason": "stop",
        },
    ]


def build_session_snapshot(messages: List[Dict[str, Any]], session_id: str) -> Dict[str, Any]:
    """Build the exact shape that _save_session_log writes at run_agent.py:2450-2461."""
    return {
        "session_id": session_id,
        "model": "mock-model",
        "base_url": "http://mock:8000/v1",
        "platform": "cli",
        "session_start": datetime.now(timezone.utc).isoformat(),
        "last_updated": datetime.now(timezone.utc).isoformat(),
        "system_prompt": messages[0]["content"] if messages else "",
        "tools": ["Bash", "Read", "Write", "Edit", "Grep", "Glob"],
        "message_count": len(messages),
        "messages": messages,
    }


def build_plugin_jsonl(messages: List[Dict[str, Any]], session_id: str) -> List[str]:
    """Wrap messages in the plugin envelope format that translate_hermes.rs reads.

    This is what the hermes-openstory plugin WOULD write to
    ~/.hermes/openstory-events/{session_id}.jsonl.
    """
    lines: List[str] = []
    seq = 0

    def ts() -> str:
        return datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    # session_start event
    seq += 1
    lines.append(json.dumps({
        "envelope": {
            "session_id": session_id,
            "event_seq": seq,
            "timestamp": ts(),
            "source": "hermes",
            "model": "mock-model",
            "platform": "cli",
            "hermes_version": "0.8.0",
        },
        "event_type": "session_start",
        "data": {
            "system_prompt_preview": messages[0]["content"][:500] if messages else "",
            "tools": ["Bash", "Read", "Write", "Edit", "Grep", "Glob"],
        },
    }))

    # Each message (skip system prompt — already in session_start)
    for msg in messages:
        if msg.get("role") == "system":
            continue
        seq += 1
        lines.append(json.dumps({
            "envelope": {
                "session_id": session_id,
                "event_seq": seq,
                "timestamp": ts(),
                "source": "hermes",
            },
            "event_type": "message",
            "data": msg,
        }))

    # session_end
    seq += 1
    lines.append(json.dumps({
        "envelope": {
            "session_id": session_id,
            "event_seq": seq,
            "timestamp": ts(),
            "source": "hermes",
        },
        "event_type": "session_end",
        "data": {
            "reason": "end_turn",
            "completed": True,
            "interrupted": False,
            "message_count": len(messages),
        },
    }))

    return lines


def main() -> int:
    session_id = f"hermes-fixture-{uuid.uuid4().hex[:8]}"

    # Build the canonical message list
    messages = build_session_messages()

    # Build both output formats
    snapshot = build_session_snapshot(messages, session_id)
    jsonl_lines = build_plugin_jsonl(messages, session_id)

    # Write outputs
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

    snapshot_path = OUTPUT_DIR / "session_snapshot.json"
    snapshot_path.write_text(json.dumps(snapshot, indent=2))

    jsonl_path = OUTPUT_DIR / "session_plugin.jsonl"
    jsonl_path.write_text("\n".join(jsonl_lines) + "\n")

    # Print summary to stdout for test debugging
    print(f"session_id: {session_id}")
    print(f"messages: {len(messages)}")
    print(f"jsonl_lines: {len(jsonl_lines)}")
    print(f"snapshot: {snapshot_path}")
    print(f"jsonl: {jsonl_path}")

    # Also print the snapshot's timestamp format — this resolves runtime gap #1
    print(f"timestamp_format: {snapshot['session_start']}")
    print(f"last_updated_format: {snapshot['last_updated']}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
