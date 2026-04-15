# Eval-Apply Walk

Audit walk #7 — `rs/patterns/src/eval_apply.rs`. 756 lines, 5 inline
tests before this commit. The state machine that turns CloudEvent
streams into `StructuralTurn`s, which then drive sentence detection
and the Story view.

This file is the largest single piece of behavior in the codebase
that wasn't previously walked.

## Findings

### F-1 (real bug, characterized) — Tool result → call pairing is FIFO, ignores call_id

`eval_apply.rs:240-280`. When a `message.user.tool_result` event
arrives, the code resolves it against `pending_applies.first().clone()`
and then `pending_applies.remove(0)` — **regardless of the
`tool_call_id` on either side**.

```rust
"message.user.tool_result" => {
    if let Some(pending) = acc.pending_applies.first().cloned() {
        acc.pending_applies.remove(0);
        // pair output_summary + tool_outcome to this pending
        ...
    }
}
```

For sequential tool use (one call → one result → next call) this is
fine. **For parallel tool use** — pi-mono's bundled `[toolCall,
toolCall]` decomposing into two `assistant.tool_use` events followed
by two `tool_result` events — the matching is correct only if
results arrive in the same order as calls.

Real LLM/runtime can deliver parallel results in completion order, not
call order. Fast tool finishes first, slow tool finishes second.
Today the fast tool's outcome attaches to the first call (which was
the slow one), and vice versa. Silent data corruption in
`StructuralTurn.applies[].tool_outcome`, which then flows into
`SentenceDetector` and the rendered Story.

**Both sides have the call_id available:**
- `assistant.tool_use` events: `agent_payload.tool_use_id` (Claude Code) or `tool_call_id` (pi-mono, Hermes)
- `tool_result` events: `agent_payload.tool_call_id` (pi-mono, Hermes), or in `data.raw.message.content[].tool_use_id` (Claude Code)

The fix is a pairing-by-id lookup. ~30 LOC change to the tool_result branch.

**Test status:** characterization test added —
`parallel_tool_results_out_of_call_order_currently_misattribute`. It
asserts the broken behavior so a fix that switches to call_id
matching **flips this test red** and the developer deletes it.
Companion test `parallel_tool_results_in_call_order_pair_correctly`
documents the baseline — that one stays green under any fix.

**Why not fix now:** real refactor of the tool_result branch + extending the
Accumulator to track call_id alongside `pending_applies` entries.
Touches the eval-apply state machine, which is the heart of the
patterns crate. Deserves its own focused commit; the audit's job is
naming the bug.

### F-2 (medium, characterized) — text + tool_use in same turn loses text content

`eval_apply.rs:282-336`. The `message.assistant.*` branch sets
`acc.pending_eval = Some(EvalOutput { content, ... })`. If a turn
emits both `message.assistant.text` AND `message.assistant.tool_use`
(the pi-mono decomposition shape), the second event overwrites the
first's content. Whichever arrives last wins.

In practice the tool_use event often has empty text, so the
narrative content from `assistant.text` ("I'll check the directory
first") is silently lost from the rendered turn.

**Test:** `assistant_text_then_tool_use_overwrites_pending_eval_content`.
Asserts the empty-content state.

**Fix shape:** accumulate text content rather than overwrite —
`pending_eval.content = format!("{old}{new}")` or push into a
`Vec<String>` and join. Smaller change than F-1 but interacts with
the `decision` field (`text_only` vs `tool_use`).

### F-3 (info, characterized) — system.compact emits a pattern but doesn't decrement env_size

`eval_apply.rs:410-420`. `system.compact` produces a "compact"
PatternEvent for the timeline, but the accumulator's `env_size`
counter doesn't shrink. A turn after compaction reports
pre-compact env_size + N, not post-compact + N.

The downstream effect: the turn-end pattern's "env: N messages"
metadata is wrong after compaction, until the accumulator is
naturally reset by something else. SentenceDetector reads env_size
to bias sentence templates — wrong env_size → slightly wrong
narration.

**Test:** `system_compact_does_not_decrement_env_size`. Locks today's
behavior in.

### F-4 (info) — turn_complete with no prior events makes an empty terminal turn

Documented edge case — `turn_complete_ce` fired without any
preceding human/eval/applies produces a `StructuralTurn` with all
`None` content, `is_terminal = true`, `turn_number = 1`. Doesn't
crash; doesn't filter. Worth knowing the SentenceDetector handles
"empty turns" gracefully (it does — they just produce no sentence).

**Test:** `turn_complete_with_no_prior_events_emits_empty_terminal_turn`.

### F-5 (test gap, FIXED) — `summarize_tool_input` was 50 LOC of pure helper with zero direct tests

7 boundary-table tests added — file path extraction (Read/Write/Edit),
Bash truncation at 80 chars, pattern extraction (Grep/Glob), query
extraction (WebSearch), URL extraction (WebFetch), description
extraction (Agent), JSON fallback for unknown tools, graceful empty
handling.

A typo in any tool name's field extraction (e.g., `"file_path"` →
`"filepath"`) would silently empty-string the summary and not fail
any prior test. Now it would fail one specific test by name.

## Tests added

```
parallel_tool_results_in_call_order_pair_correctly
parallel_tool_results_out_of_call_order_currently_misattribute
assistant_text_then_tool_use_overwrites_pending_eval_content
system_compact_does_not_decrement_env_size
turn_complete_with_no_prior_events_emits_empty_terminal_turn
summarize_tool_input_extracts_file_path_for_file_tools
summarize_tool_input_truncates_long_bash_commands
summarize_tool_input_extracts_pattern_for_search_tools
summarize_tool_input_extracts_query_for_websearch_and_url_for_webfetch
summarize_tool_input_extracts_description_for_agent
summarize_tool_input_falls_back_to_json_for_unknown_tools
summarize_tool_input_handles_missing_fields_gracefully
```

12 new tests, 17 total in eval_apply.rs (was 5). All green on first run.

## Backlog entries warranted

1. **Pair tool_result to pending_apply by call_id.** Real bug. Affects
   pi-mono parallel tool outcomes silently. ~30 LOC + extending
   `PendingApply` to carry call_id. Test `parallel_tool_results_out_of_call_order_currently_misattribute` flips green → red on fix; delete it then.

2. **Accumulate text content across multi-event turns.** The
   text-then-tool-use overwrite drops narrative. Smaller fix than F-1.

3. **Update env_size on system.compact.** Not strictly a bug — env_size
   semantics aren't documented to track post-compact state — but the
   "env: N messages" metric in the turn-end pattern is misleading
   after compaction.

## Pattern, seven walks in

Hit rate still 100%. This walk's haul is the largest yet — one real
bug (F-1), two medium-impact contracts captured (F-2, F-3), and ~50
LOC of pure helpers locked in (F-5). The audit discipline of
"characterize first, fix on a focused branch" is paying off as a
pattern.
