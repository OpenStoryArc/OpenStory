"""Compose multi-turn sessions from stories + action catalog."""

from __future__ import annotations

import uuid
from typing import Iterable

from catalog.actions import (
    CATALOG,
    STORIES,
    ActSpec,
    Story,
    by_id,
    stories_covering,
    tool_input,
    tool_output,
)
from emit.wire import Emitter


def _new_session_id() -> str:
    # UUID-ish, stable shape like real Grok sessions
    return str(uuid.uuid4())


def emit_story(em: Emitter, story: Story, turn_number: int, counter: list[int]) -> None:
    """Emit one user turn as a story (may include multiple tool loops)."""
    acts = by_id()
    prompt_id = str(uuid.uuid4())
    em.turn_start(turn_number)
    em.user(story.motive, prompt_index=turn_number)

    loops = 1
    for act_id in story.act_ids:
        if act_id == "speak.think":
            em.think(story.thought, prompt_id)
            continue
        if act_id == "speak.answer":
            em.say(story.closing, prompt_id)
            continue
        if act_id == "speak.user":
            continue  # already emitted
        spec = acts.get(act_id)
        if not spec:
            continue
        if spec.verb in ("agent_message", "agent_thought", "user_message"):
            continue
        i = counter[0]
        counter[0] += 1
        raw_in = tool_input(spec, i)
        out, err = tool_output(spec, i)
        # Second model loop after tools is common; mark when we apply
        if loops == 1 and act_id.startswith(("explore", "mutate", "execute", "coord", "fail")):
            loops = 2
            em.event("loop_started", loop_index=1)
        em.tool(
            act_id=act_id,
            name=spec.verb,
            raw_input=raw_in,
            output=out,
            is_error=err,
            prompt_id=prompt_id,
            tool_kind=spec.tool_kind,
            read_only=spec.read_only,
        )

    em.turn_end(prompt_id, loop_count=max(1, loops))


def compose_session(
    stories: Iterable[Story],
    *,
    session_id: str | None = None,
    cwd: str = "/workspace/demo",
    t0: int = 1_700_000_000,
) -> Emitter:
    em = Emitter(session_id=session_id or _new_session_id(), cwd=cwd, t0=t0)
    counter = [0]
    for i, story in enumerate(stories):
        emit_story(em, story, turn_number=i, counter=counter)
    return em


def pick_stories_for_gaps(missing: set[str], n: int = 4) -> list[Story]:
    """Greedy: pick up to n stories that cover the most missing acts."""
    remaining = set(missing)
    chosen: list[Story] = []
    ranked = stories_covering(remaining)
    for s in ranked:
        if len(chosen) >= n:
            break
        acts = set(s.act_ids) | set(s.recovery_act_ids)
        if not remaining or acts & remaining or not chosen:
            chosen.append(s)
            remaining -= acts
    # Always include at least one story
    if not chosen:
        chosen = [STORIES[0]]
    return chosen


def default_corpus_stories() -> list[Story]:
    """One of each story — full catalog pass in a single fat session."""
    return list(STORIES)


def catalog_act_ids() -> set[str]:
    return {a.id for a in CATALOG}
