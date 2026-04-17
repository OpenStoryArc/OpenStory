# Bus Walk

Audit walk #6 — `rs/bus/`. Three files, 378 lines total. The
architectural seam between event producers and consumers.

## Findings

### F-1 (real concern, NatsBus) — Silent message loss on deserialize failure

`nats_bus.rs:138-153` — the subscribe spawn loop:

```rust
while let Some(Ok(msg)) = messages.next().await {
    match serde_json::from_slice::<IngestBatch>(&msg.payload) {
        Ok(batch) => { /* forward via tx */ }
        Err(e) => {
            eprintln!("bus: failed to deserialize IngestBatch: {e}");
        }
    }
    // Acknowledge the message — UNCONDITIONALLY
    if let Err(e) = msg.ack().await {
        eprintln!("bus: failed to ack message: {e}");
    }
}
```

The `msg.ack().await` runs **whether deserialization succeeded or not**. So if a producer publishes a malformed payload, the subscriber:
1. Logs the deserialize error to stderr
2. Acks the message anyway → JetStream considers it delivered
3. Replay won't surface it
4. The data is unrecoverable

This is the worst flavor of silent failure: the producer's `publish().await` returned `Ok(())`, the broker stored the message, the subscriber consumed it, the broker freed it — and nobody ever processed it.

**Fix shape:** ack only on successful processing. On deserialize error: `nak()` instead, or better — publish the malformed payload to a dead-letter subject (`events.dead_letter.{original_subject}`) so it's preserved for inspection.

**Why not fix now:** the schema registry capstone tests (cloud_event dogfood + view_record dogfood) would surface real deserialize failures end-to-end as soon as they happened. So the silent-loss footgun is real but the schema work has put a tripwire in front of it. Filing as a follow-up.

### F-2 (design concern) — Subscriber disconnect leaves NATS consumer dangling

`nats_bus.rs:141-143` — `if tx.send(batch).await.is_err() { break; }`. When the receiver side is dropped, the spawn task exits. The JetStream consumer it created (line 121-128) is never explicitly cleaned up — JetStream's broker-side inactivity GC eventually reaps it (default ~5 minutes).

In production, subscribers live the lifetime of the server, so this isn't usually hit. The case it bites: tests that subscribe + drop in quick succession leave dangling consumers on the broker for the test container's lifetime.

**Fix shape:** explicit `consumer.delete().await` on subscriber drop. Requires holding the consumer handle in the spawned task and a Drop hook on `BusSubscription`. Small change, but more ceremony than the audit cycle warrants.

### F-3 (limitation) — Replay is best-effort and time-bounded

`nats_bus.rs:159-213` — replay reads up to `info.state.messages` (the count at start) with a 5-second timeout. Two implications:

- **Race:** new messages arriving during replay are missed (count was frozen at start)
- **Timeout:** if the broker is slow to deliver, replay returns whatever it got after 5s

Documented in code? No. The function signature implies "give me everything matching this pattern" — the actual behavior is "give me at-most-N matching this pattern, in at-most-5s." Worth a doc comment + a defaults constant.

### F-4 (mismatch) — Subscribe `DeliverPolicy::New` vs Replay `DeliverPolicy::All`

`nats_bus.rs:124` and `:176`. This is intentional — subscribe is "live tail," replay is "from-beginning." The split keeps live consumers from getting drowned by historical events on connect. But the contract isn't called out anywhere — a reader could reasonably assume `subscribe()` lets them see history.

**Fix:** doc comment on the trait method clarifying. One line.

### F-5 (test gap, FIXED for NoopBus) — Bus implementations had thin test coverage

`NoopBus` had two tests. Most of its contract (subscribe-returns-never-receiving, replay-returns-empty, is_active-false) was undocumented and untested. Test code that depends on these behaviors had no way to know they were guaranteed.

**Fixed this commit:** four new tests pin every `NoopBus` method's contract. The pattern is the same as projection.rs walk #5 — pure behavior of a heavily-used utility, locked in.

`NatsBus` still has zero unit tests because all its methods need a real broker. Coverage there comes from `rs/bus/tests/nats_integration.rs` (testcontainer-driven). That's appropriate.

## Tests added

```
noop_bus_publish_succeeds_silently
noop_bus_subscribe_returns_never_receiving_channel
noop_bus_replay_returns_empty_vec
noop_bus_is_active_returns_false
```

7 lib tests in bus crate, all green.

## Pattern, seven walks in

Six of the seven walks (data.raw, subagent, watcher, hooks,
projection, bus + the earlier reader/T4) found at least one
test gap or untested pure function. The bus walk is the one
where the most interesting findings (F-1 silent loss, F-2 dangling
consumer) need integration testing infra to land — captured in
this doc as targeted follow-ups rather than fixed inline.
