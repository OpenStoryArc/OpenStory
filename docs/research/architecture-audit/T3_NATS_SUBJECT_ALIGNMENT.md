# T3 — NATS Subject ↔ Subscription Alignment

Part of: [PLAN.md](./PLAN.md) · [BOUNDARY_MAP.md](./BOUNDARY_MAP.md)

## Symptom (potential)

`nats_subject_from_path()` at `rs/core/src/paths.rs:38` composes subjects via `format!("events.{project}.{file_stem}.main")`. Neither `project` nor `file_stem` is sanitized — they're raw filename strings. NATS subjects have character restrictions:

- **`.`** is the token separator. A project dir named `my.project` becomes subject `events.my.project.sess.main` — five tokens instead of four. `events.{project}.{session}.>` wildcards no longer match.
- **space** is invalid in NATS subjects. Publish fails with `InvalidSubject`.
- **`>` `*`** are wildcards. Unlikely in filenames but would break subscription routing.
- **unicode**: NATS 2.10+ accepts it, but older versions reject.

Consumers subscribe to the generic `events.>` (all three: persist, patterns, projections, and broadcast via the server). If the coarse subscription still matches malformed subjects, the worst case is silent token-count drift — events arrive but any hierarchical subscription (e.g. `events.{project}.>`) breaks downstream.

## Recon

```
persist.rs       subscribes: events.>
patterns.rs      subscribes: events.>
projections.rs   subscribes: events.>
broadcast.rs     subscribes: events.> + patterns.> + changes.>
```

No hierarchical filters in the default consumer set, so **at the current configuration most bad subjects would still be delivered**. The risk is:

1. A future feature that subscribes `events.{project}.{session}.>` breaks on dotted project names
2. A publish containing a space fails at the NATS driver before any consumer runs
3. Metrics / traces that group by subject see garbled keys

## Audit shape

Two-level test strategy:

### L1. Characterization unit test (cheap, immediate)

Verify current behavior against tricky paths — record what the function does today so any regression is caught.

```rust
// rs/core/src/paths.rs inline tests
#[test]
fn subject_handles_dotted_project_name() {
    let path = PathBuf::from("/watch/my.project/sess.jsonl");
    let watch = PathBuf::from("/watch");
    // Today: "events.my.project.sess.main" — 5 tokens, breaks wildcards
    let subject = nats_subject_from_path(&path, &watch);
    // Record what the function does so regressions are caught
    assert_eq!(subject, "events.my.project.sess.main");
    // Flag expectation: wildcards events.{project}.> will break here
}

#[test]
fn subject_produces_nats_invalid_chars_for_spaces() {
    let path = PathBuf::from("/watch/My Project/sess.jsonl");
    let watch = PathBuf::from("/watch");
    let subject = nats_subject_from_path(&path, &watch);
    // Documents the failure mode: subject contains a space
    assert!(subject.contains(' '));
    // NATS will reject on publish. Consumer never sees the event.
}
```

### L2. Container integration (more expensive)

Stand up the stack, seed a fixture under a dotted/spaced project dir, verify events reach SQLite through `events.>` (coarse match should still work, documents end-to-end reality).

## Fix shape (when prioritized)

Three options, increasing honesty:

- **A. Sanitize**: replace `.`, space, `*`, `>` with `_` before formatting the subject. Pro: always valid NATS. Con: subject ↔ path is lossy; reverse lookup from subject to file becomes ambiguous.
- **B. Percent-encode**: URL-encode the project and session segments. Pro: reversible. Con: `%` is fine in NATS but subjects get ugly.
- **C. Hash-prefix**: hash the raw path, use hash as the token, keep a project/session registry that maps hash → original. Pro: clean subjects, lossless. Con: more moving parts.

**Recommendation when we get here:** Option A with a one-way sanitizer and a warning log when a character is rewritten. Matches the "observe, never interfere" principle better than forcing users to rename project directories.

## Decision for the audit

L1 unit tests only, for now. The tests characterize current behavior and would fire loudly if anyone adds a hierarchical subscription filter. Full container test (L2) is overkill until a real user hits this — gates exist (`watch_dir` defaults to `~/.claude/projects/` which uses UUID dirs, so no dots/spaces in practice).

Add the sanitizer design (Option A) to BACKLOG as "NATS subject sanitization" with a one-line pointer to this doc.

## Exit criteria

- L1 characterization tests added to `rs/core/src/paths.rs`
- BACKLOG entry for the sanitizer
- T3 section updated with outcome

## Outcome (2026-04-14)

Three characterization tests added: dotted project name (extra tokens), space in project name (invalid NATS subject), wildcard chars (`*` passes through). All pass, documenting that the function currently applies **zero sanitization**.

No bug fix pursued — the default watch directories (`~/.claude/projects/` for Claude Code, `~/.pi/agent/sessions/` for pi-mono) use UUID-keyed subdirectories in practice, so the footgun is latent, not live. BACKLOG entry added ("NATS Subject Sanitization") with the three fix shapes (sanitize / percent-encode / hash-prefix) so whoever picks it up starts with a design, not just a symptom.

**Audit value:** the tests now explicitly lock in today's behavior. If someone later adds a hierarchical subscription filter (`events.{project}.>`) without first sanitizing, these tests will be the obvious place to remember why. No surprises when it breaks — it'll break loudly.