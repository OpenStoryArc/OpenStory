# Principle Tests — OpenStory

The idea: each auditable principle from CLAUDE.md is paired with a
test file that encodes the principle as an executable invariant. The
test reads the code, applies a conservative heuristic, and fails with
actionable output when the principle is violated.

See also: [BUS_WALK.md](./BUS_WALK.md),
[EVAL_APPLY_WALK.md](./EVAL_APPLY_WALK.md), and the reflection that
motivated this work in the `research/architecture-audit` branch
history.

## Why

Audits of implementation (walks 1–11 on this branch) find drift:
duplicated logic, dead code, untested branches, occasional bugs.
Useful, but every walk is a one-off.

Principle tests do the opposite job: they're **standing guards** on
the project's identity. They don't verify that `function X works` —
they verify that `the shape of the system is still what we said it
was`. Runs on every CI. Fails when drift crosses a line.

The exponential-benefit shape discussed in the audit reflection:
schemas and principle-tests > boundary tests > unit tests. Not
because the former are more tests; because each one guards a
broader invariant.

## Current principle tests

| # | Principle (CLAUDE.md) | Test | Shape |
|---|-----------------------|------|-------|
| 1 | Observe, never interfere | `rs/tests/test_principle_observe_never_interfere.rs` | grep — forbidden write-op patterns on watch_dir-adjacent lines |
| 3 | Actor systems and message-passing | `rs/tests/test_principle_actor_isolation.rs` | grep — forbidden cross-module imports in consumer actors |
| 4 | Functional-first, side effects at edges | `rs/tests/test_principle_functional_purity.rs` | grep — forbidden I/O patterns in declared-pure modules |

Three tests. ~400 LOC total. They execute in under 3 seconds combined.

## Shape of each test

Every principle test follows the same template:

1. **Top-of-file comment quotes CLAUDE.md verbatim.** Anyone reading
   the test sees the principle first. Changes to the principle
   require updating the quote.
2. **Declares the audit surface explicitly.** A list of source files
   (`PURE_MODULES`, `ACTOR_MODULES`) or a path-walk pattern. No implicit
   discovery.
3. **Declares forbidden patterns with reasons.** Each pattern is
   tagged with a one-line explanation that appears in the failure
   output.
4. **Has an allowlist for intentional exceptions.** A violation
   matching the allowlist is reported as ✓ but doesn't fail. Entries
   in the allowlist are their own documentation.
5. **Self-validates.** The observe-never-interfere test asserts `>= 5
   watch_dir sightings` so a future refactor that renames or restructures
   fails loudly rather than silently passing an empty scan.
6. **Fails with actionable output.** Every violation prints the
   file, line number, offending text, and the tagged reason.

## Validation

Every test was smoke-tested by planting a fake violation in real
source, running the test, watching it fail with the right message,
then removing the plant. See commit history for the specific plant +
revert cycles.

## What's not here yet

Principles from CLAUDE.md that could become principle tests but
aren't:

- **#5 Reactive and event-driven** — "Data flows one direction." Testable as: "ViewRecord never flows back into CloudEvent." Hard to grep; needs AST analysis.
- **#6 Open standards, user-owned data** — already tested indirectly by the schema registry capstone (`rs/schemas/tests/test_jsonl_escape_hatch.rs`) which validates every on-disk JSONL line against the declared schema. Arguably the strongest principle-test on the branch.

Principles that are NOT testable programmatically (by design):

- **#2 BDD** — practice, not invariant
- **#7 Minimal, honest code** — taste
- **#8 Shift prototyping left** — practice
- **#9 Scripts over rawdogging** — practice

## Limits of grep-style principle tests

Honest about what these catch and don't:

- **Catch:** direct forbidden-pattern occurrences. `std::fs::write(watch_dir.join(...))` fires.
- **Miss:** indirect flows. If a function takes a `&Path` parameter that happens to be derived from watch_dir several call sites back, the grep doesn't know. A real data-flow analysis via MIR would catch those.
- **Miss:** type-based violations. "This Arc shouldn't leak across this boundary" is a type question, not a string-match question.

The spike chose grep because it's cheap and the false-positive rate turned out to be zero for the tested surface. If drift starts hitting the grep's blind spots, the escalation path is syn-based AST scanning — more ceremony, catches more.

## Adding a new principle test

Rough recipe:

1. Pick a principle from CLAUDE.md that has a testable invariant.
2. Decide the audit surface — which files does the principle apply to?
3. Decide forbidden patterns — what substrings indicate a violation?
4. Write the test file following the template above.
5. **Validate by planting a fake violation.** If the plant doesn't fail the test, the scanner is broken.
6. Add a row to the table in this file.
7. Optional: link from CLAUDE.md back to the test file, so future readers see the enforcement.

## The bigger idea

This spike validated (for me) a methodology I'd only half-believed
before: **testing intention, not implementation.** A codebase whose
principles are executable contracts continues to match its own soul
as it evolves. The alternative — principles stated only in
documentation — drifts slowly and invisibly until the soul and the
code no longer recognize each other.

Three tests isn't enough to guarantee that. But three tests running
on every CI is enough to make the next drift *visible* the moment it
happens. That's the compounding value.
