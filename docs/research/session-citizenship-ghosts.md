# Session citizenship: ghosts (Live without Explore)

**Date:** 2026-07-17  
**Branch origin:** self-reflective Grok loop on OpenStory MCP  
**Status:** diagnosed; bus reconnect fix + `scripts/session_citizenship.py`

## What we saw

| Chart | Observation |
|-------|-------------|
| Disk (`~/.grok/sessions/.../019f71cd-…/updates.jsonl`) | Growing continuously (10MB+, tens of k phase edges) |
| Watcher `/api/watchers` | `last_processed_path` = that `updates.jsonl`; `publish` phase `success=true` |
| Store / MCP `session_synopsis` | **not found**; `events` table count **0** for the UUID |
| UI | Could still Story a *fossil* Grok session; not the live ghost |

Soul reading (philosophy.md): **Live** (stream on disk + NATS publish) and **Explore** (SQLite atom) must not be silently merged. Here they diverged — the human’s sovereignty over the story failed for this co-creator unless they grepped JSONL.

## Root cause

From `/tmp/nats.log` during serve boot backfill:

```text
Slow Consumer Detected: MaxPending of 67108864 Exceeded
Slow Consumer Detected: WriteDeadline of 10s exceeded
```

Timeline:

1. Grok watcher backfills ~3k events in large batches into JetStream.
2. Persist consumer’s push subscription falls behind (`mpsc` + SQLite).
3. NATS disconnects the slow push consumer.
4. `spawn_consumer`’s task did `while let Some(Ok(msg)) = messages.next()` and **exited forever** when the stream ended.
5. Watcher kept publishing (JetStream ack green → diagnostics happy).
6. Nothing drained the bus into the store → **ghost**.

## Fix

`rs/bus/src/nats_bus.rs`:

- Reconnect loop with exponential backoff when delivery ends.
- `DeliverPolicy::All` + event-id PK dedup → redelivery safe.
- Larger `mpsc` (2048) and `max_ack_pending: 4096` to absorb short stalls.

## Tooling

```bash
python3 scripts/session_citizenship.py
python3 scripts/session_citizenship.py --session 019f71cd-6bc2-7340-b633-3d2aecc507d2
python3 scripts/session_citizenship.py --test
```

Verdicts: `citizen` | `ghost` | `orphan-store` | `absent`.

## Follow-ups

- Durable named consumers (resume from last ack) instead of ephemeral + All.
- Watcher backfill rate-limit as belt-and-suspenders.
- Surface “ghost risk” on `/api/health` when watcher `cloud_events_emitted` climbs but store session count is flat.
- After deploy: restart `open-story serve` so reconnect code is live; re-check citizenship for the ghost UUID.
