# The Three `is_ephemeral` Functions

Discovered while walking the patterns consumer during the audit's
continuation. The codebase has three functions named `is_ephemeral`
that mean slightly different things.

## The three

| # | Location | Signature | Returns true for |
|---|----------|-----------|-------------------|
| 1 | `rs/core/src/subtype.rs:178` | `Subtype::is_ephemeral(&self) -> bool` | `progress.*` |
| 2 | `rs/store/src/projection.rs:398` | `pub fn is_ephemeral(subtype: Option<&str>) -> bool` | `progress.*` |
| 3 | `rs/server/src/consumers/patterns.rs:139` | `fn is_ephemeral(subtype: &str) -> bool` | `progress.*`, `system.hook`, `queue.enqueue`, `queue.dequeue`, `queue.remove`, `queue.popAll`, `file.snapshot` |

## Why they diverge

#1 and #2 classify "ephemeral" as the lifecycle axis: **not durably
stored**. The only current members are `progress.*` events (streaming
tool output, subagent progress, hook progress). They show in the UI
transiently and nothing persists them.

#3 classifies "ephemeral" as the pattern-detection axis: **do not
feed into eval-apply**. The set is larger — it includes metadata
events (`file.snapshot`), queue lifecycle events, and `system.hook`
— because those don't contribute to the structural turn shape AND
some of them lack a stable `event.time` that would corrupt turn
start timestamps. The comment at `consumers/patterns.rs:125-138`
explains the `file.snapshot` case in detail.

## Why it matters

**Short version:** the name is wrong in patterns.rs. The predicate
there isn't about ephemerality — it's about pattern-detection
eligibility. Two different questions with the same name is a latent
bug magnet.

**Longer version:** someone later adding a new subtype has to pick
which list to update. If they assume "ephemeral" is one concept
(because there's a `Subtype::is_ephemeral` method now), they'll
update only the one that matches the struct method's meaning. The
pattern-detection filter silently drifts out of sync with reality.

The risk is real: the subtype dogfood test surfaced
`queue.remove` (142 occurrences) and `queue.popAll` (19) as subtypes
the `Subtype` enum didn't know about. Now they're added to the enum,
and because the patterns.rs list was also maintained by hand, those
two subtypes happen to be in it correctly. But nothing enforces that.

## Why #1 and #2 still coexist

They're the same semantics but different signatures:
- `#1` takes `&Subtype`
- `#2` takes `Option<&str>`

`#2` predates the `Subtype` enum. The Subtype refactor on this branch
did not touch call sites — that was the Path B decision documented in
`SCHEMA_MAP.md` and the Subtype commit. So `ingest_events` at
`ingest.rs:182` still calls `projection::is_ephemeral(Option<&str>)`
rather than parsing the string into a `Subtype` first.

No bug here, just ownership drift waiting to happen when someone
adds the next subtype.

## Fixes, ranked

### A. Rename the pattern-detection filter — smallest

`consumers/patterns.rs::is_ephemeral` → `should_skip_pattern_detection`
(or `skip_for_eval_apply`). Mechanical rename. Comment updated.
Tests updated. No behavior change.

### B. Consolidate #1 and #2 — medium

`projection::is_ephemeral(Option<&str>)` becomes a thin wrapper over
`Subtype::from_str(s).map(|s| s.is_ephemeral()).unwrap_or(false)`.
Source of truth is the enum. If an unknown subtype string appears
(mid-migration, unknown agent), it's treated as durable — the safe
fallback.

Slightly more work because `ingest_events:182` passes `Option<&str>`
from a serde_json::Value, not a `Subtype`. A helper keeps the call
site ergonomics the same.

### C. Both — recommended

Fix B surfaces the source of truth (Subtype). Fix A removes the
naming confusion at the pattern-detection filter. Both are small,
independent, and safe to land together.

## Audit test

Adding a test in `consumers/patterns.rs` that explicitly documents
the divergence:

```rust
#[test]
fn pattern_detection_filter_is_broader_than_subtype_is_ephemeral() {
    use open_story_core::subtype::Subtype;
    use std::str::FromStr;

    // Subtype::is_ephemeral covers only progress.*
    assert!(Subtype::from_str("progress.bash").unwrap().is_ephemeral());
    assert!(!Subtype::from_str("file.snapshot").unwrap().is_ephemeral());
    assert!(!Subtype::from_str("queue.remove").unwrap().is_ephemeral());

    // patterns::is_ephemeral is a broader "skip for pattern detection"
    assert!(is_ephemeral("progress.bash"));
    assert!(is_ephemeral("file.snapshot"));
    assert!(is_ephemeral("queue.remove"));

    // Documents the intentional divergence. If a future refactor
    // deletes one in favor of the other, this test is where to
    // read WHY they differed.
}
```

## Recommendation

Land the rename (Fix A) on this branch — it's 20 lines, mechanical,
and removes real ambiguity. Defer Fix B to follow-up; it touches
`ingest_events` which is already the big-refactor target.
