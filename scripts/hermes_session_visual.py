#!/usr/bin/env python3
"""
hermes_session_visual.py — Visualize a Hermes Agent session's data flow.

Shows how Hermes's eval-apply loop works internally: what the agent sees,
how it stores state, when the session log rewrites, and how each message
maps to a CloudEvent after translation.

Usage:
    python3 scripts/hermes_session_visual.py ~/.hermes/sessions/session_20260410_112248_a150ad.json
    python3 scripts/hermes_session_visual.py ~/.hermes/sessions/session_20260410_112248_a150ad.json --html > session.html
    open session.html
"""

from __future__ import annotations

import argparse
import json
import os
import sys
from typing import Any, Dict, List


# ── Colors for terminal output ──────────────────────────────────────────

class C:
    RESET = "\033[0m"
    BOLD = "\033[1m"
    DIM = "\033[2m"

    BLUE = "\033[34m"
    GREEN = "\033[32m"
    YELLOW = "\033[33m"
    RED = "\033[31m"
    CYAN = "\033[36m"
    MAGENTA = "\033[35m"
    WHITE = "\033[37m"

    BG_DARK = "\033[48;5;235m"
    BG_BLUE = "\033[48;5;17m"
    BG_GREEN = "\033[48;5;22m"
    BG_YELLOW = "\033[48;5;58m"
    BG_RED = "\033[48;5;52m"


# ── Terminal visualization ──────────────────────────────────────────────


def render_terminal(session: Dict[str, Any]) -> None:
    messages = session.get("messages", [])
    model = session.get("model", "?")
    sid = session.get("session_id", "?")
    platform = session.get("platform", "?")

    print()
    print(f"{C.BOLD}{C.CYAN}╔══════════════════════════════════════════════════════════════╗{C.RESET}")
    print(f"{C.BOLD}{C.CYAN}║  Hermes Agent Session — Data Flow Visualization             ║{C.RESET}")
    print(f"{C.BOLD}{C.CYAN}╚══════════════════════════════════════════════════════════════╝{C.RESET}")
    print()
    print(f"  {C.DIM}session_id:{C.RESET}  {sid}")
    print(f"  {C.DIM}model:{C.RESET}       {model}")
    print(f"  {C.DIM}platform:{C.RESET}    {platform}")
    print(f"  {C.DIM}messages:{C.RESET}    {len(messages)}")
    print()

    # ── Persistence model comparison ──
    print(f"{C.BOLD}{C.YELLOW}┌─ How Hermes stores this session ─────────────────────────────┐{C.RESET}")
    print(f"{C.YELLOW}│{C.RESET}")
    print(f"{C.YELLOW}│{C.RESET}  {C.BOLD}Claude Code:{C.RESET} append-only JSONL")
    print(f"{C.YELLOW}│{C.RESET}    Each event → one line appended to ~/.claude/projects/*.jsonl")
    print(f"{C.YELLOW}│{C.RESET}    File grows monotonically. Watcher reads from byte offset N.")
    print(f"{C.YELLOW}│{C.RESET}")
    print(f"{C.YELLOW}│{C.RESET}  {C.BOLD}Hermes Agent:{C.RESET} snapshot rewrite")
    print(f"{C.YELLOW}│{C.RESET}    After EACH turn → entire session rewritten to")
    print(f"{C.YELLOW}│{C.RESET}    ~/.hermes/sessions/session_{sid}.json")
    print(f"{C.YELLOW}│{C.RESET}    The file is a {C.BOLD}snapshot{C.RESET}, not a log. It reflects the")
    print(f"{C.YELLOW}│{C.RESET}    {C.BOLD}latest state{C.RESET} of _session_messages at every moment.")
    print(f"{C.YELLOW}│{C.RESET}")
    print(f"{C.YELLOW}│{C.RESET}  {C.BOLD}The bridge:{C.RESET} hermes-openstory plugin")
    print(f"{C.YELLOW}│{C.RESET}    Hooks post_llm_call + post_tool_call → emits append-only")
    print(f"{C.YELLOW}│{C.RESET}    JSONL to ~/.hermes/openstory-events/{sid}.jsonl")
    print(f"{C.YELLOW}│{C.RESET}    OpenStory's watcher reads this, not the snapshot.")
    print(f"{C.YELLOW}└──────────────────────────────────────────────────────────────┘{C.RESET}")
    print()

    # ── The eval-apply loop ──
    print(f"{C.BOLD}{C.CYAN}┌─ The Eval-Apply Loop ────────────────────────────────────────┐{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}  {C.DIM}Hermes's agent loop (run_agent.py) follows this cycle:{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}    ┌──────────┐    ┌──────────┐    ┌──────────┐")
    print(f"{C.CYAN}│{C.RESET}    │  {C.GREEN}EVAL{C.RESET}    │───▶│  {C.YELLOW}APPLY{C.RESET}   │───▶│  {C.GREEN}EVAL{C.RESET}    │───▶ ...")
    print(f"{C.CYAN}│{C.RESET}    │ LLM call │    │ tool run │    │ LLM call │")
    print(f"{C.CYAN}│{C.RESET}    └──────────┘    └──────────┘    └──────────┘")
    print(f"{C.CYAN}│{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}  {C.DIM}Each box produces a message added to _session_messages.{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}  {C.DIM}After each EVAL, _save_session_log() rewrites the JSON.{C.RESET}")
    print(f"{C.CYAN}│{C.RESET}  {C.DIM}The loop ends when finish_reason == 'stop' (no more tools).{C.RESET}")
    print(f"{C.CYAN}└──────────────────────────────────────────────────────────────┘{C.RESET}")
    print()

    # ── Message-by-message walkthrough ──
    print(f"{C.BOLD}{C.MAGENTA}┌─ Message Flow ───────────────────────────────────────────────┐{C.RESET}")
    print(f"{C.MAGENTA}│{C.RESET}")

    snapshot_count = 0
    tool_call_map = {}  # id → name, for linking

    for i, msg in enumerate(messages):
        role = msg.get("role", "?")

        if role == "user":
            content = msg.get("content", "")
            print(f"{C.MAGENTA}│{C.RESET}  {C.BOLD}{C.BLUE}[{i}] USER PROMPT{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}role: user | keys: {sorted(msg.keys())}{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.BLUE}\"{content[:100]}{'...' if len(content) > 100 else ''}\"{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ _session_messages.append(msg){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: pre_llm_call(user_message=..., conversation_history=[...{i+1} msgs]){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}")

        elif role == "assistant":
            tc = msg.get("tool_calls", [])
            reasoning = msg.get("reasoning")
            finish = msg.get("finish_reason", "?")
            content = msg.get("content", "")

            phase = "EVAL → tool dispatch" if tc else "EVAL → final answer"
            color = C.GREEN

            print(f"{C.MAGENTA}│{C.RESET}  {C.BOLD}{color}[{i}] ASSISTANT — {phase}{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}role: assistant | finish_reason: {finish} | keys: {sorted(msg.keys())}{C.RESET}")

            if reasoning:
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}reasoning: \"{reasoning[:80]}...\"{C.RESET}")
            elif "reasoning" in msg:
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}reasoning: null (Sonnet 4 default — no extended thinking){C.RESET}")

            if content:
                print(f"{C.MAGENTA}│{C.RESET}  {color}\"{content[:100]}{'...' if len(content) > 100 else ''}\"{C.RESET}")

            if tc:
                for j, t in enumerate(tc):
                    fn = t.get("function", {})
                    name = fn.get("name", "?")
                    args = fn.get("arguments", "{}")
                    tid = t.get("id", "?")
                    tool_call_map[tid] = name

                    # Show extra Anthropic SDK fields
                    extra_keys = [k for k in t.keys() if k not in ("id", "function")]
                    extra = f" + SDK fields: {extra_keys}" if extra_keys else ""

                    if isinstance(args, str) and len(args) > 80:
                        args_display = args[:77] + "..."
                    else:
                        args_display = args

                    print(f"{C.MAGENTA}│{C.RESET}  {C.YELLOW}  tool_calls[{j}]: {name}({args_display}){C.RESET}")
                    print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}    id: {tid}{extra}{C.RESET}")

                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ messages.append(msg)  [OpenAI shape: tool_calls array]{C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: post_llm_call(assistant_response=msg, conversation_history=[...{i+1} msgs]){C.RESET}")
                snapshot_count += 1
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}→ _persist_session():  [21 exit points call this]{C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    1. _session_messages = messages         (in-memory update){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    2. _save_session_log(messages)           (REWRITE #{snapshot_count}: atomic_json_write){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    3. _flush_messages_to_session_db()        (append new msgs to SQLite){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ loop continues: dispatch tool calls...{C.RESET}")
            else:
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ messages.append(msg){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: post_llm_call(assistant_response=msg){C.RESET}")
                snapshot_count += 1
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}→ _persist_session():  [final exit path]{C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    1. _session_messages = messages         (in-memory update){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    2. _save_session_log(messages)           (REWRITE #{snapshot_count}: atomic_json_write){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.RED}    3. _flush_messages_to_session_db()        (append new msgs to SQLite){C.RESET}")
                print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ finish_reason='stop': loop ends{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}")

        elif role == "tool":
            tid = msg.get("tool_call_id", "?")
            tool_name = tool_call_map.get(tid, msg.get("tool_name", "?"))
            content = msg.get("content", "")

            # Parse the content JSON to show structure
            is_error = False
            content_summary = ""
            try:
                parsed = json.loads(content)
                if isinstance(parsed, dict):
                    keys = list(parsed.keys())
                    if "error" in parsed and parsed["error"]:
                        is_error = True
                        content_summary = f"ERROR: {parsed['error']}"
                    elif "output" in parsed:
                        content_summary = f"output: {str(parsed['output'])[:60]}"
                    elif "content" in parsed:
                        content_summary = f"content: {str(parsed['content'])[:60]}"
                    elif "success" in parsed:
                        content_summary = f"success: {parsed['success']}"
                    elif "bytes_written" in parsed:
                        content_summary = f"bytes_written: {parsed['bytes_written']}"
                    else:
                        content_summary = f"keys: {keys}"
            except json.JSONDecodeError:
                content_summary = content[:60]

            color = C.RED if is_error else C.YELLOW
            error_label = " [ERROR]" if is_error else ""

            print(f"{C.MAGENTA}│{C.RESET}  {C.BOLD}{color}[{i}] TOOL RESULT — {tool_name}{error_label}{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}role: tool | tool_call_id: {tid} | keys: {sorted(msg.keys())}{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}NOTE: tool_name field {'PRESENT' if 'tool_name' in msg else 'ABSENT'} (Hermes never sets it){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {color}{content_summary}{C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ content is ALWAYS a JSON string (not plain text){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ _session_messages.append(msg){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: post_tool_call(tool_name='{tool_name}', result=content, tool_call_id='{tid[:20]}...'){C.RESET}")
            print(f"{C.MAGENTA}│{C.RESET}")

    print(f"{C.MAGENTA}│{C.RESET}")
    print(f"{C.MAGENTA}│{C.RESET}  {C.BOLD}Session complete.{C.RESET}")
    print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: on_session_end(completed=True, interrupted=False){C.RESET}")
    print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}→ hook: on_session_finalize(session_id='{sid}'){C.RESET}")
    print(f"{C.MAGENTA}│{C.RESET}  {C.DIM}Total snapshots written: {snapshot_count} (file rewritten {snapshot_count} times){C.RESET}")
    print(f"{C.MAGENTA}└──────────────────────────────────────────────────────────────┘{C.RESET}")

    # ── What OpenStory sees ──
    print()
    print(f"{C.BOLD}{C.GREEN}┌─ What OpenStory Sees (after translation) ────────────────────┐{C.RESET}")
    print(f"{C.GREEN}│{C.RESET}")

    ce_count = 0
    for i, msg in enumerate(messages):
        role = msg.get("role", "?")
        if role == "system":
            continue

        tc = msg.get("tool_calls", []) if role == "assistant" else []
        reasoning = msg.get("reasoning") if role == "assistant" else None

        if reasoning:
            ce_count += 1
            print(f"{C.GREEN}│{C.RESET}  CE{ce_count}: {C.CYAN}message.assistant.thinking{C.RESET}  reasoning={reasoning[:40]}...")

        if tc:
            for t in tc:
                ce_count += 1
                name = t.get("function", {}).get("name", "?")
                print(f"{C.GREEN}│{C.RESET}  CE{ce_count}: {C.YELLOW}message.assistant.tool_use{C.RESET}  tool={name}  id={t.get('id','?')[:20]}")
        elif role == "assistant":
            ce_count += 1
            print(f"{C.GREEN}│{C.RESET}  CE{ce_count}: {C.GREEN}message.assistant.text{C.RESET}     stop_reason={msg.get('finish_reason','?')}")
        elif role == "user":
            ce_count += 1
            content = msg.get("content", "")
            print(f"{C.GREEN}│{C.RESET}  CE{ce_count}: {C.BLUE}message.user.prompt{C.RESET}       \"{content[:50]}...\"")
        elif role == "tool":
            ce_count += 1
            tid = msg.get("tool_call_id", "?")
            print(f"{C.GREEN}│{C.RESET}  CE{ce_count}: {C.YELLOW}message.user.tool_result{C.RESET}   call_id={tid[:20]}")

    print(f"{C.GREEN}│{C.RESET}")
    print(f"{C.GREEN}│{C.RESET}  {C.DIM}{len(messages)} Hermes messages → {ce_count} CloudEvents{C.RESET}")
    print(f"{C.GREEN}│{C.RESET}  {C.DIM}Fan-out: assistant+tool_calls → 1 CE per tool call{C.RESET}")
    print(f"{C.GREEN}│{C.RESET}  {C.DIM}All events carry: agent='hermes', _variant='hermes'{C.RESET}")
    print(f"{C.GREEN}│{C.RESET}  {C.DIM}IDs: deterministic uuid5(session_id, seq, subtype){C.RESET}")
    print(f"{C.GREEN}└──────────────────────────────────────────────────────────────┘{C.RESET}")
    print()


# ── HTML visualization ──────────────────────────────────────────────────


def render_html(session: Dict[str, Any]) -> str:
    messages = session.get("messages", [])
    model = session.get("model", "?")
    sid = session.get("session_id", "?")
    platform = session.get("platform", "?")
    start = session.get("session_start", "?")
    updated = session.get("last_updated", "?")
    tools = session.get("tools", [])
    tool_names = []
    for t in tools:
        if isinstance(t, dict) and "function" in t:
            tool_names.append(t["function"]["name"])
        elif isinstance(t, str):
            tool_names.append(t)

    tool_call_map = {}

    def msg_html(i: int, msg: dict) -> str:
        role = msg.get("role", "?")
        tc = msg.get("tool_calls", []) if role == "assistant" else []
        reasoning = msg.get("reasoning")
        finish = msg.get("finish_reason", "")
        content = msg.get("content", "")
        tid = msg.get("tool_call_id", "")

        # Map tool calls
        for t in tc:
            tool_call_map[t.get("id", "")] = t.get("function", {}).get("name", "?")

        if role == "user":
            return f"""
            <div class="msg user">
                <div class="msg-header">
                    <span class="badge badge-user">USER</span>
                    <span class="msg-index">[{i}]</span>
                    <span class="phase-label">→ pre_llm_call fires</span>
                </div>
                <div class="msg-content">{_esc(content[:300])}</div>
                <div class="msg-meta">keys: {sorted(msg.keys())}</div>
            </div>"""

        elif role == "assistant" and tc:
            tc_html = ""
            for j, t in enumerate(tc):
                fn = t.get("function", {})
                name = fn.get("name", "?")
                args = fn.get("arguments", "{}")
                if isinstance(args, str) and len(args) > 120:
                    args = args[:117] + "..."
                extra = [k for k in t.keys() if k not in ("id", "function")]
                tc_html += f"""
                <div class="tool-call">
                    <span class="tool-name">{_esc(name)}</span>
                    <span class="tool-args">{_esc(str(args))}</span>
                    <div class="tool-meta">id: {t.get('id','')} {f'| extra SDK fields: {extra}' if extra else ''}</div>
                </div>"""

            reasoning_html = ""
            if reasoning:
                reasoning_html = f'<div class="reasoning">reasoning: "{_esc(reasoning[:120])}..."</div>'
            elif "reasoning" in msg:
                reasoning_html = '<div class="reasoning dim">reasoning: null (no extended thinking)</div>'

            return f"""
            <div class="msg assistant tool-dispatch">
                <div class="msg-header">
                    <span class="badge badge-eval">EVAL</span>
                    <span class="msg-index">[{i}]</span>
                    <span class="badge badge-dispatch">→ TOOL DISPATCH</span>
                    <span class="phase-label">finish_reason: {finish}</span>
                </div>
                {reasoning_html}
                {f'<div class="msg-content">{_esc(content[:200])}</div>' if content else ''}
                <div class="tool-calls">{tc_html}</div>
                <div class="snapshot">⟳ _save_session_log() — snapshot rewrite</div>
                <div class="msg-meta">→ post_llm_call fires → loop dispatches tool calls</div>
            </div>"""

        elif role == "assistant":
            reasoning_html = ""
            if reasoning:
                reasoning_html = f'<div class="reasoning">reasoning: "{_esc(reasoning[:120])}..."</div>'

            return f"""
            <div class="msg assistant final">
                <div class="msg-header">
                    <span class="badge badge-eval">EVAL</span>
                    <span class="msg-index">[{i}]</span>
                    <span class="badge badge-final">→ FINAL ANSWER</span>
                    <span class="phase-label">finish_reason: {finish}</span>
                </div>
                {reasoning_html}
                <div class="msg-content">{_esc(content[:500])}</div>
                <div class="snapshot">⟳ _save_session_log() — snapshot rewrite</div>
                <div class="msg-meta">→ post_llm_call fires → loop ends (stop)</div>
            </div>"""

        elif role == "tool":
            tool_name = tool_call_map.get(tid, "?")
            is_error = False
            content_display = content[:300]
            try:
                parsed = json.loads(content)
                if isinstance(parsed, dict):
                    if "error" in parsed and parsed["error"]:
                        is_error = True
                    content_display = json.dumps(parsed, indent=2)[:400]
            except:
                pass

            error_class = " error" if is_error else ""
            return f"""
            <div class="msg tool{error_class}">
                <div class="msg-header">
                    <span class="badge badge-apply">APPLY</span>
                    <span class="msg-index">[{i}]</span>
                    <span class="tool-result-name">{_esc(tool_name)}</span>
                    {'<span class="badge badge-error">ERROR</span>' if is_error else ''}
                    <span class="phase-label">→ post_tool_call fires</span>
                </div>
                <div class="msg-meta">tool_call_id: {tid} | tool_name field: {'PRESENT' if 'tool_name' in msg else 'ABSENT'}</div>
                <pre class="tool-output">{_esc(content_display)}</pre>
                <div class="msg-meta dim">content is ALWAYS a JSON string wrapping structured output</div>
            </div>"""

        return ""

    msgs_html = "\n".join(msg_html(i, m) for i, m in enumerate(messages))

    # Count CloudEvents
    ce_count = 0
    for m in messages:
        if m.get("role") == "system": continue
        if m.get("role") == "assistant":
            if m.get("reasoning"): ce_count += 1
            tc = m.get("tool_calls", [])
            if tc:
                ce_count += len(tc)
            else:
                ce_count += 1
        else:
            ce_count += 1

    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>Hermes Session — {sid}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{ background: #1a1b26; color: #c0caf5; font-family: 'JetBrains Mono', 'Fira Code', monospace; font-size: 13px; padding: 24px; max-width: 900px; margin: 0 auto; }}
  h1 {{ color: #7aa2f7; font-size: 18px; margin-bottom: 8px; }}
  h2 {{ color: #bb9af7; font-size: 15px; margin: 24px 0 12px; border-bottom: 1px solid #3b4261; padding-bottom: 4px; }}
  .meta {{ color: #565f89; margin-bottom: 16px; }}
  .meta span {{ margin-right: 16px; }}

  .comparison {{ display: grid; grid-template-columns: 1fr 1fr; gap: 16px; margin: 16px 0; }}
  .comparison > div {{ background: #24283b; border-radius: 8px; padding: 12px; border: 1px solid #3b4261; }}
  .comparison h3 {{ font-size: 12px; color: #7aa2f7; margin-bottom: 8px; }}
  .comparison p {{ font-size: 11px; color: #9aa5ce; line-height: 1.5; }}
  .comparison .highlight {{ color: #f7768e; font-weight: bold; }}

  .loop-diagram {{ background: #24283b; border-radius: 8px; padding: 16px; margin: 16px 0; border: 1px solid #3b4261; text-align: center; }}
  .loop-boxes {{ display: flex; align-items: center; justify-content: center; gap: 8px; margin: 12px 0; flex-wrap: wrap; }}
  .loop-box {{ padding: 8px 16px; border-radius: 6px; font-weight: bold; font-size: 12px; }}
  .loop-box.eval {{ background: #1a3a1a; color: #9ece6a; border: 1px solid #9ece6a44; }}
  .loop-box.apply {{ background: #3a3a1a; color: #e0af68; border: 1px solid #e0af6844; }}
  .loop-arrow {{ color: #3b4261; font-size: 18px; }}

  .msg {{ background: #24283b; border-radius: 8px; padding: 12px; margin: 8px 0; border-left: 3px solid #3b4261; }}
  .msg.user {{ border-left-color: #7aa2f7; }}
  .msg.assistant.tool-dispatch {{ border-left-color: #e0af68; }}
  .msg.assistant.final {{ border-left-color: #9ece6a; }}
  .msg.tool {{ border-left-color: #bb9af7; }}
  .msg.tool.error {{ border-left-color: #f7768e; }}

  .msg-header {{ display: flex; align-items: center; gap: 8px; margin-bottom: 6px; flex-wrap: wrap; }}
  .msg-index {{ color: #565f89; font-size: 11px; }}
  .badge {{ padding: 2px 6px; border-radius: 4px; font-size: 10px; font-weight: bold; }}
  .badge-user {{ background: #7aa2f720; color: #7aa2f7; }}
  .badge-eval {{ background: #9ece6a20; color: #9ece6a; }}
  .badge-apply {{ background: #bb9af720; color: #bb9af7; }}
  .badge-dispatch {{ background: #e0af6820; color: #e0af68; }}
  .badge-final {{ background: #9ece6a20; color: #9ece6a; }}
  .badge-error {{ background: #f7768e20; color: #f7768e; }}
  .phase-label {{ color: #565f89; font-size: 10px; }}

  .msg-content {{ color: #c0caf5; margin: 6px 0; line-height: 1.5; white-space: pre-wrap; word-break: break-word; }}
  .msg-meta {{ color: #565f89; font-size: 10px; margin-top: 4px; }}
  .msg-meta.dim {{ opacity: 0.6; }}
  .reasoning {{ color: #bb9af7; font-size: 11px; margin: 4px 0; opacity: 0.7; }}
  .reasoning.dim {{ opacity: 0.4; }}

  .tool-calls {{ margin: 8px 0; }}
  .tool-call {{ background: #1a1b26; border-radius: 6px; padding: 8px; margin: 4px 0; border: 1px solid #3b4261; }}
  .tool-name {{ color: #e0af68; font-weight: bold; }}
  .tool-args {{ color: #9aa5ce; font-size: 11px; display: block; margin-top: 4px; word-break: break-all; }}
  .tool-meta {{ color: #565f89; font-size: 10px; margin-top: 4px; }}
  .tool-result-name {{ color: #bb9af7; font-weight: bold; }}
  .tool-output {{ background: #1a1b26; border-radius: 4px; padding: 8px; margin: 6px 0; font-size: 11px; color: #9aa5ce; overflow-x: auto; white-space: pre-wrap; word-break: break-word; max-height: 200px; overflow-y: auto; }}

  .snapshot {{ color: #f7768e; font-size: 11px; margin: 6px 0; padding: 4px 8px; background: #f7768e10; border-radius: 4px; border: 1px solid #f7768e22; }}

  .summary {{ background: #24283b; border-radius: 8px; padding: 16px; margin: 16px 0; border: 1px solid #9ece6a33; }}
  .summary h3 {{ color: #9ece6a; margin-bottom: 8px; }}
  .summary .stat {{ display: inline-block; margin-right: 16px; color: #c0caf5; }}
  .summary .stat-label {{ color: #565f89; }}
</style>
</head>
<body>

<h1>☤ Hermes Agent Session — Data Flow</h1>
<div class="meta">
    <span>session: {sid}</span>
    <span>model: {model}</span>
    <span>platform: {platform}</span>
    <span>messages: {len(messages)}</span>
</div>

<h2>Persistence Model</h2>
<div class="comparison">
    <div>
        <h3>Claude Code — append-only JSONL</h3>
        <p>Each event → one line appended to <code>~/.claude/projects/*.jsonl</code></p>
        <p>File grows monotonically. Watcher reads from byte offset N.</p>
        <p>Each line is independent. Order = append order.</p>
    </div>
    <div>
        <h3>Hermes Agent — <span class="highlight">snapshot rewrite</span></h3>
        <p>After EACH turn → entire session rewritten to <code>session_{sid}.json</code></p>
        <p>The file is a <span class="highlight">snapshot</span>, not a log.</p>
        <p>Contains: envelope + full messages array in OpenAI shape.</p>
        <p>The <strong>plugin bridge</strong> converts to append-only JSONL for OpenStory.</p>
    </div>
</div>

<h2>The Eval-Apply Loop — Precise Write Operations</h2>
<div class="loop-diagram" style="text-align: left;">
    <p style="color: #c0caf5; margin-bottom: 12px; font-weight: bold;">What happens inside run_agent.py on every turn:</p>

    <div style="margin: 12px 0;">
        <div class="loop-boxes">
            <div class="loop-box eval">EVAL<br><small>LLM call</small></div>
            <span class="loop-arrow">→</span>
            <div class="loop-box apply">APPLY<br><small>tool run</small></div>
            <span class="loop-arrow">→</span>
            <div class="loop-box eval">EVAL<br><small>LLM call</small></div>
            <span class="loop-arrow">→</span>
            <div class="loop-box apply">APPLY<br><small>tool run</small></div>
            <span class="loop-arrow">→</span>
            <div class="loop-box eval" style="border-color: #9ece6a;">EVAL<br><small>final</small></div>
        </div>
    </div>

    <p style="color: #9ece6a; font-weight: bold; margin: 16px 0 8px;">After EVERY EVAL (assistant response):</p>
    <pre style="background: #1a1b26; padding: 12px; border-radius: 6px; font-size: 11px; color: #c0caf5; line-height: 1.6; text-align: left;">
<span style="color: #565f89;">// run_agent.py:5666-5678 — build the message dict</span>
msg = {{
    "role": "assistant",
    "content": response.content,
    <span style="color: #bb9af7;">"reasoning": extracted_reasoning,</span>  <span style="color: #565f89;">// top-level string, not content blocks</span>
    "finish_reason": "tool_calls" | "stop",
    <span style="color: #e0af68;">"tool_calls": [{{id, function: {{name, arguments}}}}],</span>  <span style="color: #565f89;">// OpenAI shape always</span>
}}

<span style="color: #565f89;">// run_agent.py:8906 — append to the in-memory list</span>
messages.append(assistant_msg)

<span style="color: #f7768e; font-weight: bold;">// run_agent.py:1874-1876 — _persist_session() fires</span>
<span style="color: #f7768e;">self._session_messages = messages              </span><span style="color: #565f89;">// 1. update reference</span>
<span style="color: #f7768e;">self._save_session_log(messages)                </span><span style="color: #565f89;">// 2. REWRITE entire JSON file (atomic_json_write)</span>
<span style="color: #f7768e;">self._flush_messages_to_session_db(messages)    </span><span style="color: #565f89;">// 3. append NEW messages to SQLite</span></pre>

    <p style="color: #e0af68; font-weight: bold; margin: 16px 0 8px;">After EVERY APPLY (tool execution):</p>
    <pre style="background: #1a1b26; padding: 12px; border-radius: 6px; font-size: 11px; color: #c0caf5; line-height: 1.6; text-align: left;">
<span style="color: #565f89;">// model_tools.py:532 — hook fires with result</span>
invoke_hook("post_tool_call",
    tool_name="read_file", args={{...}},
    <span style="color: #e0af68;">result='{{\"content\": \"...\", \"error\": null}}',</span>  <span style="color: #565f89;">// always JSON string</span>
    tool_call_id="toolu_01...", session_id="...")

<span style="color: #565f89;">// agent loop appends tool result to messages</span>
messages.append({{"role": "tool", "tool_call_id": "toolu_01...", "content": result}})</pre>

    <p style="color: #f7768e; font-weight: bold; margin: 16px 0 8px;">The critical insight — _persist_session() is called at 21 exit points:</p>
    <pre style="background: #1a1b26; padding: 12px; border-radius: 6px; font-size: 11px; color: #9aa5ce; line-height: 1.6; text-align: left;">
<span style="color: #f7768e;">✓</span> After each successful LLM response (eval complete)
<span style="color: #f7768e;">✓</span> After all tool results are collected (apply complete)
<span style="color: #f7768e;">✓</span> On user interrupt (Ctrl+C mid-stream)
<span style="color: #f7768e;">✓</span> On API error (after retries exhausted)
<span style="color: #f7768e;">✓</span> On context overflow (before compression)
<span style="color: #f7768e;">✓</span> On max iterations reached
<span style="color: #f7768e;">✓</span> On session end (final answer)
<span style="color: #f7768e;">✓</span> On any exception in the loop

<span style="color: #565f89;">Every call does the same 3 things:</span>
  1. _session_messages = messages       <span style="color: #565f89;">← in-memory update</span>
  2. _save_session_log(messages)         <span style="color: #f7768e;">← FULL FILE REWRITE</span>
  3. _flush_messages_to_session_db()     <span style="color: #e0af68;">← incremental SQLite append</span></pre>

    <p style="color: #bb9af7; font-weight: bold; margin: 16px 0 8px;">When context gets too big — compression:</p>
    <pre style="background: #1a1b26; padding: 12px; border-radius: 6px; font-size: 11px; color: #c0caf5; line-height: 1.6; text-align: left;">
<span style="color: #565f89;">// run_agent.py:5949-6007 — _compress_context()</span>
<span style="color: #565f89;">// Triggered when tokens > threshold (default: 50% of context window)</span>

<span style="color: #bb9af7;">1. flush_memories(messages)           </span><span style="color: #565f89;">// save memories before they're lost</span>
<span style="color: #bb9af7;">2. compressor.compress(messages)      </span><span style="color: #565f89;">// summarize middle turns, keep recent</span>
<span style="color: #f7768e;">3. session_db.end_session(old_id)     </span><span style="color: #565f89;">// mark old session as "compression"</span>
<span style="color: #9ece6a;">4. self.session_id = new_id           </span><span style="color: #565f89;">// NEW session ID generated</span>
<span style="color: #9ece6a;">5. session_db.create_session(new_id)  </span><span style="color: #565f89;">// new SQLite row, linked via parent_session_id</span>
<span style="color: #f7768e;">6. session_log_file = new_path        </span><span style="color: #565f89;">// NEW JSON file (old one preserved)</span>

<span style="color: #565f89;">Result: the conversation is now SPLIT across two session IDs.</span>
<span style="color: #565f89;">Old session: kept on disk, marked "compression".</span>
<span style="color: #565f89;">New session: starts with compressed summary + recent messages.</span>
<span style="color: #565f89;">The old JSON file is NOT deleted — it's a checkpoint.</span></pre>

    <p style="color: #565f89; font-size: 11px; margin-top: 12px;">
        <strong>Answer to "does Hermes delete state?":</strong> No. It <em>overwrites</em> the JSON file (atomic replace, not delete+write) and <em>appends</em> to SQLite. On compression, the old session is preserved and a new one starts. Nothing is deleted — state accumulates across session splits.
    </p>
</div>

<h2>Message Flow — {len(messages)} messages</h2>
{msgs_html}

<div class="summary">
    <h3>What OpenStory Sees</h3>
    <div>
        <span class="stat">{len(messages)} messages</span>
        <span class="stat">→ {ce_count} CloudEvents</span>
        <span class="stat-label">(fan-out: 1 CE per tool call)</span>
    </div>
    <p style="margin-top: 8px; color: #565f89; font-size: 11px;">
        All events carry: agent="hermes", _variant="hermes" | IDs: deterministic uuid5(session_id, seq, subtype)
    </p>
</div>

<div style="margin-top: 24px; color: #3b4261; font-size: 10px; text-align: center;">
    Generated by hermes_session_visual.py — OpenStory ×  Hermes Agent integration
</div>

</body>
</html>"""


def _esc(s: str) -> str:
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")


# ── CLI ─────────────────────────────────────────────────────────────────


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Visualize a Hermes Agent session's data flow."
    )
    parser.add_argument("session_file", help="Path to session_*.json")
    parser.add_argument("--html", action="store_true", help="Output HTML instead of terminal")
    args = parser.parse_args()

    data = json.load(open(args.session_file))

    if args.html:
        print(render_html(data))
    else:
        render_terminal(data)

    return 0


if __name__ == "__main__":
    sys.exit(main())
