#!/usr/bin/env python3
"""
Synthetic transcript generator for open-story test fixtures.

Generates statistically realistic Claude Code transcript data (raw JSONL)
that exercises the full translate → ingest → views pipeline without
containing any personal information.

The generator uses a hybrid approach:
  1. Grammar layer: valid event sequences and tree structure
  2. Statistical layer: distributions learned from real data

Usage:
    uv run python scripts/synth_transcripts.py --analyze         # learn from real data
    uv run python scripts/synth_transcripts.py --generate        # generate fixtures
    uv run python scripts/synth_transcripts.py --test            # run BDD tests
    uv run python scripts/synth_transcripts.py --validate FILE   # validate a JSONL file

Profiles:
    quick          — 1-3 turns, light tool use (Q&A session)
    implementation — 5-15 turns, heavy Edit/Bash (building a feature)
    debugging      — 3-10 turns, heavy Bash/Read (test-fix cycles)
    exploration    — 2-8 turns, heavy Read/Grep/Glob (code exploration)
    delegation     — 3-7 turns, subagent spawning
"""
from __future__ import annotations

import argparse
import json
import math
import random
import sys
import uuid as uuid_mod
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Optional


# =====================================================================
# Statistical Profile — distributions learned from real data
# =====================================================================

@dataclass
class Profile:
    """Statistical profile describing a session's characteristics."""
    # Tool name → relative frequency (sums to 1.0)
    tool_dist: dict[str, float] = field(default_factory=dict)
    # Turns per session: (mean, std) for log-normal
    turns_mean: float = 2.0
    turns_std: float = 0.8
    # Tools per turn: (lambda) for Poisson
    tools_per_turn_lambda: float = 3.5
    # Token counts: (mean, std) for log-normal
    input_tokens_mean: float = 8.0
    input_tokens_std: float = 1.2
    output_tokens_mean: float = 6.0
    output_tokens_std: float = 1.0
    # Timing: mean gap in ms between events
    intra_turn_gap_ms: float = 800.0
    inter_turn_gap_ms: float = 5000.0
    # Probability of thinking block in assistant message
    thinking_prob: float = 0.6
    # Probability of subagent delegation per tool call
    delegation_prob: float = 0.02
    # Probability of hook progress per tool call
    hook_progress_prob: float = 0.3


# Default profile based on analysis of 42 real sessions (from memory notes)
DEFAULT_PROFILE = Profile(
    tool_dist={
        "Bash": 0.30, "Read": 0.25, "Edit": 0.12, "Write": 0.06,
        "Grep": 0.07, "Glob": 0.05, "Agent": 0.03, "WebSearch": 0.02,
        "WebFetch": 0.02, "TaskCreate": 0.02, "TaskUpdate": 0.02,
        "Skill": 0.01, "NotebookEdit": 0.01, "AskUserQuestion": 0.01,
        "TodoWrite": 0.01,
    },
    turns_mean=2.0, turns_std=0.8,
    tools_per_turn_lambda=3.5,
    input_tokens_mean=8.0, input_tokens_std=1.2,
    output_tokens_mean=6.0, output_tokens_std=1.0,
    intra_turn_gap_ms=800.0, inter_turn_gap_ms=5000.0,
    thinking_prob=0.6, delegation_prob=0.02,
    hook_progress_prob=0.3,
)


# =====================================================================
# Session personalities — named presets that adjust the profile
# =====================================================================

PERSONALITIES: dict[str, dict] = {
    "quick": {
        "turns_mean": 1.0, "turns_std": 0.3,
        "tools_per_turn_lambda": 1.0,
        "tool_dist": {"Read": 0.5, "Bash": 0.2, "Grep": 0.15, "Glob": 0.15},
        "thinking_prob": 0.3, "delegation_prob": 0.0,
    },
    "implementation": {
        "turns_mean": 2.3, "turns_std": 0.5,
        "tools_per_turn_lambda": 5.0,
        "tool_dist": {
            "Edit": 0.25, "Bash": 0.25, "Read": 0.20, "Write": 0.10,
            "Grep": 0.08, "Glob": 0.05, "Agent": 0.03, "TaskCreate": 0.02,
            "TaskUpdate": 0.02,
        },
        "thinking_prob": 0.7, "delegation_prob": 0.03,
    },
    "debugging": {
        "turns_mean": 2.0, "turns_std": 0.6,
        "tools_per_turn_lambda": 6.0,
        "tool_dist": {
            "Bash": 0.35, "Read": 0.25, "Grep": 0.15, "Edit": 0.10,
            "Glob": 0.05, "Write": 0.05, "Agent": 0.03, "WebSearch": 0.02,
        },
        "thinking_prob": 0.8, "delegation_prob": 0.02,
    },
    "exploration": {
        "turns_mean": 1.5, "turns_std": 0.5,
        "tools_per_turn_lambda": 8.0,
        "tool_dist": {
            "Read": 0.30, "Grep": 0.25, "Glob": 0.20, "Bash": 0.10,
            "Agent": 0.05, "WebSearch": 0.05, "WebFetch": 0.03,
            "Edit": 0.02,
        },
        "thinking_prob": 0.5, "delegation_prob": 0.05,
    },
    "delegation": {
        "turns_mean": 1.8, "turns_std": 0.4,
        "tools_per_turn_lambda": 3.0,
        "tool_dist": {
            "Agent": 0.30, "Read": 0.20, "Bash": 0.15, "Edit": 0.10,
            "Grep": 0.10, "Glob": 0.05, "Write": 0.05, "TaskCreate": 0.03,
            "TaskUpdate": 0.02,
        },
        "thinking_prob": 0.6, "delegation_prob": 0.15,
    },
}


def make_profile(personality: str) -> Profile:
    """Create a Profile from a named personality."""
    overrides = PERSONALITIES.get(personality, {})
    p = Profile(**{**DEFAULT_PROFILE.__dict__, **overrides})
    return p


# =====================================================================
# Content banks — synthetic but realistic-looking content
# =====================================================================

USER_PROMPTS = [
    "Implement the login feature with JWT authentication",
    "Fix the failing test in auth.ts",
    "Refactor the database module to use connection pooling",
    "Add pagination to the /api/users endpoint",
    "Write unit tests for the validation layer",
    "Set up the CI/CD pipeline with GitHub Actions",
    "Optimize the query performance for the dashboard",
    "Add error handling to the file upload service",
    "Create a migration script for the new schema",
    "Update the README with setup instructions",
    "Debug the memory leak in the WebSocket handler",
    "Add rate limiting to the API endpoints",
    "Implement the search feature with full-text indexing",
    "Fix the CORS configuration for the staging environment",
    "Add logging and monitoring to the background workers",
    "Refactor the event system to use typed events",
    "Write integration tests for the payment flow",
    "Add input validation to all form handlers",
    "Implement the notification system with WebSockets",
    "Fix the race condition in the session manager",
    "Add TypeScript types for the API response objects",
    "Create a data export feature in CSV format",
    "Optimize the build process to reduce bundle size",
    "Add health check endpoints for the load balancer",
    "Implement role-based access control for admin routes",
    "Fix the date parsing issue in the analytics module",
    "Add caching to the frequently accessed endpoints",
    "Write a migration to normalize the address table",
    "Set up automated backups for the production database",
    "Add request tracing with correlation IDs",
]

ASSISTANT_TEXTS = [
    "I'll start by reading the relevant files to understand the current implementation.",
    "Let me analyze the test output to identify the root cause.",
    "I'll make the changes incrementally, starting with the core logic.",
    "Let me check the existing tests first to understand the expected behavior.",
    "I'll implement this using the existing patterns in the codebase.",
    "Let me run the tests to verify the changes work correctly.",
    "I've made the fix. Let me verify it passes all related tests.",
    "Let me search for similar patterns in the codebase to ensure consistency.",
    "I'll update the types first, then modify the implementation to match.",
    "Here's the implementation. Let me walk you through the key changes.",
    "The issue was in the error handling path. I've added proper error propagation.",
    "I'll need to modify three files to implement this feature properly.",
    "Let me check if there are any edge cases we need to handle.",
    "The tests are passing now. Here's a summary of what I changed.",
    "I've refactored the code to be more maintainable without changing behavior.",
]

THINKING_TEXTS = [
    "Let me analyze this step by step. First, I need to understand the data flow...",
    "I should check what the existing implementation does before making changes.",
    "There are several approaches here. The simplest would be to...",
    "Looking at the test output, the failure is in the assertion on line 42...",
    "I need to be careful about backwards compatibility here.",
    "The user wants me to fix the failing test. Let me trace the error...",
    "This is a refactoring task. I should ensure all existing tests still pass.",
    "Let me think about edge cases: empty input, null values, unicode...",
    "The performance bottleneck is likely in the database query. Let me profile...",
    "I'll use the builder pattern here to keep the API clean.",
]

FILE_PATHS = [
    "/project/src/main.rs", "/project/src/lib.rs",
    "/project/src/server/mod.rs", "/project/src/server/api.rs",
    "/project/src/server/ws.rs", "/project/src/config.rs",
    "/project/src/models/user.rs", "/project/src/models/session.rs",
    "/project/src/db/connection.rs", "/project/src/db/queries.rs",
    "/project/tests/test_api.rs", "/project/tests/test_auth.rs",
    "/project/ui/src/App.tsx", "/project/ui/src/components/Dashboard.tsx",
    "/project/ui/src/lib/utils.ts", "/project/ui/src/hooks/useAuth.ts",
    "/project/Cargo.toml", "/project/package.json",
    "/project/README.md", "/project/docker-compose.yml",
    "/project/.github/workflows/test.yml",
]

BASH_COMMANDS = [
    "cargo test", "cargo build", "cargo clippy",
    "npm test", "npm run build", "npm run dev",
    "git status", "git diff", "git log --oneline -5",
    "ls -la src/", "cat Cargo.toml",
    "grep -r 'TODO' src/", "find . -name '*.rs' | head",
    "docker compose up -d", "docker ps",
    "curl -s http://localhost:3000/health",
    "python -m pytest tests/",
]

BASH_OUTPUTS = [
    "   Compiling project v0.1.0\n    Finished dev [unoptimized] target(s)",
    "test result: ok. 42 passed; 0 failed; 0 ignored",
    "test result: FAILED. 41 passed; 1 failed; 0 ignored",
    "error[E0308]: mismatched types\n  --> src/main.rs:15:5",
    "warning: unused variable `x`\n  --> src/lib.rs:10:9",
    "On branch main\nnothing to commit, working tree clean",
    "PASS  src/tests/auth.test.ts\nTests: 5 passed, 5 total",
    "CONTAINER ID   IMAGE          STATUS    PORTS\nabc123   postgres:15   Up 2m     5432/tcp",
    "{\"status\":\"healthy\",\"uptime\":3600}",
    "src/main.rs\nsrc/lib.rs\nsrc/server/mod.rs",
]

GREP_PATTERNS = [
    "fn main", "struct.*Config", "impl.*Handler",
    "use tokio", "async fn", "#\\[test\\]",
    "TODO", "FIXME", "pub fn",
]

TOOL_RESULT_CONTENTS = [
    "     1\tuse std::io;\n     2\t\n     3\tfn main() {\n     4\t    println!(\"Hello\");\n     5\t}\n",
    "test result: ok. 12 passed; 0 failed\n",
    "error: could not compile `project`\n",
    "No files found matching pattern\n",
    "[package]\nname = \"project\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    "     1\timport React from 'react';\n     2\t\n     3\texport function App() {\n",
    "M  src/main.rs\nM  src/lib.rs\n?? src/new_file.rs\n",
    "abc1234 Fix authentication bug\ndef5678 Add user model\nghi9012 Initial commit\n",
]

AGENT_PROMPTS = [
    "Search the codebase for all usages of the deprecated API",
    "Explore the test directory to understand the testing patterns",
    "Find all files that import the auth module",
    "Analyze the database schema and report the table relationships",
]


# =====================================================================
# UUID + timestamp generators (deterministic from seed)
# =====================================================================

class IdGen:
    """Deterministic ID generator using a seeded RNG."""

    def __init__(self, rng: random.Random):
        self._rng = rng
        self._msg_counter = 0
        self._tool_counter = 0

    def uuid(self) -> str:
        """Generate a random UUID-like string."""
        return str(uuid_mod.UUID(int=self._rng.getrandbits(128), version=4))

    def msg_id(self) -> str:
        """Generate a Claude-style message ID."""
        self._msg_counter += 1
        suffix = "".join(self._rng.choices("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789", k=20))
        return f"msg_{suffix}"

    def tool_use_id(self) -> str:
        """Generate a Claude-style tool use ID."""
        self._tool_counter += 1
        suffix = "".join(self._rng.choices("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789", k=20))
        return f"toolu_{suffix}"


class Clock:
    """Monotonic timestamp generator with realistic gaps."""

    def __init__(self, rng: random.Random, start: Optional[datetime] = None):
        self._rng = rng
        self._now = start or datetime(2026, 3, 10, 14, 0, 0, tzinfo=timezone.utc)

    def tick(self, mean_gap_ms: float = 800.0) -> str:
        """Advance clock by a random gap and return ISO timestamp."""
        gap_ms = max(50, self._rng.expovariate(1.0 / mean_gap_ms))
        self._now += timedelta(milliseconds=gap_ms)
        return self._now.strftime("%Y-%m-%dT%H:%M:%S.") + f"{self._now.microsecond // 1000:03d}Z"

    def current(self) -> str:
        return self._now.strftime("%Y-%m-%dT%H:%M:%S.") + f"{self._now.microsecond // 1000:03d}Z"


# =====================================================================
# Grammar-based session generator
# =====================================================================

@dataclass
class SessionContext:
    """Mutable context for a session being generated."""
    session_id: str
    rng: random.Random
    ids: IdGen
    clock: Clock
    profile: Profile
    lines: list[dict] = field(default_factory=list)
    # Track parent chain for tree structure
    last_uuid: Optional[str] = None
    cwd: str = "/home/dev/project"
    version: str = "2.3.0"
    git_branch: str = "main"
    model: str = "claude-sonnet-4-6"


def _sample_tool(ctx: SessionContext) -> str:
    """Sample a tool name from the profile's distribution."""
    names = list(ctx.profile.tool_dist.keys())
    weights = [ctx.profile.tool_dist[n] for n in names]
    return ctx.rng.choices(names, weights=weights, k=1)[0]


def _sample_lognormal(rng: random.Random, mu: float, sigma: float) -> int:
    """Sample from log-normal, return as positive int."""
    return max(1, int(math.exp(rng.gauss(mu, sigma))))


def _make_envelope(ctx: SessionContext, parent_uuid: Optional[str] = None) -> dict:
    """Create the common envelope fields."""
    uid = ctx.ids.uuid()
    env = {
        "uuid": uid,
        "sessionId": ctx.session_id,
        "timestamp": ctx.clock.tick(ctx.profile.intra_turn_gap_ms),
        "cwd": ctx.cwd,
        "version": ctx.version,
        "gitBranch": ctx.git_branch,
    }
    if parent_uuid is not None:
        env["parentUuid"] = parent_uuid
    ctx.last_uuid = uid
    return env


def _tool_input(ctx: SessionContext, tool_name: str) -> dict:
    """Generate realistic tool input for the given tool."""
    rng = ctx.rng
    if tool_name == "Bash":
        return {"command": rng.choice(BASH_COMMANDS)}
    elif tool_name == "Read":
        return {"file_path": rng.choice(FILE_PATHS)}
    elif tool_name == "Edit":
        return {
            "file_path": rng.choice(FILE_PATHS),
            "old_string": "old_value",
            "new_string": "new_value",
        }
    elif tool_name == "Write":
        return {
            "file_path": rng.choice(FILE_PATHS),
            "content": "// generated content\n",
        }
    elif tool_name in ("Grep", "Glob"):
        return {"pattern": rng.choice(GREP_PATTERNS)}
    elif tool_name == "Agent":
        return {
            "prompt": rng.choice(AGENT_PROMPTS),
            "description": "code exploration",
        }
    elif tool_name in ("WebSearch", "WebFetch"):
        return {"query": "rust async patterns"}
    elif tool_name in ("TaskCreate", "TaskUpdate"):
        return {"description": "implement feature"}
    else:
        return {}


def _tool_result_content(ctx: SessionContext, tool_name: str) -> str:
    """Generate realistic tool result content."""
    rng = ctx.rng
    if tool_name == "Bash":
        return rng.choice(BASH_OUTPUTS)
    elif tool_name == "Read":
        return rng.choice(TOOL_RESULT_CONTENTS)
    elif tool_name in ("Edit", "Write"):
        return "File updated successfully."
    elif tool_name in ("Grep", "Glob"):
        paths = rng.sample(FILE_PATHS, min(3, len(FILE_PATHS)))
        return "\n".join(paths)
    elif tool_name == "Agent":
        return "Agent completed analysis. Found 3 relevant files."
    else:
        return "Operation completed."


def emit_file_snapshot(ctx: SessionContext) -> None:
    """Emit a file-history-snapshot (session root)."""
    env = _make_envelope(ctx, parent_uuid=None)
    env["type"] = "file-history-snapshot"
    env["messageId"] = ctx.ids.msg_id()
    env["snapshot"] = {
        "trackedFileBackups": {},
        "timestamp": ctx.clock.current(),
    }
    # file snapshots don't carry sessionId in real data sometimes
    ctx.lines.append(env)


def emit_hook_progress(ctx: SessionContext, hook_event: str = "PostToolUse",
                       parent: Optional[str] = None) -> None:
    """Emit a hook progress event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "progress"
    env["data"] = {
        "type": "hook_progress",
        "hookEvent": hook_event,
    }
    ctx.lines.append(env)


def emit_user_prompt(ctx: SessionContext, parent: Optional[str] = None) -> str:
    """Emit a user prompt message. Returns the event UUID."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "user"
    env["message"] = {
        "role": "user",
        "content": ctx.rng.choice(USER_PROMPTS),
    }
    ctx.lines.append(env)
    return env["uuid"]


def emit_user_tool_result(ctx: SessionContext, tool_use_id: str,
                          tool_name: str, parent: Optional[str] = None,
                          is_error: bool = False) -> str:
    """Emit a user message containing a tool_result. Returns UUID."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "user"
    content_text = _tool_result_content(ctx, tool_name)
    env["message"] = {
        "role": "user",
        "content": [
            {
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": content_text,
                "is_error": is_error,
            }
        ],
    }
    ctx.lines.append(env)
    return env["uuid"]


def emit_assistant_text(ctx: SessionContext, parent: Optional[str] = None,
                        with_thinking: bool = False) -> str:
    """Emit an assistant text response. Returns UUID."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "assistant"
    content_blocks = []
    if with_thinking:
        content_blocks.append({
            "type": "thinking",
            "thinking": ctx.rng.choice(THINKING_TEXTS),
        })
    content_blocks.append({
        "type": "text",
        "text": ctx.rng.choice(ASSISTANT_TEXTS),
    })
    env["message"] = {
        "role": "assistant",
        "model": ctx.model,
        "id": ctx.ids.msg_id(),
        "content": content_blocks,
        "usage": {
            "input_tokens": _sample_lognormal(ctx.rng, ctx.profile.input_tokens_mean, ctx.profile.input_tokens_std),
            "output_tokens": _sample_lognormal(ctx.rng, ctx.profile.output_tokens_mean, ctx.profile.output_tokens_std),
        },
        "stop_reason": "end_turn",
    }
    ctx.lines.append(env)
    return env["uuid"]


def emit_assistant_tool_use(ctx: SessionContext, tool_name: str,
                            parent: Optional[str] = None,
                            with_thinking: bool = False) -> tuple[str, str]:
    """Emit an assistant message with a tool_use block.
    Returns (event_uuid, tool_use_id)."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "assistant"
    tool_id = ctx.ids.tool_use_id()
    content_blocks = []
    if with_thinking:
        content_blocks.append({
            "type": "thinking",
            "thinking": ctx.rng.choice(THINKING_TEXTS),
        })
    content_blocks.append({
        "type": "tool_use",
        "id": tool_id,
        "name": tool_name,
        "input": _tool_input(ctx, tool_name),
    })
    env["message"] = {
        "role": "assistant",
        "model": ctx.model,
        "id": ctx.ids.msg_id(),
        "content": content_blocks,
        "usage": {
            "input_tokens": _sample_lognormal(ctx.rng, ctx.profile.input_tokens_mean, ctx.profile.input_tokens_std),
            "output_tokens": _sample_lognormal(ctx.rng, ctx.profile.output_tokens_mean, ctx.profile.output_tokens_std),
        },
        "stop_reason": "tool_use",
    }
    ctx.lines.append(env)
    return env["uuid"], tool_id


def emit_bash_progress(ctx: SessionContext, parent: Optional[str] = None) -> None:
    """Emit a bash progress event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "progress"
    env["data"] = {
        "type": "bash_progress",
        "output": ctx.rng.choice(BASH_OUTPUTS)[:200],
    }
    ctx.lines.append(env)


def emit_agent_progress(ctx: SessionContext, agent_id: str,
                        parent: Optional[str] = None) -> None:
    """Emit an agent progress event.

    Real data has agentId nested inside data AND at the top level.
    The test_subagent_identity test specifically checks data.agentId (camelCase).
    """
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "progress"
    env["data"] = {
        "type": "agent_progress",
        "agentId": agent_id,
    }
    # Also at top level (real data has it both places)
    env["agentId"] = agent_id
    ctx.lines.append(env)


def emit_turn_complete(ctx: SessionContext, duration_ms: int,
                       parent: Optional[str] = None) -> None:
    """Emit a system turn_duration event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "system"
    env["subtype"] = "turn_duration"
    env["durationMs"] = duration_ms
    ctx.lines.append(env)


def emit_stop_hook(ctx: SessionContext, hook_count: int = 1,
                   parent: Optional[str] = None) -> None:
    """Emit a system stop_hook_summary event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "system"
    env["subtype"] = "stop_hook_summary"
    env["hookCount"] = hook_count
    env["preventedContinuation"] = False
    ctx.lines.append(env)


def emit_system_error(ctx: SessionContext, parent: Optional[str] = None) -> None:
    """Emit a system error event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "system"
    env["subtype"] = "api_error"
    ctx.lines.append(env)


def emit_compact_boundary(ctx: SessionContext, parent: Optional[str] = None) -> None:
    """Emit a system compact_boundary event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "system"
    env["subtype"] = "compact_boundary"
    ctx.lines.append(env)


def emit_queue_op(ctx: SessionContext, operation: str,
                  parent: Optional[str] = None) -> None:
    """Emit a queue operation event."""
    env = _make_envelope(ctx, parent_uuid=parent)
    env["type"] = "queue-operation"
    env["operation"] = operation
    if operation == "enqueue":
        env["content"] = ctx.rng.choice(USER_PROMPTS)
    ctx.lines.append(env)


# =====================================================================
# Turn generator — the grammar
# =====================================================================

def generate_turn(ctx: SessionContext, parent_uuid: Optional[str] = None,
                  is_first: bool = False) -> str:
    """Generate a complete turn: user prompt → assistant phases → system events.
    Returns the UUID of the last event in the turn."""

    # User prompt
    prompt_uuid = emit_user_prompt(ctx, parent=parent_uuid)

    # Sample how many tool cycles this turn has
    n_tools = max(0, int(ctx.rng.expovariate(1.0 / ctx.profile.tools_per_turn_lambda)))
    # Cap at something reasonable
    n_tools = min(n_tools, 20)

    current_parent = prompt_uuid

    for i in range(n_tools):
        tool_name = _sample_tool(ctx)
        has_thinking = ctx.rng.random() < ctx.profile.thinking_prob and i == 0

        # Hook progress before tool (sometimes)
        if ctx.rng.random() < ctx.profile.hook_progress_prob:
            emit_hook_progress(ctx, "PreToolUse", parent=current_parent)

        # Assistant tool_use
        asst_uuid, tool_id = emit_assistant_tool_use(
            ctx, tool_name, parent=current_parent, with_thinking=has_thinking
        )

        # Bash progress for Bash tool calls
        if tool_name == "Bash":
            emit_bash_progress(ctx, parent=asst_uuid)

        # Agent delegation
        if tool_name == "Agent" or (ctx.rng.random() < ctx.profile.delegation_prob):
            agent_id = f"agent-{ctx.ids.uuid()[:8]}"
            emit_agent_progress(ctx, agent_id, parent=asst_uuid)

        # Hook progress after tool (sometimes)
        if ctx.rng.random() < ctx.profile.hook_progress_prob:
            emit_hook_progress(ctx, "PostToolUse", parent=asst_uuid)

        # User tool_result
        is_error = ctx.rng.random() < 0.05  # 5% error rate
        result_uuid = emit_user_tool_result(
            ctx, tool_id, tool_name, parent=asst_uuid, is_error=is_error
        )
        current_parent = result_uuid

    # Final assistant text response
    has_thinking = ctx.rng.random() < ctx.profile.thinking_prob
    final_uuid = emit_assistant_text(ctx, parent=current_parent, with_thinking=has_thinking)

    # Turn complete
    turn_duration = int(ctx.rng.expovariate(1.0 / 5000))
    emit_turn_complete(ctx, turn_duration, parent=final_uuid)

    # Stop hook summary (sometimes)
    if ctx.rng.random() < 0.3:
        emit_stop_hook(ctx, hook_count=ctx.rng.randint(1, 3), parent=final_uuid)

    # Inter-turn gap
    ctx.clock.tick(ctx.profile.inter_turn_gap_ms)

    return final_uuid


# =====================================================================
# Session generator — composes turns
# =====================================================================

def generate_session(
    profile: Profile,
    session_id: Optional[str] = None,
    seed: int = 42,
    min_events: int = 0,
    git_branch: str = "main",
    cwd: str = "/home/dev/project",
) -> list[dict]:
    """Generate a complete synthetic session.

    Returns a list of raw transcript line dicts (the format that
    translate_line() expects as input).
    """
    rng = random.Random(seed)
    ids = IdGen(rng)
    clock = Clock(rng)

    if session_id is None:
        session_id = ids.uuid()

    ctx = SessionContext(
        session_id=session_id,
        rng=rng,
        ids=ids,
        clock=clock,
        profile=profile,
        cwd=cwd,
        git_branch=git_branch,
    )

    # Optional file snapshot at start
    if rng.random() < 0.4:
        emit_file_snapshot(ctx)

    # Initial hook progress (SessionStart)
    emit_hook_progress(ctx, "SessionStart")

    # Sample number of turns
    n_turns = max(1, _sample_lognormal(rng, profile.turns_mean, profile.turns_std))
    n_turns = min(n_turns, 50)  # safety cap

    # Keep generating turns until we have enough events
    last_uuid = None
    turn_count = 0
    while turn_count < n_turns or len(ctx.lines) < min_events:
        last_uuid = generate_turn(ctx, parent_uuid=last_uuid, is_first=(turn_count == 0))
        turn_count += 1
        if turn_count > 100:  # hard cap
            break

    # Occasional compact boundary in long sessions
    if len(ctx.lines) > 200 and rng.random() < 0.5:
        emit_compact_boundary(ctx, parent=last_uuid)

    # Occasional error
    if rng.random() < 0.1:
        emit_system_error(ctx, parent=last_uuid)

    return ctx.lines


# =====================================================================
# Fixture generation — the specific files tests expect
# =====================================================================

def generate_test_fixtures(output_dir: Path, seed: int = 42) -> dict[str, int]:
    """Generate all fixture files needed by the Rust test suite.

    Returns a dict of filename → line count.
    """
    results = {}

    # 1. synth_origin.jsonl — replaces real_session_origin.jsonl
    #    Requirements:
    #    - > 100 ViewRecords (after ephemeral filtering)
    #    - tree depth >= 1 (non-trivial parent chains)
    #    - tools > 0, narrative > 0
    #    - is_sidechain = false for all events
    #    - at least some progress events with agent_id
    #    - multiple subtypes (>= 2)
    origin = generate_session(
        make_profile("implementation"),
        session_id="synth-origin",
        seed=seed,
        min_events=200,
        git_branch="main",
        cwd="/home/dev/project",
    )
    _write_jsonl(output_dir / "synth_origin.jsonl", origin)
    results["synth_origin.jsonl"] = len(origin)

    # 2. synth_hooks.jsonl — replaces real_session_hooks.jsonl
    #    Requirements:
    #    - tools > 0, narrative > 0
    #    - all filter counts valid (each <= "all")
    #    - filter deltas accumulate correctly
    #    - different event count from origin
    hooks = generate_session(
        make_profile("debugging"),
        session_id="synth-hooks",
        seed=seed + 1,
        min_events=300,
        git_branch="fix/auth-bug",
        cwd="/home/dev/project",
    )
    _write_jsonl(output_dir / "synth_hooks.jsonl", hooks)
    results["synth_hooks.jsonl"] = len(hooks)

    # 3. synth_translator.jsonl — replaces real_session_translator.jsonl
    #    Requirements:
    #    - valid JSON, multiple subtypes
    #    - CloudEvent schema compliance
    translator = generate_session(
        make_profile("exploration"),
        session_id="synth-translator",
        seed=seed + 2,
        min_events=150,
        git_branch="feature/search",
        cwd="/home/dev/project",
    )
    _write_jsonl(output_dir / "synth_translator.jsonl", translator)
    results["synth_translator.jsonl"] = len(translator)

    # 4. synth_global.jsonl — replaces real_session_global.jsonl
    #    Requirements:
    #    - 500+ events (tests capping at 500 records in initial_state)
    #    - tree depth >= 2
    global_session = generate_session(
        make_profile("implementation"),
        session_id="synth-global",
        seed=seed + 3,
        min_events=700,
        git_branch="master",
        cwd="/home/dev/project",
    )
    _write_jsonl(output_dir / "synth_global.jsonl", global_session)
    results["synth_global.jsonl"] = len(global_session)

    return results


def _write_jsonl(path: Path, lines: list[dict]) -> None:
    """Write lines as JSONL."""
    path.parent.mkdir(parents=True, exist_ok=True)
    with open(path, "w", encoding="utf-8") as f:
        for line in lines:
            f.write(json.dumps(line, separators=(",", ":")) + "\n")


# =====================================================================
# Validation — check structural invariants
# =====================================================================

def validate_session(lines: list[dict]) -> list[str]:
    """Validate structural invariants of a generated session.
    Returns a list of error messages (empty = valid).
    """
    errors = []
    uuids_seen: set[str] = set()
    prev_ts = None

    for i, line in enumerate(lines):
        line_type = line.get("type", "")

        # Required: type field
        if not line_type:
            errors.append(f"line {i}: missing 'type' field")

        # UUID uniqueness
        uid = line.get("uuid")
        if uid:
            if uid in uuids_seen:
                errors.append(f"line {i}: duplicate uuid {uid}")
            uuids_seen.add(uid)

        # parentUuid references existing uuid (if set)
        parent = line.get("parentUuid")
        if parent and parent not in uuids_seen:
            # Parent might be from the "p-001" style synthetic prefix
            if not parent.startswith("p-"):
                errors.append(f"line {i}: parentUuid {parent} not found in prior events")

        # Timestamp monotonicity
        ts = line.get("timestamp")
        if ts and prev_ts and ts < prev_ts:
            errors.append(f"line {i}: timestamp {ts} < previous {prev_ts}")
        if ts:
            prev_ts = ts

        # Type-specific checks
        if line_type in ("user", "assistant"):
            msg = line.get("message")
            if not isinstance(msg, dict):
                errors.append(f"line {i}: {line_type} missing 'message' dict")
            elif "role" not in msg:
                errors.append(f"line {i}: message missing 'role'")

        if line_type == "progress":
            data = line.get("data")
            if not isinstance(data, dict):
                errors.append(f"line {i}: progress missing 'data' dict")
            elif "type" not in data:
                errors.append(f"line {i}: progress.data missing 'type'")

        if line_type == "system":
            if "subtype" not in line:
                errors.append(f"line {i}: system missing 'subtype'")

    return errors


# =====================================================================
# Analyzer — learn profile from real data
# =====================================================================

def analyze_fixtures(fixtures_dir: Path) -> Profile:
    """Analyze real JSONL fixtures and extract a statistical profile.

    This reads the fixture files and computes distributions for:
    - Tool name frequencies
    - Events per session
    - Token counts
    - Timing gaps
    """
    from collections import Counter

    tool_counts: Counter = Counter()
    input_tokens: list[int] = []
    output_tokens: list[int] = []
    timestamps: list[str] = []
    total_events = 0
    session_counts: Counter = Counter()

    for jsonl in sorted(fixtures_dir.glob("*.jsonl")):
        with open(jsonl, "r", encoding="utf-8", errors="replace") as f:
            for raw_line in f:
                raw_line = raw_line.strip()
                if not raw_line:
                    continue
                try:
                    obj = json.loads(raw_line)
                except json.JSONDecodeError:
                    continue

                total_events += 1
                sid = obj.get("sessionId", jsonl.stem)
                session_counts[sid] += 1

                ts = obj.get("timestamp")
                if ts:
                    timestamps.append(ts)

                msg = obj.get("message", {})
                if isinstance(msg, dict):
                    usage = msg.get("usage", {})
                    if isinstance(usage, dict):
                        it = usage.get("input_tokens")
                        ot = usage.get("output_tokens")
                        if isinstance(it, (int, float)) and it > 0:
                            input_tokens.append(int(it))
                        if isinstance(ot, (int, float)) and ot > 0:
                            output_tokens.append(int(ot))

                    content = msg.get("content", [])
                    if isinstance(content, list):
                        for block in content:
                            if isinstance(block, dict) and block.get("type") == "tool_use":
                                tool_counts[block.get("name", "unknown")] += 1

    # Compute tool distribution
    total_tools = sum(tool_counts.values()) or 1
    tool_dist = {name: count / total_tools for name, count in tool_counts.most_common()}

    # Compute token log-normal parameters
    def log_params(values: list[int]) -> tuple[float, float]:
        if not values:
            return (8.0, 1.2)
        logs = [math.log(max(1, v)) for v in values]
        mu = sum(logs) / len(logs)
        var = sum((x - mu) ** 2 for x in logs) / max(1, len(logs) - 1)
        return (mu, math.sqrt(var))

    it_mu, it_std = log_params(input_tokens)
    ot_mu, ot_std = log_params(output_tokens)

    n_sessions = len(session_counts) or 1
    avg_events = total_events / n_sessions

    profile = Profile(
        tool_dist=tool_dist,
        turns_mean=math.log(max(1, avg_events / 15)),  # rough: ~15 events per turn
        turns_std=0.6,
        tools_per_turn_lambda=max(1.0, sum(tool_counts.values()) / max(1, n_sessions * 5)),
        input_tokens_mean=it_mu,
        input_tokens_std=it_std,
        output_tokens_mean=ot_mu,
        output_tokens_std=ot_std,
    )

    print(f"Analyzed {total_events} events across {n_sessions} sessions")
    print(f"  Tool distribution: {dict(tool_counts.most_common(10))}")
    print(f"  Input tokens: mu={it_mu:.1f}, sigma={it_std:.1f}")
    print(f"  Output tokens: mu={ot_mu:.1f}, sigma={ot_std:.1f}")
    print(f"  Avg events/session: {avg_events:.0f}")

    return profile


# =====================================================================
# BDD Tests
# =====================================================================

def run_tests() -> bool:
    """Run BDD-style tests for the generator. Returns True if all pass."""
    passed = 0
    failed = 0
    errors: list[str] = []

    def describe(name: str):
        print(f"\n  {name}")

    def it(name: str, condition: bool, detail: str = ""):
        nonlocal passed, failed
        if condition:
            passed += 1
            print(f"    ok  {name}")
        else:
            failed += 1
            msg = f"    FAIL  {name}"
            if detail:
                msg += f" — {detail}"
            print(msg)
            errors.append(f"{name}: {detail}")

    # -- Profile tests --
    describe("Profile")
    p = make_profile("quick")
    it("quick profile has fewer turns", p.turns_mean < DEFAULT_PROFILE.turns_mean)
    p2 = make_profile("implementation")
    it("implementation has more tools/turn", p2.tools_per_turn_lambda > p.tools_per_turn_lambda)

    # -- Determinism tests --
    describe("Determinism")
    s1 = generate_session(DEFAULT_PROFILE, seed=99)
    s2 = generate_session(DEFAULT_PROFILE, seed=99)
    it("same seed produces identical output",
       json.dumps(s1) == json.dumps(s2))
    s3 = generate_session(DEFAULT_PROFILE, seed=100)
    it("different seed produces different output",
       json.dumps(s1) != json.dumps(s3))

    # -- Structural validation --
    describe("Structural validity")
    for personality in PERSONALITIES:
        lines = generate_session(make_profile(personality), seed=42)
        errs = validate_session(lines)
        it(f"{personality}: no structural errors",
           len(errs) == 0,
           "; ".join(errs[:3]))

    # -- Event type coverage --
    describe("Event type coverage")
    lines = generate_session(make_profile("implementation"), seed=42, min_events=100)
    types_seen = {l.get("type") for l in lines}
    it("has user events", "user" in types_seen)
    it("has assistant events", "assistant" in types_seen)
    it("has progress events", "progress" in types_seen)
    it("has system events", "system" in types_seen)

    # Subtypes
    subtypes_seen: set[str] = set()
    for l in lines:
        if l.get("type") == "system":
            subtypes_seen.add(l.get("subtype", ""))
        elif l.get("type") == "progress":
            d = l.get("data", {})
            if isinstance(d, dict):
                subtypes_seen.add(d.get("type", ""))
    it("has turn_duration system events", "turn_duration" in subtypes_seen)
    it("has bash_progress events", "bash_progress" in subtypes_seen)

    # -- Tree structure --
    describe("Tree structure")
    uuids = {l.get("uuid") for l in lines if l.get("uuid")}
    with_parent = [l for l in lines if l.get("parentUuid") and l.get("parentUuid") in uuids]
    it("has parent-child links", len(with_parent) > 0, f"found {len(with_parent)}")

    # Compute max depth
    parent_map = {l.get("uuid"): l.get("parentUuid") for l in lines if l.get("uuid")}
    def depth(uid):
        d = 0
        while uid and uid in parent_map and parent_map[uid]:
            uid = parent_map[uid]
            d += 1
            if d > 1000:
                break
        return d
    max_depth = max((depth(uid) for uid in parent_map), default=0)
    it("tree depth >= 2", max_depth >= 2, f"max_depth={max_depth}")

    # -- Tool call / result pairing --
    describe("Tool call/result pairing")
    tool_use_ids: set[str] = set()
    tool_result_ids: set[str] = set()
    for l in lines:
        msg = l.get("message", {})
        if isinstance(msg, dict):
            content = msg.get("content", [])
            if isinstance(content, list):
                for block in content:
                    if isinstance(block, dict):
                        if block.get("type") == "tool_use":
                            tool_use_ids.add(block.get("id", ""))
                        elif block.get("type") == "tool_result":
                            tool_result_ids.add(block.get("tool_use_id", ""))
    matched = tool_use_ids & tool_result_ids
    it("tool_use IDs have matching tool_results",
       len(matched) == len(tool_use_ids),
       f"{len(matched)}/{len(tool_use_ids)} matched")

    # -- Timestamp monotonicity --
    describe("Timestamp ordering")
    timestamps = [l.get("timestamp") for l in lines if l.get("timestamp")]
    sorted_ts = sorted(timestamps)
    it("timestamps are monotonically increasing",
       timestamps == sorted_ts)

    # -- Min event counts for test fixtures --
    describe("Fixture generation targets")
    fixtures = generate_test_fixtures(Path("/tmp/synth_test"), seed=42)
    it("synth_origin >= 200 events",
       fixtures.get("synth_origin.jsonl", 0) >= 200,
       f"got {fixtures.get('synth_origin.jsonl', 0)}")
    it("synth_hooks >= 300 events",
       fixtures.get("synth_hooks.jsonl", 0) >= 300,
       f"got {fixtures.get('synth_hooks.jsonl', 0)}")
    it("synth_global >= 700 events",
       fixtures.get("synth_global.jsonl", 0) >= 700,
       f"got {fixtures.get('synth_global.jsonl', 0)}")

    # -- Agent progress with agent_id --
    describe("Agent identity")
    agent_progress = [l for l in lines
                      if l.get("type") == "progress"
                      and isinstance(l.get("data"), dict)
                      and l["data"].get("type") == "agent_progress"]
    has_agent_id = [l for l in agent_progress if l.get("agentId")]
    it("agent_progress events have agentId",
       len(agent_progress) == 0 or len(has_agent_id) > 0,
       f"{len(has_agent_id)}/{len(agent_progress)}")

    # -- No personal data --
    describe("Privacy")
    full_json = json.dumps(lines)
    it("no 'msgla' in output", "msgla" not in full_json)
    it("no personal paths", "C:\\Users\\" not in full_json)
    it("no real session IDs", "personal-inventory" not in full_json)

    # Summary
    total = passed + failed
    print(f"\n  {passed}/{total} passed")
    if failed:
        print(f"  FAILURES:")
        for e in errors:
            print(f"    - {e}")
    return failed == 0


# =====================================================================
# CLI
# =====================================================================

def main():
    parser = argparse.ArgumentParser(
        description="Generate synthetic Claude Code transcript data",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )
    sub = parser.add_subparsers(dest="command")

    # --test
    sub.add_parser("test", help="Run BDD tests")

    # --analyze
    p_analyze = sub.add_parser("analyze", help="Learn profile from real fixture data")
    p_analyze.add_argument("--dir", type=Path,
                           help="Directory containing JSONL fixtures")

    # --generate
    p_gen = sub.add_parser("generate", help="Generate synthetic fixtures")
    p_gen.add_argument("--output-dir", "-o", type=Path,
                       default=Path("rs/tests/fixtures"),
                       help="Output directory (default: rs/tests/fixtures)")
    p_gen.add_argument("--seed", type=int, default=42, help="RNG seed")
    p_gen.add_argument("--personality", choices=list(PERSONALITIES.keys()),
                       help="Session personality (default: mixed)")
    p_gen.add_argument("--count", type=int, default=1,
                       help="Number of sessions (for single personality)")
    p_gen.add_argument("--min-events", type=int, default=100,
                       help="Minimum events per session")
    p_gen.add_argument("--fixtures", action="store_true",
                       help="Generate the full test fixture set")

    # --validate
    p_val = sub.add_parser("validate", help="Validate a JSONL file")
    p_val.add_argument("file", type=Path)

    # Legacy flag support
    parser.add_argument("--test", action="store_true", help="Run BDD tests")
    parser.add_argument("--analyze", action="store_true", help="Analyze fixtures")
    parser.add_argument("--generate", action="store_true", help="Generate fixtures")

    args = parser.parse_args()

    # Handle legacy flags
    if args.test or args.command == "test":
        print("Synthetic Transcript Generator — BDD Tests")
        print("=" * 50)
        success = run_tests()
        sys.exit(0 if success else 1)

    elif args.analyze or args.command == "analyze":
        fixtures_dir = getattr(args, "dir", None) or Path("rs/tests/fixtures")
        profile = analyze_fixtures(fixtures_dir)
        print(f"\nProfile: {profile}")

    elif args.generate or args.command == "generate":
        if getattr(args, "fixtures", False):
            output_dir = getattr(args, "output_dir", Path("rs/tests/fixtures"))
            seed = getattr(args, "seed", 42)
            results = generate_test_fixtures(output_dir, seed=seed)
            print(f"Generated {len(results)} fixture files:")
            for name, count in results.items():
                print(f"  {name}: {count} events")
        else:
            personality = getattr(args, "personality", None) or "implementation"
            seed = getattr(args, "seed", 42)
            count = getattr(args, "count", 1)
            min_events = getattr(args, "min_events", 100)

            for i in range(count):
                profile = make_profile(personality)
                lines = generate_session(profile, seed=seed + i, min_events=min_events)
                if count == 1:
                    for line in lines:
                        print(json.dumps(line, separators=(",", ":")))
                else:
                    sid = lines[0].get("sessionId", f"session-{i}")
                    out_path = Path(getattr(args, "output_dir", ".")) / f"{sid}.jsonl"
                    _write_jsonl(out_path, lines)
                    print(f"  {out_path}: {len(lines)} events")

    elif args.command == "validate":
        lines = []
        with open(args.file, "r", encoding="utf-8") as f:
            for raw_line in f:
                raw_line = raw_line.strip()
                if raw_line:
                    lines.append(json.loads(raw_line))
        errs = validate_session(lines)
        if errs:
            print(f"INVALID: {len(errs)} errors:")
            for e in errs:
                print(f"  - {e}")
            sys.exit(1)
        else:
            print(f"VALID: {len(lines)} events, no structural errors")

    else:
        parser.print_help()


if __name__ == "__main__":
    main()
