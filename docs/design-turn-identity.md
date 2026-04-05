# Design: Turn Identity & Data Quality

*From data analysis of real sessions, April 2026.*

## What We Found

Analyzing session ca2bc88e (121 turns, 5041 events):

1. **Turns are windows over the event stream.** Each turn is a contiguous subsequence of CloudEvents bounded by `system.turn.complete`. The first event is usually `message.user.prompt`, the last is `system.turn.complete`.

2. **Turns are sequential and non-overlapping.** Turn 1's last event precedes Turn 2's first event in time. Gaps between turns are noise (file_snapshot, queue ops, hooks).

3. **2,348 duplicate event IDs** — events appearing in multiple turns. The accumulator's `event_ids` is not resetting properly between turns. This is a data quality bug.

4. **Turn numbers are synthetic.** They're assigned by the fold function, not by the data. If the fold changes, the numbers change. They're not stable identifiers.

## Turn Identity: Derived from Data

A turn's identity IS the range of events it spans:

```
Turn = { first_event_id, last_event_id }
```

This is:
- **Stable** — derived from the source of truth (CloudEvent UUIDs), not from the fold
- **Unique** — no two turns span the same event range
- **Navigable** — click a turn → filter events to `first_event_id..last_event_id`
- **Verifiable** — you can always check that the events in the range match the turn's content

No synthetic turn IDs needed. The event range IS the identity.

## Start and End Timestamps

Each turn has a natural time range:

```
Turn = {
  start: timestamp of first_event_id,
  end: timestamp of last_event_id,
  duration: end - start,
}
```

This enables:
- **Duration display** on each card (already have `duration_ms` from `system.turn.complete`)
- **Time gaps** between turns (Turn 4 ends at 23:52, Turn 5 starts at 00:41 — 49 minutes gap. The human went away.)
- **Timeline layout** — turns positioned on a time axis, proportional to duration

## Data Quality Issues to Fix

### 1. Duplicate event IDs across turns

The `step()` function's `acc.event_ids` accumulates events but isn't clearing properly between turns. When a turn completes, `event_ids` is moved out via `std::mem::take`, but events from the *next* turn's early events (before the accumulator fully resets) may leak in.

**Fix:** Verify that `std::mem::take(&mut acc.event_ids)` in the `system.turn.complete` branch clears the vector. Check if the `start_ts` reset is also clearing `event_ids` for the next turn.

### 2. Events between turns not captured

Events between `system.turn.complete` and the next `message.user.prompt` (file snapshots, queue ops) are accumulated into the *next* turn's event_ids even though they don't belong to it.

**Fix:** Only start accumulating event_ids after seeing a `message.user.prompt` or `message.assistant.*`. Skip noise events.

### 3. Turn number stability

Turn numbers increment per `system.turn.complete` in the `step()` function. But if events are replayed in a different order, or if the fold function changes, the numbers change. 

**Fix:** Use `{session_id}:{first_event_id}` as the turn's stable ID in SQLite instead of `turn:{session_id}:{turn_number}`.

## Features to Build

### Click-through to events (Priority 1)

Click a turn card → navigate to Live/Explore view filtered to that turn's event range. The sentence diagram links to the raw events that produced it.

Implementation: the turn's `event_ids` are already on the PatternEvent. Add a click handler that navigates to `#/live/{session_id}?from={first_event_id}` or `#/explore/{session_id}/event/{first_event_id}`.

### Turn duration display (Priority 2)

Show `duration_ms` on each card and the gap between turns. "Turn 5: 3.2s · 49min gap". The gaps are where the human was thinking (or away).

### Timeline view (Priority 3)

Turns on a time axis. Each turn is a block proportional to its duration. Gaps are visible as empty space. Click a block to see the turn card. This is the session's rhythm made visual.

## Relationship to the Two Streams Architecture

Stream 1 (CloudEvents) is the linked list. Stream 2 (StructuralTurns) is windows over that list. The turn identity (first_event_id..last_event_id) is the bridge — it says exactly which segment of Stream 1 produced this element of Stream 2.

This is what makes the fold auditable. "Why did the sentence say 'committed'?" → look at the turn → look at the event range → see the raw CloudEvents. The chain from meaning back to fact is traceable.
