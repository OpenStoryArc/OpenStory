"""SVO action catalog for software-agent session synthesis.

Each ActSpec is one *kind* of harness agency we want OpenStory to observe.
Coverage is measured over act ids, not raw tool call counts.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Callable


@dataclass(frozen=True)
class ActSpec:
    """One statistically meaningful software action type."""

    id: str
    verb: str  # tool name or speech act
    kind: str  # explore | mutate | execute | search | coordinate | speak | fail
    subject: str = "grok"
    # Human-readable object pattern (for MANIFEST / sentences)
    object_pattern: str = ""
    # ACP tool meta kind
    tool_kind: str = "other"
    read_only: bool = True
    # Weight for sampling when filling coverage gaps (higher = more common IRL)
    weight: float = 1.0
    # Tags for story composition
    tags: tuple[str, ...] = ()


# ── Catalog: statistically viable range of SE agent acts ─────────────
# Grounded in real Grok histograms (read ≫ bash ≫ edit ≫ grep ≫ list)
# plus deliberate long-tail: write, mcp, background task, plan, error.

CATALOG: list[ActSpec] = [
    # Speech / cognition
    ActSpec("speak.answer", "agent_message", "speak", object_pattern="reply text", tags=("speech",)),
    ActSpec("speak.think", "agent_thought", "speak", object_pattern="reasoning", tags=("speech", "think")),
    ActSpec("speak.user", "user_message", "speak", object_pattern="human prompt", tags=("speech", "motive")),
    # Explore
    ActSpec(
        "explore.list",
        "list_dir",
        "explore",
        object_pattern="directory",
        tool_kind="list",
        weight=2.0,
        tags=("explore", "read"),
    ),
    ActSpec(
        "explore.read",
        "read_file",
        "explore",
        object_pattern="source file",
        tool_kind="read",
        weight=5.0,
        tags=("explore", "read"),
    ),
    ActSpec(
        "explore.grep",
        "grep",
        "search",
        object_pattern="pattern in codebase",
        tool_kind="search",
        weight=3.0,
        tags=("explore", "search"),
    ),
    # Mutate
    ActSpec(
        "mutate.edit",
        "search_replace",
        "mutate",
        object_pattern="file region",
        tool_kind="edit",
        read_only=False,
        weight=3.0,
        tags=("mutate", "write"),
    ),
    ActSpec(
        "mutate.write",
        "write",
        "mutate",
        object_pattern="new or full file",
        tool_kind="edit",
        read_only=False,
        weight=1.5,
        tags=("mutate", "write"),
    ),
    # Execute
    ActSpec(
        "execute.shell",
        "run_terminal_command",
        "execute",
        object_pattern="shell command",
        tool_kind="execute",
        read_only=False,
        weight=4.0,
        tags=("execute", "shell"),
    ),
    ActSpec(
        "execute.test",
        "run_terminal_command",
        "execute",
        object_pattern="test runner",
        tool_kind="execute",
        read_only=False,
        weight=2.5,
        tags=("execute", "test"),
    ),
    ActSpec(
        "execute.git",
        "run_terminal_command",
        "execute",
        object_pattern="git status|diff|log",
        tool_kind="execute",
        read_only=False,
        weight=2.0,
        tags=("execute", "git"),
    ),
    # Coordinate / meta
    ActSpec(
        "coord.todo",
        "todo_write",
        "coordinate",
        object_pattern="task list",
        tool_kind="other",
        read_only=False,
        weight=0.8,
        tags=("coordinate",),
    ),
    ActSpec(
        "coord.mcp_search",
        "search_tool",
        "search",
        object_pattern="MCP tool catalog",
        tool_kind="search",
        weight=0.6,
        tags=("mcp", "search"),
    ),
    ActSpec(
        "coord.mcp_call",
        "use_tool",
        "coordinate",
        object_pattern="MCP server tool",
        tool_kind="other",
        read_only=False,
        weight=1.0,
        tags=("mcp",),
    ),
    ActSpec(
        "coord.background_poll",
        "get_command_or_subagent_output",
        "coordinate",
        object_pattern="background task",
        tool_kind="other",
        weight=0.5,
        tags=("async",),
    ),
    # Failure modes (objects of recovery stories)
    ActSpec(
        "fail.read_missing",
        "read_file",
        "fail",
        object_pattern="missing path",
        tool_kind="read",
        weight=1.2,
        tags=("fail", "recover"),
    ),
    ActSpec(
        "fail.shell_nonzero",
        "run_terminal_command",
        "fail",
        object_pattern="failing command",
        tool_kind="execute",
        read_only=False,
        weight=1.0,
        tags=("fail", "recover", "test"),
    ),
]


def by_id() -> dict[str, ActSpec]:
    return {a.id: a for a in CATALOG}


def ids() -> list[str]:
    return [a.id for a in CATALOG]


# ── Object / I/O templates (deterministic given seed) ────────────────


def tool_input(act: ActSpec, i: int) -> dict[str, Any]:
    """Concrete rawInput for a tool verb."""
    n = i % 7
    if act.id == "explore.list":
        return {"target_directory": f"/workspace/demo/src" if n else "/workspace/demo"}
    if act.id == "explore.read":
        files = [
            "/workspace/demo/Cargo.toml",
            "/workspace/demo/src/main.rs",
            "/workspace/demo/src/lib.rs",
            "/workspace/demo/README.md",
        ]
        return {"target_file": files[n % len(files)]}
    if act.id == "explore.grep":
        return {"pattern": ["TODO", "fn main", "eval_apply", "CloudEvent"][n % 4], "glob": "**/*.{rs,ts,md}"}
    if act.id == "mutate.edit":
        return {
            "file_path": "/workspace/demo/src/main.rs",
            "old_string": "version = \"0.1.0\"",
            "new_string": f"version = \"0.1.{n}\"",
        }
    if act.id == "mutate.write":
        return {
            "file_path": f"/workspace/demo/src/generated_{n}.rs",
            "content": f"// generated act {n}\npub fn f{n}() {{}}\n",
        }
    if act.id == "execute.shell":
        cmds = ["ls -la", "pwd", "wc -l src/main.rs", "echo ok"]
        return {"command": cmds[n % len(cmds)], "description": "shell explore"}
    if act.id == "execute.test":
        return {"command": "cargo test -q", "description": "run unit tests"}
    if act.id == "execute.git":
        cmds = ["git status --short", "git log -1 --oneline", "git diff --stat"]
        return {"command": cmds[n % len(cmds)], "description": "git inspect"}
    if act.id == "coord.todo":
        return {
            "todos": [
                {"id": "1", "content": "explore", "status": "completed"},
                {"id": "2", "content": "implement", "status": "in_progress"},
            ]
        }
    if act.id == "coord.mcp_search":
        return {"query": "openstory session", "limit": 10}
    if act.id == "coord.mcp_call":
        return {
            "tool_name": "openstory__list_sessions",
            "tool_input": {"days": 7, "limit": 5},
        }
    if act.id == "coord.background_poll":
        return {"task_ids": [f"task-{n}"], "timeout_ms": 1000}
    if act.id == "fail.read_missing":
        return {"target_file": "/workspace/demo/does_not_exist.rs"}
    if act.id == "fail.shell_nonzero":
        return {"command": "cargo test -q -- --exact no_such_test", "description": "expected fail"}
    return {"note": act.id}


def tool_output(act: ActSpec, i: int) -> tuple[Any, bool]:
    """(rawOutput-ish content, is_error)."""
    if act.id == "fail.read_missing":
        return ("Error: File not found: /workspace/demo/does_not_exist.rs", True)
    if act.id == "fail.shell_nonzero":
        return ("error: no test named `no_such_test`\n", True)
    if act.id == "explore.read":
        return ("// sample file contents\nfn main() {}\n", False)
    if act.id == "explore.list":
        return ("Cargo.toml\nsrc/\nREADME.md\n", False)
    if act.id == "explore.grep":
        return ("src/main.rs:1:fn main() {\n", False)
    if act.id == "mutate.edit":
        return ("The file /workspace/demo/src/main.rs has been updated.", False)
    if act.id == "mutate.write":
        return (f"Wrote /workspace/demo/src/generated_{i % 7}.rs", False)
    if act.id == "execute.test":
        return ("running 3 tests\nall tests passed\n", False)
    if act.id == "execute.git":
        return (" M src/main.rs\n", False)
    if act.id == "coord.mcp_call":
        return ('[{"id":"sess-1","label":"demo","event_count":12}]', False)
    if act.id == "coord.mcp_search":
        return ('{"results":[{"tool_name":"openstory__list_sessions"}]}', False)
    if act.id == "coord.todo":
        return ("Todos updated", False)
    if act.id == "coord.background_poll":
        return ('{"status":"completed","output":"done"}', False)
    return (f"ok:{act.id}", False)


# ── Stories: compositional grammar over acts ─────────────────────────


@dataclass
class Story:
    """A multi-act narrative unit (one or more eval-apply cycles inside a turn)."""

    id: str
    motive: str  # user-facing prompt fragment
    act_ids: list[str]
    thought: str
    closing: str
    # If True, insert a recovery act after the first fail.* act
    recover: bool = False
    recovery_act_ids: list[str] = field(default_factory=list)


STORIES: list[Story] = [
    Story(
        id="story.text_only",
        motive="What is an eval-apply loop in one sentence?",
        act_ids=["speak.think", "speak.answer"],
        thought="Pure explanation — no tools needed.",
        closing="Eval applies operators to operands; agents eval plans and apply tools.",
    ),
    Story(
        id="story.explore_tree",
        motive="What's in this project?",
        act_ids=["speak.think", "explore.list", "explore.read", "speak.answer"],
        thought="List root then read the manifest.",
        closing="Demo crate with Cargo.toml and a small src tree.",
    ),
    Story(
        id="story.search_then_read",
        motive="Where is main defined?",
        act_ids=["speak.think", "explore.grep", "explore.read", "speak.answer"],
        thought="Grep then open the hit.",
        closing="`fn main` lives in src/main.rs.",
    ),
    Story(
        id="story.edit_and_test",
        motive="Bump the patch version and run tests.",
        act_ids=["speak.think", "mutate.edit", "execute.test", "speak.answer"],
        thought="Edit then cargo test.",
        closing="Version bumped; tests green.",
    ),
    Story(
        id="story.write_file",
        motive="Add a tiny helper module.",
        act_ids=["speak.think", "mutate.write", "explore.read", "speak.answer"],
        thought="Write then verify.",
        closing="Added generated helper and confirmed contents.",
    ),
    Story(
        id="story.git_status",
        motive="What did we change?",
        act_ids=["speak.think", "execute.git", "speak.answer"],
        thought="git status / diff.",
        closing="Working tree shows the expected edits.",
    ),
    Story(
        id="story.fail_recover_read",
        motive="Open missing.rs please.",
        act_ids=["speak.think", "fail.read_missing", "explore.read", "speak.answer"],
        thought="Path may be wrong — try then recover.",
        closing="Missing path; opened src/main.rs instead.",
        recover=True,
        recovery_act_ids=["explore.read"],
    ),
    Story(
        id="story.fail_recover_test",
        motive="Run the flaky test then fix.",
        act_ids=["speak.think", "fail.shell_nonzero", "mutate.edit", "execute.test", "speak.answer"],
        thought="Reproduce failure, patch, retest.",
        closing="Failed once, patched, tests pass.",
        recover=True,
    ),
    Story(
        id="story.mcp_query",
        motive="Can you list sessions via OpenStory MCP?",
        act_ids=["speak.think", "coord.mcp_search", "coord.mcp_call", "speak.answer"],
        thought="Discover then call MCP tool.",
        closing="Listed recent sessions from OpenStory.",
    ),
    Story(
        id="story.plan_and_shell",
        motive="Orient and inspect the environment.",
        act_ids=["speak.think", "coord.todo", "execute.shell", "explore.list", "speak.answer"],
        thought="Todo then shell + list.",
        closing="Environment looks healthy.",
    ),
    Story(
        id="story.async_poll",
        motive="Check that background task.",
        act_ids=["speak.think", "coord.background_poll", "speak.answer"],
        thought="Poll background output.",
        closing="Background task completed.",
    ),
    Story(
        id="story.multi_read_implement",
        motive="Read the entrypoints then make a small edit.",
        act_ids=[
            "speak.think",
            "explore.read",
            "explore.read",
            "explore.grep",
            "mutate.edit",
            "execute.shell",
            "speak.answer",
        ],
        thought="Gather context, edit, verify with shell.",
        closing="Context gathered and edit applied.",
    ),
]


def stories_covering(missing: set[str]) -> list[Story]:
    """Prefer stories that hit the most missing act ids."""
    scored: list[tuple[int, Story]] = []
    for s in STORIES:
        acts = set(s.act_ids) | set(s.recovery_act_ids)
        scored.append((len(acts & missing), s))
    scored.sort(key=lambda x: (-x[0], x[1].id))
    return [s for _, s in scored]
