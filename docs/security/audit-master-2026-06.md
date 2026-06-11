# Security Audit — master, 2026-06-10

**Target:** `master` at `aea771a` (rebased; tip was `4070ffd` "Merge PR #56 mcp-full-rewrite" + the v0.2.0 / Homebrew / managed-NATS stack landed since).
**Method:** re-ran the 2026-05-28 red-team baseline tooling (`scripts/red_team.py`, `scripts/red_team_live.py`, the `test_security*` suites) against master in an isolated worktree on branch `security/audit-master-2026-06`.
**Scope exclusion:** the `[person]`/principals identity model, Participants admin panel, grant-role bootstrap, `principal_id` threading, Codex host/user stamping, and federation host-in-subject all live on unmerged feature branches and were **deliberately not audited** — this audit is master-only.

The tooling itself was ported from `feat/federation-host-in-subject` (commits `07f8561`, `42b2ca7`, `2cd45f0`). **Only the harness was ported — none of the branch's security *fixes*.** Those had to be re-derived as findings against master, which is the point: it tells us which May fixes never reached master.

---

## Headline finding

**F1 — Unauthenticated single-request DoS that *permanently* bricks the store (HIGH → effectively CRITICAL on master).**

`GET /api/insights/pulse?days=4294967295` (and the other `days`-taking insights endpoints) panics the worker thread at `rs/store/src/queries.rs:328`:

```
thread 'tokio-rt-worker' panicked at store/src/queries.rs:328:18:
`DateTime - TimeDelta` overflowed
```

`chrono::Duration::days(days as i64)` overflows when `days` is large. The 2026-05-28 baseline already caught this and fixed it on the feature branch with a saturating `cutoff_for_days()` helper — **that fix never merged to master.** Master still has the raw `chrono::Utc::now() - chrono::Duration::days(days as i64)` at lines 328, 387, 593, 720, 829.

What makes it worse on master than the baseline recorded: the panic fires **while holding the SQLite connection `Mutex` guard** (`self.conn.lock().unwrap()` in `sqlite_store.rs:202`). The unwinding panic **poisons the mutex**, so every subsequent DB access — *any* endpoint, authenticated or not — then panics on `.lock().unwrap()`:

```
thread 'tokio-rt-worker' panicked at store/src/sqlite_store.rs:202:37   (poisoned-lock cascade)
thread 'tokio-rt-worker' panicked at store/src/sqlite_store.rs:368:37
```

Confirmed live: after one `?days=4294967295` request, `/api/sessions`, `/api/search`, and `/api/insights/pulse?days=7` all returned `000` (connection failed) indefinitely. **One unauthenticated request → total persistent denial of service until process restart.** On the localhost-only default threat model this needs a local caller, but with `host = 0.0.0.0` (the documented Docker/prod shape) it is remotely reachable pre-auth.

**Fix:** port the branch's saturating `cutoff_for_days()` helper. Independently, the `conn.lock().unwrap()` pattern should be hardened (recover poisoned locks, or use a non-poisoning mutex) so a future panic anywhere in a DB closure can't brick the whole store — that's defense-in-depth the branch fix alone doesn't provide.

---

## Dependency CVEs (cargo-audit + osv-scanner)

All present on master; the May audit's dep bumps never merged. CVE IDs are 2026-dated — these advisories postdate the baseline, so even the branch would need a re-bump.

| Crate | Version | Advisory | CVSS | Path | Severity |
|---|---|---|---|---|---|
| rustls-webpki | 0.102.8 | RUSTSEC-2026-0104 (CRL parse panic) | 7.5 | via async-nats 0.38 | HIGH |
| rustls-webpki | 0.102.8 | RUSTSEC-2026-0049/0098/0099 (CRL/name-constraint logic) | 2.2–4.4 | via async-nats 0.38 | LOW-MED |
| hickory-proto | 0.25.2 | RUSTSEC-2026-0118 (NSEC3 unbounded loop) | 8.7 | via mongodb 3.6 (`--features mongo` only) | HIGH |
| hickory-proto | 0.25.2 | RUSTSEC-2026-0119 (O(n²) name compression) | 6.9 | via mongodb 3.6 (`--features mongo` only) | MED |

The rustls-webpki chain is the same root the May baseline fixed by bumping `async-nats 0.38 → 0.49` (zero source changes). Master is still on 0.38. hickory-proto only ships when built `--features mongo`; the default SQLite build is unaffected.

**Unmaintained/yanked warnings (info):** `backoff` (RUSTSEC-2025-0012), `instant` (RUSTSEC-2024-0384), `rustls-pemfile` (RUSTSEC-2025-0134), `fastrand 2.4.0` yanked. These match the baseline's "kube dev-dep chain — don't re-fight" note; no CVEs, no exploit path.

### npm (dev-only, never shipped)

| Package | Advisory | Severity | Tree |
|---|---|---|---|
| vitest <3.2.6 | GHSA-5xrq-8626-4rwp (UI server arbitrary file read/exec) | CRITICAL (dev) | ui/ |
| tmp <0.2.6 | GHSA-ph9p-34f9-6g65 (path traversal) | HIGH (dev) | e2e/ → testcontainers |
| uuid <11.1.1 | GHSA-w5hq-g745-h8pq (buffer bounds) | MED (dev) | e2e/ → dockerode |

All dev dependencies — not in any shipped artifact. The vitest one is `npm audit fix`-able without a breaking change; the e2e/testcontainers chain needs `testcontainers 11 → 12` (the baseline already did this on-branch).

---

## In-process suites

- **`test_security` (20 baseline tests): all green.** Master's core hardening (auth middleware, payload truncation, FTS metachar handling) is intact.
- **`test_security_aggressive` (27 tests): 25 green, 2 red** — both are branch-fixed-never-merged:
  - `huge_days_param_in_pulse_does_not_overflow` → **F1** above.
  - `ws_query_token_authorizes_when_correct` → a valid `?token=` on the WS upgrade is rejected with 401. Master's `auth.rs` has the *doc comment* describing `?token=` support but the query-param branch isn't wired into `auth_middleware` for the WS path. **F2 (MED):** WS auth is effectively unusable when a token is set (browsers can't set WS headers, so the query param is the only path) — this is an availability bug with a security framing, the branch implemented the fallback.

## Live adversarial probes (`red_team_live.py`, 19 vectors)

**19/19 blocked** against an unauthenticated master instance (the baseline's threat model). Path traversal, static-dir escape, NoSQL operator injection, CORS reflection, HTTP smuggling (TE.CL), JSON depth bomb, gzip bomb, slowloris, WS flood, pipelining, header bomb, timing-attack resistance — all held.

> **Methodology note (important for re-runs):** running the live script with `--token` against an *authenticated* instance produces 3 **false-positive** "exploits" (slowloris, pipelining, WS flood). The script's post-flood health check hits `/api/sessions` *without* the token, gets a correct `401`, and misreads it as "server DoS'd." The server was verifiably `200` after every flood. Run DoS probes against an unauthenticated instance, or fix the script to authenticate its health checks. Tracked as a tooling bug, not a server finding.

## Container suite (`test_security_container`)

7/15 failed — **all harness failures, not server findings.** The container helper boots `open-story:test` with no reachable NATS; master makes NATS a hard boot dependency, so the container exits before the probes connect. (The live probes above cover the same vectors against a properly-NATS-wired container.) This is the same class as the baseline's documented `PortNotExposed` harness limitation. To fix: the container helper needs a NATS sidecar or `--manage-nats` with a bundled `nats-server` binary in the test image.

---

## MCP surface (new on master — PR #56 rewrite)

The `rs/mcp` server is **stdio transport, not network** (`stdio.rs`: line-delimited JSON-RPC on stdin/stdout). So the HTTP live-probe suite doesn't apply; the threat model is a local, single-user trust boundary where the parent agent process (Claude Desktop, etc.) is trusted.

Assessed by reading:
- **SQL injection via tool args: not present.** All 15 tools are read-only store queries. The injection-prone path — `search` → `store.search_fts(query, ...)` — uses `rusqlite::params![query, sid, limit]` (fully parameterized, `sqlite_store.rs:234`). FTS5 syntax errors return a graceful `Err(String)`, not a panic.
- **Protocol parsing:** malformed JSON → `parse_error()` response, no panic.
- **F3 (INFO):** the stdin reader uses `BufReader::lines()` / `next_line()` — **unbounded line length.** A single newline-less line can grow until OOM. Given the local-trusted-parent threat model this is info-level, but a `take(limit)` bound would be cheap defense-in-depth. Note this for the MCP threat model doc; not a merge blocker.

---

## Deploy-surface deltas vs the May baseline (these fixes never merged)

- **Caddyfile (F4, MED):** has `X-Content-Type-Options`, `X-Frame-Options`, `Referrer-Policy` — but **no CSP and no HSTS**, and the `Server` header isn't suppressed. The baseline added all three. Port them.
- **`docker-compose.prod.yml` (F5, MED):** `OPEN_STORY_API_TOKEN=${OPEN_STORY_API_TOKEN:-}` — the `:-` makes the token **optional**, so a prod deploy silently boots with no auth. The baseline changed this to `:?` (required, fail-fast). Port it.
- **`Dockerfile.prod` (F6, LOW):** doesn't COPY the `mcp` workspace member, which is now a member of `rs/Cargo.toml`. Master's `Dockerfile.prod` may fail to build the prod image (the baseline hit the same class of bug with the `mcp` crate). Needs a build verification.
- **Env-var naming (F7, INFO):** the NATS URL flag reads env `NATS_URL`, not `OPEN_STORY_NATS_URL` — breaks the documented `OPEN_STORY_*` convention. Cosmetic, but surprising in ops.

---

## Threat-model notes carried forward (don't re-fight)

From the 2026-05-28 baseline, still accepted:
- `/api/health` exposing version/backend/session-count is by-design when `api_token` is unset (localhost-only); auth gates it when a token is set.
- `user: "0:0"` in compose files is needed to read Claude's 600-mode transcript files; network/process isolation still applies.
- The kube/backoff/instant/rustls-pemfile unmaintained chain — warnings, not exploits.

---

## Prioritized remediation

| # | Finding | Severity | Fix | Status |
|---|---|---|---|---|
| F1 | `days` overflow → panic → **poisoned-mutex full DoS** | HIGH (CRIT on 0.0.0.0) | `cutoff_for_days()` saturating helper + harden all 20 `conn.lock()` against poisoning | ✅ FIXED `658a5b4` |
| — | rustls-webpki CVE chain | HIGH | Bump `async-nats 0.38 → 0.49` | ✅ FIXED `90ea571` |
| — | hickory-proto CVE chain (mongo only) | HIGH | Bump `mongodb 3.6 → 3.7` | ✅ FIXED `401e168` |
| F2 | WS `?token=` rejected | MED | Query-param fallback in `auth_middleware` (header-authoritative precedence) | ✅ FIXED `5b46c26` |
| F4 | Caddyfile missing CSP/HSTS | MED | Add headers + suppress Server | ✅ FIXED `daabd84` |
| F5 | prod compose token optional | MED | `:-` → `:?` | ✅ FIXED `daabd84` |
| F6 | Dockerfile.prod mcp COPY | LOW | Add mcp manifest/src/benches + stubs | ✅ FIXED `191269d` |
| — | vitest/tmp/uuid (dev) | LOW (dev) | `npm audit fix`; testcontainers 11→12 | ⬜ OPEN (dev-only, not shipped) |
| F3 | MCP unbounded stdin line | INFO | `take(limit)` bound | ⬜ OPEN (local-trusted threat model) |
| F7 | `NATS_URL` env naming | INFO | Align to `OPEN_STORY_*` | ⬜ OPEN (cosmetic) |

### Remediation applied on this branch (`security/audit-master-2026-06`)

Every fix above was implemented TDD-first and verified. After the fixes:
- `cargo audit` exits **0** — all 6 CVEs cleared (was 6 at audit start; only the 4 documented unmaintained-dep *warnings* remain).
- `test_security` (20) + `test_security_aggressive` (28, incl. the new poison-recovery regression) + `auth::` (13) — **all green, zero red.**
- Full `open-story-store` (263) and `open-story-server` (140) suites green.
- The 19-vector live probe is 19/19 blocked (unauthenticated instance).

Three INFO/dev-only items left open by design — none is a shipped-artifact exposure. The npm dev CVEs are `npm audit fix`-able whenever the UI/e2e toolchains are next touched.

### Follow-up test coverage (added after the initial fixes)

The initial fixes were verified mostly against the *ported* probe harness. These additions close the gap between "verified by reasoning" and "verified by test":

- **Per-site lock-poison recovery** (`poisoned_lock_recovers_across_call_sites`, sqlite_store.rs) — poisons the connection mutex via a panic inside `with_connection`, then exercises **5 distinct lock sites** (sync read, async read, list, write, FTS) and asserts each recovers *and* the connection stays functional (a post-poison write persists). Verified to fail loudly when any one site is reverted to `.unwrap()`, so it genuinely detects the F1 vulnerability rather than just passing.
- **MCP JSON-RPC fuzz** (5 tests, protocol.rs) — throws ~33 adversarial frames at `handle_message` (truncated/non-JSON, null bytes, control chars, wrong types, 1MB method, **20K-deep nesting bomb**) asserting no panic/stack-overflow and that the contract holds (invalid → Parse error, notification → None, unknown method → Method-not-found). Confirms the read-audit conclusion: the stdio parser is robust, and serde's recursion limit defuses the depth bomb.
- **Mongo conformance behavior-verified** — `cargo test -p open-story-store --features mongo --test event_store_conformance` runs the full parity suite (testcontainers mongo): **112 passed, 0 failed** after the mongodb 3.6→3.7 bump. The CVE fix is now verified at behavior level, not just build+audit.
