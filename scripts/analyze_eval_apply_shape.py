#!/usr/bin/env python3
"""Analyze the eval-apply shape of sessions — main agents vs subagents.

Discovers the recursive structure of agent sessions:
- Main agents have explicit turn boundaries (system.turn.complete)
- Subagents have the SAME eval-apply cycle pattern but NO turn boundaries
- Both follow: Human → (Eval → Apply* → Result*)* → Eval(final)

The cycle boundary is deterministic:
  - An Eval followed by Apply = start of a new eval-apply cycle
  - An Eval followed by Eval or EOF = terminal (agent chose text, no tools)

Usage:
  uv run python scripts/analyze_eval_apply_shape.py
  uv run python scripts/analyze_eval_apply_shape.py --session agent-a6dcf911fa2a142b1
  uv run python scripts/analyze_eval_apply_shape.py --test
"""

import argparse
import json
import sys
import urllib.request
from dataclasses import dataclass

API_BASE = "http://localhost:3002/api"


@dataclass
class EvalApplyCycle:
    """One eval-apply cycle: the model evaluated, then optionally applied tools."""
    cycle_number: int
    eval_text: str          # what the model said
    tool_calls: list        # list of (tool_name, input_summary)
    is_terminal: bool       # True if no tool calls (model chose to stop)


@dataclass
class SessionShape:
    """The structural shape of a session's eval-apply unfolding."""
    session_id: str
    is_agent: bool
    total_records: int
    prompts: int
    evals: int
    tool_calls: int
    tool_results: int
    turn_ends: int
    cycles: list            # list of EvalApplyCycle
    symbol_sequence: str    # H E A R T $ symbols


def fetch_records(session_id: str) -> list:
    """Fetch records from the OpenStory API."""
    resp = urllib.request.urlopen(f"{API_BASE}/sessions/{session_id}/records")
    return json.loads(resp.read())


def analyze_session(session_id: str) -> SessionShape:
    """Analyze one session's eval-apply structure."""
    records = fetch_records(session_id)
    is_agent = session_id.startswith("agent-")

    # Count record types
    counts = {}
    for r in records:
        rt = r.get("record_type", "?")
        counts[rt] = counts.get(rt, 0) + 1

    # Build symbol sequence
    symbols = []
    for r in records:
        rt = r.get("record_type", "?")
        if rt == "user_message": symbols.append("H")
        elif rt == "assistant_message": symbols.append("E")
        elif rt == "tool_call": symbols.append("A")
        elif rt == "tool_result": symbols.append("R")
        elif rt == "turn_end": symbols.append("T")
        elif rt == "token_usage": symbols.append("$")

    # Extract eval-apply cycles
    cycles = []
    current_eval = None
    current_tools = []
    cycle_num = 0

    for r in records:
        rt = r.get("record_type", "?")

        if rt == "assistant_message":
            # If we had a previous eval with tools, finalize that cycle
            if current_eval is not None:
                cycle_num += 1
                cycles.append(EvalApplyCycle(
                    cycle_number=cycle_num,
                    eval_text=current_eval,
                    tool_calls=current_tools,
                    is_terminal=len(current_tools) == 0,
                ))
                current_tools = []

            # Start new cycle
            payload = r.get("payload", {})
            text = ""
            if isinstance(payload, dict):
                blocks = payload.get("content", [])
                if isinstance(blocks, list):
                    for b in blocks:
                        if isinstance(b, dict) and b.get("type") == "text":
                            text = b.get("text", "")
                            break
                elif isinstance(blocks, str):
                    text = blocks
            current_eval = text[:100]

        elif rt == "tool_call":
            payload = r.get("payload", {})
            name = payload.get("name", "?") if isinstance(payload, dict) else "?"
            current_tools.append(name)

    # Finalize last cycle
    if current_eval is not None:
        cycle_num += 1
        cycles.append(EvalApplyCycle(
            cycle_number=cycle_num,
            eval_text=current_eval,
            tool_calls=current_tools,
            is_terminal=len(current_tools) == 0,
        ))

    return SessionShape(
        session_id=session_id,
        is_agent=is_agent,
        total_records=len(records),
        prompts=counts.get("user_message", 0),
        evals=counts.get("assistant_message", 0),
        tool_calls=counts.get("tool_call", 0),
        tool_results=counts.get("tool_result", 0),
        turn_ends=counts.get("turn_end", 0),
        cycles=cycles,
        symbol_sequence="".join(symbols),
    )


def print_shape(shape: SessionShape, verbose: bool = False):
    """Print the structural analysis of a session."""
    stype = "SUBAGENT" if shape.is_agent else "MAIN"
    print(f"\n{'='*70}")
    print(f"{stype}: {shape.session_id}")
    print(f"{'='*70}")
    print(f"Records: {shape.total_records}  Prompts: {shape.prompts}  "
          f"Evals: {shape.evals}  Tools: {shape.tool_calls}  "
          f"Results: {shape.tool_results}  Turn boundaries: {shape.turn_ends}")
    print(f"Eval-apply cycles: {len(shape.cycles)}")

    terminal = [c for c in shape.cycles if c.is_terminal]
    with_tools = [c for c in shape.cycles if not c.is_terminal]
    print(f"  with tools: {len(with_tools)}  terminal (text-only): {len(terminal)}")

    if with_tools:
        tools_per_cycle = [len(c.tool_calls) for c in with_tools]
        print(f"  tools per cycle: min={min(tools_per_cycle)} max={max(tools_per_cycle)} "
              f"avg={sum(tools_per_cycle)/len(tools_per_cycle):.1f}")

    if verbose:
        print(f"\nSymbol sequence: {shape.symbol_sequence[:120]}{'...' if len(shape.symbol_sequence) > 120 else ''}")
        print(f"\nCycles:")
        for c in shape.cycles:
            tools_str = ", ".join(c.tool_calls[:5])
            if len(c.tool_calls) > 5:
                tools_str += f" +{len(c.tool_calls)-5} more"
            term = " [TERMINAL]" if c.is_terminal else ""
            print(f"  {c.cycle_number:3d}. {tools_str or '(text only)'}{term}")
            if verbose:
                print(f"       \"{c.eval_text[:70]}{'...' if len(c.eval_text) > 70 else ''}\"")

    # Key finding
    if shape.is_agent and shape.turn_ends == 0 and len(shape.cycles) > 1:
        print(f"\n  ⚠️  {len(shape.cycles)} eval-apply cycles with 0 turn boundaries")
        print(f"      The coalgebra unfolds the same way — cycles are turns without markers")


def run_tests():
    """Self-test: verify structural properties of eval-apply cycles."""
    print("Running tests against live data...\n")

    resp = urllib.request.urlopen(f"{API_BASE}/sessions")
    sessions = json.loads(resp.read())

    passed = 0
    failed = 0

    for s in sessions:
        sid = s["session_id"]
        try:
            shape = analyze_session(sid)
        except Exception as e:
            print(f"  SKIP {sid[:16]}: {e}")
            continue

        is_agent = sid.startswith("agent-")

        # Test 1: every session with events should have at least one cycle
        if shape.total_records > 3:
            if len(shape.cycles) > 0:
                passed += 1
            else:
                failed += 1
                print(f"  FAIL {sid[:16]}: {shape.total_records} records but 0 cycles")

        # Test 2: subagents should have 0 turn_end events
        if is_agent:
            if shape.turn_ends == 0:
                passed += 1
            else:
                failed += 1
                print(f"  FAIL {sid[:16]}: subagent has {shape.turn_ends} turn_ends")

        # Test 3: the last cycle should be terminal (text-only, no tools)
        if shape.cycles:
            last = shape.cycles[-1]
            if last.is_terminal:
                passed += 1
            else:
                failed += 1
                print(f"  FAIL {sid[:16]}: last cycle is not terminal (has {len(last.tool_calls)} tools)")

        # Test 4: main agents should have turn_ends
        if not is_agent and shape.total_records > 10:
            if shape.turn_ends > 0:
                passed += 1
            else:
                failed += 1
                print(f"  FAIL {sid[:16]}: main agent with {shape.total_records} records but 0 turn_ends")

        # Test 5: tool_calls should equal tool_results (every call gets a result)
        if shape.tool_calls == shape.tool_results:
            passed += 1
        else:
            failed += 1
            print(f"  FAIL {sid[:16]}: {shape.tool_calls} calls != {shape.tool_results} results")

        # Test 6: cycles with tools should have at least 1 tool
        for c in shape.cycles:
            if not c.is_terminal and len(c.tool_calls) == 0:
                failed += 1
                print(f"  FAIL {sid[:16]}: cycle {c.cycle_number} is non-terminal but has 0 tools")

    print(f"\n{passed} passed, {failed} failed")
    return failed == 0


def main():
    parser = argparse.ArgumentParser(description="Analyze eval-apply session shapes")
    parser.add_argument("--session", help="Analyze a specific session ID")
    parser.add_argument("--test", action="store_true", help="Run self-tests")
    parser.add_argument("--verbose", "-v", action="store_true", help="Show cycle details")
    parser.add_argument("--all", action="store_true", help="Analyze all sessions")
    args = parser.parse_args()

    if args.test:
        success = run_tests()
        sys.exit(0 if success else 1)

    if args.session:
        shape = analyze_session(args.session)
        print_shape(shape, verbose=args.verbose)
        return

    if args.all:
        resp = urllib.request.urlopen(f"{API_BASE}/sessions")
        sessions = json.loads(resp.read())
        for s in sessions:
            try:
                shape = analyze_session(s["session_id"])
                print_shape(shape, verbose=args.verbose)
            except Exception as e:
                print(f"\nSKIP {s['session_id'][:16]}: {e}")
        return

    # Default: summary table
    resp = urllib.request.urlopen(f"{API_BASE}/sessions")
    sessions = json.loads(resp.read())

    print(f"{'SESSION':<25} {'TYPE':<5} {'REC':>5} {'CYCLES':>6} {'TURNS':>5} {'RATIO':>6}")
    print("-" * 60)

    for s in sessions:
        sid = s["session_id"]
        try:
            shape = analyze_session(sid)
        except:
            continue

        stype = "sub" if shape.is_agent else "main"
        ratio = f"{len(shape.cycles)/shape.turn_ends:.1f}" if shape.turn_ends > 0 else "∞" if shape.cycles else "-"
        label = sid[:22] if shape.is_agent else sid[:8]
        print(f"{label:<25} {stype:<5} {shape.total_records:>5} {len(shape.cycles):>6} {shape.turn_ends:>5} {ratio:>6}")


if __name__ == "__main__":
    main()
