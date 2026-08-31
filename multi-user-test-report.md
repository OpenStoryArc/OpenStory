# Arena multi-user sandbox isolation test — report

## What was added

`arena/src/docker_driver.rs`, inside the existing `#[cfg(test)] mod docker_tests`
block (same module as `create_is_sealed_and_destroy_cleans_up`), one new test:

```rust
#[tokio::test]
#[ignore]
async fn multiple_users_get_isolated_sandboxes_and_clean_teardown() { ... }
```

It reuses the module's existing helpers (`test_config()`, `ensure_image()`) and
adds one import, `use anyhow::Context;` (needed for `.with_context(...)` on the
per-user `create` results). No production code (`docker_driver.rs` outside the
test module) was touched.

### Shape of the test

1. **3 users**, `mutest--a`, `mutest--b`, `mutest--c` — `--` makes these
   permanently invalid as real usernames (`naming::validate_username` rejects
   `--`), same reasoning as the existing `PROBE_USER = "arenatest--rt"`. Each
   gets a `SandboxSpec` with event `"mutest"`, image `alpine:3.20`, a distinct
   fake `api_key`, and `expires_at = Utc::now()`.
2. **Pre-clean**: `destroy(user, false)` best-effort for all 3 up front, so a
   previous failed run can't poison the assertions.
3. **Concurrent provisioning**: all 3 `create()` calls are launched together
   via `futures_util::future::join_all` (already a dev-dependency — no new
   crate added), not awaited sequentially. Asserts all 3 succeed and return
   distinct container ids.
4. **Per-user isolation assertions**, for each user:
   - `is_running(id) == true`
   - `sealed_network_exists(network_name(u)) == true` (internal + bridge)
   - `volume_exists(volume_name(u)) == true`
   - **Positive control**: `exec_exit_code(sandbox_a, ["sh","-c","command -v wget"]) == 0`,
     run *before* the cross-reach probes, so a nonzero cross-reach exit can't
     be misread as "no tool" instead of "blocked" (mirrors the v1 single-user
     test's seal-probe reasoning).
   - **Cross-user unreachability** (the actual isolation proof): for each of
     the 3 ordered pairs (a→b, b→c, c→a), `exec_exit_code(sandbox_a, ["wget",
     "-T","3","-q","-O-","http://sandbox-mutest--b"])` must be nonzero — each
     user's sealed *internal* network has no route to any other user's
     network, so DNS resolution/connection to another sandbox's container
     name fails.
5. **Clean teardown**: `destroy(user, false)` for all 3, then per-user
   asserts `is_running == false`, `network_exists == false`,
   `volume_exists == false`. Then a second `destroy` on one user is asserted
   `Ok` (idempotent).
6. **Daemon-clean-even-on-failure**: all assertions inside the concurrent
   create + isolation-check phase report failure via `anyhow::ensure!`/`?`
   into a `Result` rather than `assert!`/`panic!`. Teardown for all 3 users
   runs *unconditionally* after that block, regardless of whether it
   returned `Ok` or `Err`; only then does the test `.unwrap()` the outcome
   (so a genuine failure still fails the test, but only after cleanup ran).

## Real `just arena-test-docker` output

```
$ just arena-test-docker
cd arena && cargo test -- --ignored
    Finished `test` profile [unoptimized + debuginfo] target(s)
     Running unittests src/lib.rs (target/debug/deps/arena-...)

running 3 tests
test docker_driver::docker_tests::create_refuses_a_pre_existing_network_that_is_not_sealed ... ok
test docker_driver::docker_tests::create_is_sealed_and_destroy_cleans_up ... ok
test docker_driver::docker_tests::multiple_users_get_isolated_sandboxes_and_clean_teardown ... ok

test result: ok. 3 passed; 0 failed; 0 ignored; 0 measured; 62 filtered out; finished in 16.99s
```

(Other test binaries — `http_auth.rs`, `http_launch.rs` — reported 0 ignored
tests to run, as expected; they have no `#[ignore]` tests.)

## Confirming the isolation assertion is discriminating (not vacuous)

TDD nuance requested: the behavior under test already works (the seal is
proven by the existing single-user test), so this test should go green
immediately — the rigor demonstration here is proving the *new* cross-user
assertion would actually catch a real break, not just always pass.

Method: temporarily inverted the expectation in the cross-reach loop from
`exit != 0` to `exit == 0` (i.e., "assert `a` CAN reach `b`"), leaving
everything else — including the positive control — untouched, and reran:

```
$ cd arena && cargo test multiple_users_get_isolated -- --ignored
running 1 test
test docker_driver::docker_tests::multiple_users_get_isolated_sandboxes_and_clean_teardown ... FAILED

---- ... stdout ----
thread '...' panicked at src/docker_driver.rs:988:17:
called `Result::unwrap()` on an `Err` value: mutest--a could reach
mutest--b across sandboxes — cross-user isolation is broken

test result: FAILED. 0 passed; 1 failed; ...
```

This confirms: the real exit code from `wget` reaching across sandboxes is
genuinely nonzero (the network layer really does block it) — the assertion
is not passing "by accident" (e.g. because of a typo'd container name, a
missing binary, or an exec plumbing bug that always returns nonzero
regardless of network reachability). The positive control earlier in the
same run already proves `wget` exists and `exec` works correctly on
`mutest--a`, so the nonzero result can only mean the network blocked it.

Also confirmed the daemon was left clean after this *deliberately failing*
run — the unconditional teardown ran despite the panic:

```
$ docker ps -a --filter "name=mutest--" --format '{{.Names}}'
$ docker network ls --filter "name=arena-sb-mutest" --format '{{.Name}}'
$ docker volume ls --filter "name=arena-home-mutest" --format '{{.Name}}'
(all empty)
```

Reverted the inversion (`exit != 0` restored) and reran — back to green (see
above). Diff was a one-line temporary edit, applied and reverted in the same
session; final committed code has the correct `exit != 0` assertion.

## Non-ignored suite + clippy

```
$ cd arena && cargo test
test result: ok. 62 passed; 0 failed; 3 ignored; 0 measured; 0 filtered out   # unit tests (lib)
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # main.rs
test result: ok. 10 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out   # tests/http_auth.rs
test result: ok. 6 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # tests/http_launch.rs
test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out    # doc-tests

$ cd arena && cargo clippy --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s)   # zero warnings
```

`3 ignored` in the unit-test line is `create_is_sealed_and_destroy_cleans_up`,
`create_refuses_a_pre_existing_network_that_is_not_sealed`, and the new
`multiple_users_get_isolated_sandboxes_and_clean_teardown`.

## Daemon-clean confirmation

Before starting: `docker ps`/`network ls`/`volume ls` filtered on `mutest--`
showed nothing (verified before writing any code).

After the full `just arena-test-docker` run (all 3 ignored tests green):

```
$ docker ps -a --filter "name=mutest--" --format '{{.Names}}'
$ docker network ls --filter "name=arena-sb-mutest" --format '{{.Name}}'
$ docker volume ls --filter "name=arena-home-mutest" --format '{{.Name}}'
(all empty)
```

Also verified clean after the deliberately-failing inverted-assertion run
(see above) — the unconditional teardown holds even on test failure.

## Dependencies

No new dependency added. `futures_util::future::join_all` comes from
`futures-util = "0.3.34"`, already an `arena` dev-dependency (used elsewhere
in the same test module via `futures_util::StreamExt` for `ensure_image`).
`tokio::task::JoinSet`/`tokio::spawn` were considered but would require
`'static` futures (cloning `Docker` + `DockerDriverConfig` per task); `
join_all` over `&DockerDriver` borrows is simpler and still exercises true
concurrent interleaving of the awaited Docker API calls within the single
test task — which is what actually matters for proving no cross-user race
in `create()` (network/volume/container creation for different users
interleaving on the daemon), as opposed to OS-thread parallelism.

## Findings / concerns

None. No race or isolation bug found in `docker_driver.rs`. The existing
409-conflict handling in `create()` is scoped to same-username races (a
network name collision within one user's `create` calls); this test does
not exercise that path (each user has a distinct network name by
construction) — it specifically covers the *different-users-concurrently*
shape, which was previously untested. Concurrent creation across 3 distinct
users completed cleanly every run, with distinct container ids and no
cross-network contamination.
