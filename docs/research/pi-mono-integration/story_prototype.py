"""
story_prototype.py — End-to-end prototype: pi-mono JSONL → sentences → story.

Takes captured pi-mono sessions, runs them through the decomposing translator,
builds eval/apply turns, generates sentence diagrams, and compares the output
to what the SAME data would look like under the OLD translator (single subtype).

Usage:
    cd docs/research/pi-mono-integration
    python story_prototype.py                    # all scenarios
    python story_prototype.py 06-thinking-text-tool  # specific scenario

No external dependencies.
"""

from __future__ import annotations

import json
import os
import sys
from collections import defaultdict
from typing import Dict, List, Optional

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from translate_pi_decompose import translate_file, translate_line

HERE = os.path.dirname(os.path.abspath(__file__))
CAPTURES = os.path.join(HERE, "captures")


# ─── Tool classification (mirrors sentence.rs) ──────────────────────

PREPARATORY = {"Read", "read", "Grep", "Glob", "WebSearch", "WebFetch"}
CREATIVE = {"Write", "Edit"}
DELEGATORY = {"Agent"}
VERIFICATORY_BASH = {"test", "spec", "jest", "vitest", "pytest", "cargo test", "npm test"}


def classify_tool(name: str, args: Optional[Dict] = None) -> str:
    if name in PREPARATORY:
        return "preparatory"
    if name in CREATIVE:
        return "creative"
    if name in DELEGATORY:
        return "delegatory"
    if name == "Bash":
        cmd = json.dumps(args or {}).lower()
        if any(t in cmd for t in VERIFICATORY_BASH):
            return "verificatory"
        return "verificatory"
    return "verificatory"


# ─── Eval/Apply turn construction ────────────────────────────────────

def build_turns(events: List[Dict]) -> List[Dict]:
    """Group CloudEvents into eval/apply turns.

    A turn starts with a user prompt and ends with a terminal assistant
    response (stop_reason=stop) or the end of events.
    """
    turns = []
    current_turn = None

    for e in events:
        subtype = e.get("subtype", "")

        if subtype == "message.user.prompt":
            if current_turn:
                turns.append(current_turn)
            current_turn = {
                "human": e["data"]["agent_payload"].get("text", ""),
                "eval_blocks": [],
                "apply_blocks": [],
                "is_terminal": False,
            }

        elif subtype.startswith("message.assistant.") and current_turn:
            payload = e["data"]["agent_payload"]
            block = {
                "subtype": subtype,
                "text": payload.get("text", ""),
                "tool": payload.get("tool"),
                "tool_call_id": payload.get("tool_call_id"),
                "args": payload.get("args"),
                "stop_reason": payload.get("stop_reason"),
            }
            current_turn["eval_blocks"].append(block)
            if payload.get("stop_reason") == "stop":
                current_turn["is_terminal"] = True

        elif subtype == "message.user.tool_result" and current_turn:
            payload = e["data"]["agent_payload"]
            current_turn["apply_blocks"].append({
                "tool_call_id": payload.get("tool_call_id"),
                "tool_name": payload.get("tool_name"),
                "is_error": payload.get("is_error", False),
                "text": payload.get("text", "")[:100],
            })

    if current_turn:
        turns.append(current_turn)

    return turns


# ─── Sentence generation (mirrors sentence.rs) ──────────────────────

def build_sentence(turn: Dict) -> str:
    """Generate a natural language sentence from a turn."""
    subject = "Pi"
    human = turn["human"]
    eval_blocks = turn["eval_blocks"]
    apply_blocks = turn["apply_blocks"]
    is_terminal = turn["is_terminal"]

    # Count block types
    thinking_blocks = [b for b in eval_blocks if b["subtype"] == "message.assistant.thinking"]
    text_blocks = [b for b in eval_blocks if b["subtype"] == "message.assistant.text"]
    tool_blocks = [b for b in eval_blocks if b["subtype"] == "message.assistant.tool_use"]

    # Classify tools
    tool_roles = defaultdict(list)
    for b in tool_blocks:
        role = classify_tool(b["tool"] or "", b.get("args"))
        tool_roles[role].append(b)

    # Build the sentence
    parts = []

    # Subject
    parts.append(subject)

    # Verb + object based on dominant action
    if tool_blocks and not text_blocks:
        # Pure tool call turn
        tools = [b["tool"] for b in tool_blocks]
        if len(tools) == 1:
            parts.append(f"called {tools[0]}")
        else:
            parts.append(f"called {len(tools)} tools ({', '.join(tools)})")
    elif text_blocks and not tool_blocks:
        # Pure text response
        text = text_blocks[0]["text"]
        if len(text) > 60:
            parts.append(f"responded ({len(text)} chars)")
        else:
            parts.append("responded")
    elif tool_blocks and text_blocks:
        # Mixed: text + tools
        tools = [b["tool"] for b in tool_blocks]
        parts.append(f"explained and called {', '.join(tools)}")
    else:
        parts.append("acted")

    # Thinking subordinate
    if thinking_blocks:
        thinking_len = sum(len(b["text"]) for b in thinking_blocks)
        parts.append(f"(after {thinking_len} chars of reasoning)")

    # Adverbial from human prompt
    if human:
        short = human[:50] + "..." if len(human) > 50 else human
        parts.append(f'when asked "{short}"')

    # Apply results
    if apply_blocks:
        errors = [a for a in apply_blocks if a["is_error"]]
        ok = len(apply_blocks) - len(errors)
        if errors:
            parts.append(f"→ {ok} ok, {len(errors)} errors")
        else:
            parts.append(f"→ {ok} tool results")

    # Predicate
    if is_terminal:
        parts.append("— answered")
    else:
        parts.append("— continued")

    return " ".join(parts)


# ─── Old translator simulation ───────────────────────────────────────

def old_translator_subtypes(jsonl_path: str) -> List[str]:
    """What the OLD translator would produce (single subtype per line)."""
    subtypes = []
    with open(jsonl_path) as f:
        for line_str in f:
            line = json.loads(line_str.strip())
            if line.get("type") != "message":
                continue
            msg = line.get("message", {})
            role = msg.get("role", "")
            if role == "assistant":
                content = msg.get("content", [])
                types = [b.get("type") for b in content]
                if "toolCall" in types:
                    subtypes.append("message.assistant.tool_use")
                elif "thinking" in types:
                    subtypes.append("message.assistant.thinking")
                else:
                    subtypes.append("message.assistant.text")
            elif role == "user":
                subtypes.append("message.user.prompt")
            elif role == "toolResult":
                subtypes.append("message.user.tool_result")
    return subtypes


def old_translator_sentence(jsonl_path: str) -> str:
    """What story the OLD translator tells (no decomposition)."""
    subtypes = old_translator_subtypes(jsonl_path)
    has_text = "message.assistant.text" in subtypes
    has_thinking = "message.assistant.thinking" in subtypes
    has_tool = "message.assistant.tool_use" in subtypes

    parts = ["Pi"]
    if has_tool:
        parts.append("called tools")
    elif has_thinking:
        parts.append("thought")
    elif has_text:
        parts.append("responded")
    else:
        parts.append("did something")

    if not has_text and (has_thinking or has_tool):
        parts.append("(no visible text response)")

    return " ".join(parts)


# ─── Visualization ───────────────────────────────────────────────────

def print_scenario(scenario_name: str):
    jsonl_path = os.path.join(CAPTURES, scenario_name, "session.jsonl")
    if not os.path.isfile(jsonl_path):
        print(f"  {scenario_name}: no session.jsonl found")
        return

    # New translator
    events = translate_file(jsonl_path)
    turns = build_turns(events)

    # Count what's visible
    all_subtypes = [e["subtype"] for e in events]
    assistant_text = [e for e in events if e["subtype"] == "message.assistant.text"]
    assistant_thinking = [e for e in events if e["subtype"] == "message.assistant.thinking"]
    assistant_tools = [e for e in events if e["subtype"] == "message.assistant.tool_use"]

    print(f"\n{'═' * 70}")
    print(f"  {scenario_name}")
    print(f"{'═' * 70}")

    # Show CloudEvent decomposition
    print(f"\n  CloudEvents ({len(events)} total):")
    for e in events:
        sub = e["subtype"]
        text = e["data"]["agent_payload"].get("text", "")
        tool = e["data"]["agent_payload"].get("tool", "")
        if sub == "message.assistant.text":
            preview = text[:60] + "..." if len(text) > 60 else text
            print(f"    💬 {sub}: \"{preview}\"")
        elif sub == "message.assistant.thinking":
            preview = text[:60] + "..." if len(text) > 60 else text
            print(f"    🧠 {sub}: \"{preview}\"")
        elif sub == "message.assistant.tool_use":
            print(f"    🔧 {sub}: {tool}({json.dumps(e['data']['agent_payload'].get('args', {}))})")
        elif sub == "message.user.prompt":
            preview = text[:60] + "..." if len(text) > 60 else text
            print(f"    👤 {sub}: \"{preview}\"")
        elif sub == "message.user.tool_result":
            tn = e["data"]["agent_payload"].get("tool_name", "")
            err = " ERROR" if e["data"]["agent_payload"].get("is_error") else ""
            print(f"    📋 {sub}: {tn}{err}")
        else:
            print(f"    ⚙  {sub}")

    # Show sentences
    print(f"\n  Sentences ({len(turns)} turns):")
    for i, turn in enumerate(turns):
        sentence = build_sentence(turn)
        print(f"    Turn {i+1}: {sentence}")

    # Compare to old translator
    old_sentence = old_translator_sentence(jsonl_path)
    print(f"\n  ┌─ NEW translator story:")
    for i, turn in enumerate(turns):
        print(f"  │  Turn {i+1}: {build_sentence(turn)}")
    print(f"  ├─ OLD translator story:")
    print(f"  │  {old_sentence}")

    # Show what was invisible
    old_subtypes = old_translator_subtypes(jsonl_path)
    old_text_count = old_subtypes.count("message.assistant.text")
    new_text_count = len(assistant_text)
    if new_text_count > old_text_count:
        print(f"  ├─ RECOVERED: {new_text_count - old_text_count} text blocks now visible")
    if assistant_thinking:
        old_thinking = old_subtypes.count("message.assistant.thinking")
        new_thinking = len(assistant_thinking)
        if new_thinking > old_thinking:
            print(f"  ├─ RECOVERED: {new_thinking - old_thinking} thinking blocks now visible")
    old_tool_count = old_subtypes.count("message.assistant.tool_use")
    new_tool_count = len(assistant_tools)
    if new_tool_count > old_tool_count:
        print(f"  ├─ RECOVERED: {new_tool_count - old_tool_count} tool calls now visible")

    print(f"  └─ {'✓ No data loss' if new_text_count == old_text_count and new_tool_count == old_tool_count else '⚠ Data was being lost'}")


# ─── Compare to Claude Code ──────────────────────────────────────────

def print_claude_comparison():
    print(f"\n{'═' * 70}")
    print(f"  COMPARISON: Pi-Mono vs Claude Code sentence structure")
    print(f"{'═' * 70}")
    print("""
  Claude Code (scenario: "read a file and explain it"):
    Turn 1: Claude called Read ("read config.toml") → 1 tool result — continued
    Turn 2: Claude responded (315 chars) when asked "explain it" — answered

  Pi-Mono (scenario 06: "think about the bug, explain, read"):
    Turn 1: Pi explained and called read (after 74 chars of reasoning)
            when asked "Think step by step about what test-broken.py..." → 1 tool results — continued
    Turn 2: Pi responded (650 chars) — answered

  Key structural difference:
    Claude Code Turn 1 has NO text — it's a pure tool call.
    Pi-Mono Turn 1 has thinking + text + tool — it explains WHILE calling.

  This is because pi-mono's model often produces text alongside tool calls
  (e.g., "Let me read the file first, then I can analyze it." + toolCall).
  Claude Code almost never does this — text and tool calls are separate turns.

  The sentence detector handles this correctly once the decomposition is in place.
  "Pi explained and called read" captures the mixed nature of the turn.
  Without decomposition, it would just be "Pi called tools (no visible text response)".
    """)


# ─── Main ─────────────────────────────────────────────────────────────

if __name__ == "__main__":
    scenarios = sys.argv[1:] if len(sys.argv) > 1 else sorted([
        d for d in os.listdir(CAPTURES)
        if os.path.isdir(os.path.join(CAPTURES, d)) and d.startswith("0")
    ])

    print()
    print("Pi-Mono Story Prototype — End-to-End")
    print("JSONL → Decompose → CloudEvents → Turns → Sentences")
    print()

    for s in scenarios:
        print_scenario(s)

    print_claude_comparison()

    # Final summary
    print(f"\n{'═' * 70}")
    print(f"  SUMMARY")
    print(f"{'═' * 70}")
    total_recovered = 0
    for s in scenarios:
        jsonl_path = os.path.join(CAPTURES, s, "session.jsonl")
        if not os.path.isfile(jsonl_path):
            continue
        events = translate_file(jsonl_path)
        old = old_translator_subtypes(jsonl_path)
        new_text = len([e for e in events if e["subtype"] == "message.assistant.text"])
        old_text = old.count("message.assistant.text")
        recovered = new_text - old_text
        total_recovered += recovered

    print(f"  Text blocks recovered across all scenarios: {total_recovered}")
    print(f"  These are now searchable, visible in UI, and available to the sentence detector.")
    print()
