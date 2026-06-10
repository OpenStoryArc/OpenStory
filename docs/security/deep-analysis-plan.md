# Security analysis plan — beyond reputation

This document records the concrete tools and test strategies we use to
examine OpenStory's security posture without relying on dependency
*reputation* alone. Every entry is a callable command or a runnable test
file, not advice.

## Soul

We don't trust crates because they're popular — we trust them because
we (or someone we trust) examined them. This plan operationalizes that.

## Status snapshot (2026-05-28)

| Surface | Tool | Result |
| --- | --- | --- |
| Rust 567 crates | `cargo-audit` | 0 CVEs, 3 unmaintained warnings (all via dev-only `kube 0.98`) |
| Rust deny check | `cargo-deny advisories bans sources` | 0 bans, 0 unknown registries, 0 git deps |
| Rust OSV cross-ref | `osv-scanner` | 0 CVEs (same conclusion, different DB) |
| npm UI 462 pkgs | `npm audit` + `osv-scanner` | 0 vulns |
| npm e2e 180 pkgs | `npm audit` + `osv-scanner` | 0 vulns (after `testcontainers@12`) |
| Python | `osv-scanner` | 0 vulns (after pinning `idna>=3.10`) |
| Install scripts | manual lockfile audit | 7 packages, all well-known (esbuild, fsevents, @swc/core, cpu-features, ssh2, protobufjs) |
| Server endpoints | 25 aggressive in-process tests (`test_security_aggressive.rs`) + 20 baseline (`test_security.rs`) | all green; 1 DoS found and fixed |

## Tools we run (and why each catches what others miss)

### Vulnerability databases — cross-reference, don't single-source
RustSec, GHSA, and OSV publish overlapping but non-identical advisory
sets. A vuln embargoed in one DB may already be public in another.

```bash
# 1. RustSec — Rust-native, curated
cargo audit

# 2. OSV — Google's cross-ecosystem DB (RustSec + npm + PyPI + Go + Maven)
osv-scanner scan source -L rs/Cargo.lock \
                        -L ui/package-lock.json \
                        -L e2e/package-lock.json \
                        -L telegram-bot/requirements.txt

# 3. Policy checks — bans, license clashes, multiple versions of same crate
cargo deny check advisories bans sources licenses
```

### Beyond CVE databases — examine the dependency itself
A library can be malicious without being in any CVE database (the
shai-hulud worm published clean-looking code that was only detected
days later). For each new direct dep, **look at the code**.

```bash
# Count unsafe blocks per crate. Spikes in unsafe = surface to review.
cargo geiger --workspace

# Map: which person / organization is allowed to push each crate?
# A direct dep with one publisher who has no other published crates
# is a red flag.
cargo supply-chain crates
cargo supply-chain publishers

# Compare current Cargo.lock to a known-good baseline before merging
# any dependency-touching PR. Whitelist diff entries the change author
# explains; reject silent additions.
cargo tree --workspace > /tmp/tree.now
diff <(git show master:rs/Cargo.lock) rs/Cargo.lock
```

### Container image scanning
The runtime image bundles glibc, OpenSSL, libsqlite3, ca-certificates —
all of which can ship CVEs independent of our crates.

```bash
# Build the prod image, then scan
docker build -f Dockerfile.prod -t open-story:prod .
trivy image --severity HIGH,CRITICAL open-story:prod
grype open-story:prod                # alternate scanner; different DB
```

### Source-level static analysis
```bash
# Clippy — the Rust linter has security-relevant lints
cargo clippy --workspace --all-targets -- -D warnings

# Semgrep — pattern-based static analysis with security rules
semgrep --config=p/rust --config=p/owasp-top-ten rs/
semgrep --config=p/javascript --config=p/typescript ui/src/

# Bandit for Python (telegram-bot + scripts/)
bandit -r telegram-bot/ scripts/
```

### Secret scanning — historical and current
A secret committed once is a secret leaked forever.

```bash
# History scan — catches secrets in old commits
trufflehog git file://. --since-commit master

# Pre-commit hook in .pre-commit-config.yaml: gitleaks
gitleaks protect --staged
```

### Fuzzing the parsers and translators
The translator turns untrusted JSONL into typed CloudEvents. A
malformed line shouldn't panic; a giant line shouldn't OOM.

```bash
# cargo-fuzz on the translator's `translate_line` function
cd rs && cargo fuzz init && cargo fuzz add translate_line
# Body of fuzz_target!: pass `data: &[u8]` to `translate_line(data)`
# and assert no panic.
cargo fuzz run translate_line -- -max_total_time=300

# proptest for round-trip invariants (already wired up — extend coverage)
# Pattern: any CloudEvent → serialize → deserialize → equal to original.
cargo test -p open-story-schemas --test proptest_roundtrip
```

### Mutation testing — do the tests actually catch bugs?
A test that always passes is worse than no test. Mutation testing
proves the test detects real defects by introducing fake ones.

```bash
cargo install cargo-mutants
cargo mutants -p open-story-server -- --test-threads=1
# Surviving mutants = code paths your tests can't distinguish from
# their inverse. Audit each survivor in critical modules (auth.rs,
# api.rs, queries.rs).
```

### Concurrency / race condition probing
The persist consumer dedups events; the patterns consumer reads
shared state. Loom explores every possible thread interleaving.

```bash
cargo install --locked loom-cli || true   # optional helper
# Wrap critical concurrent sections in `loom::model { ... }` blocks
# and assert invariants hold under every interleaving.
RUSTFLAGS="--cfg loom" cargo test -p open-story-server consumers::persist::loom
```

### Authorization matrix testing
For every (endpoint × HTTP method × auth state × role) tuple, the
behavior should be exactly one of {200, 401, 403, 404}. Drift
between expected and actual is a bug.

```bash
# A simple authz-matrix harness — runs against the testcontainer.
cargo test --test test_security_container authz_matrix
```

## What we test (and how each test fends off a class of attack)

### In-process (axum-tower) — fast, deterministic
`rs/tests/test_security.rs` (20 tests) + `test_security_aggressive.rs` (27 tests):
- Bearer/token auth bypass — wrong scheme, empty value, prefix match, 100 KB token
- WebSocket auth gating — Bearer header AND `?token=` query param
- Body-limit DoS — 51 MB POST returns 413, not OOM
- FTS5 metacharacter abuse — 8 adversarial queries don't crash
- Path traversal — `..`, `..\\`, URL-encoded `%2e%2e`, symlinks
- Concurrent `delete_session` TOCTOU race
- Adversarial query params — negative `limit`, max-u32 `days`, CRLF/NUL in `session_id`
- SQL injection in event ID, subtype, session ID

These run in <1s and exercise the auth + middleware + handler stack
in process. They can't catch issues that only surface with real
Docker, real OS process boundaries, or real network buffering.

### Container-based (testcontainers) — real network, real process
`rs/tests/test_security_container.rs`:
- Real HTTP/1.1 traffic into a running `open-story:test` container
- Pipelined requests, malformed Content-Length, header smuggling
- Concurrent connection floods (slowloris-style probing)
- Real Docker network — bind-mount permissions, signal handling
- WebSocket upgrade against a real bound socket

### Manual review for every PR that touches:
- `rs/server/src/auth.rs`
- `rs/server/src/router.rs`
- `rs/server/src/ws.rs`
- `Caddyfile`
- `Dockerfile*`, `docker-compose*.yml`
- `rs/store/src/sqlite_store.rs` (SQL construction)
- Any new direct dependency in `Cargo.toml`, `package.json`, or `requirements.txt`

## Continuous-audit cadence

| Cadence | Action |
| --- | --- |
| Per-PR (CI) | `cargo audit`, `npm audit`, `cargo deny check`, full test suite, `cargo clippy -D warnings`, `semgrep --config=p/rust` |
| Weekly | `osv-scanner` on all lockfiles, `cargo supply-chain publishers`, `trivy image open-story:prod`, `gitleaks` |
| Monthly | `cargo geiger` diff vs last month, `cargo mutants` on critical modules, dependency-tree review |
| On a new direct dep | Manual source review of the crate (latest published version), publisher check, optionally write a `cargo vet` audit entry |

## Known-acknowledged warnings (not fixed)

- `backoff 0.4.0` — unmaintained, via `kube 0.98` dev-dep. Cleared by
  bumping kube to 1.x (semver-major API rewrite). Tracked in BACKLOG.
- `instant 0.1.13` — unmaintained, via `notify 7.0` + `kube`. Same fix path.
- `rustls-pemfile 2.2.0` — unmaintained, via `kube 0.98`. Same fix path.

None of these are CVEs; they're "this crate has no maintainer to ship
future security patches." They reach production only if the user
enables k8s features (rare for self-hosted).

## Threat model — what we explicitly protect against vs accept

### Protected against
- Network attackers without the API token (Bearer or `?token=`)
- Malicious JSONL files dropped into `watch_dir` (translator must not panic)
- Path traversal escaping `data_dir`
- SQL/FTS5 injection via any user-controllable string
- DoS via large bodies, deep JSON, huge query params (all bounded)
- Supply-chain via typosquatted package names (lockfile-pinned, audited)

### Accepted risk (documented)
- A user who can write files to `~/.claude/projects/` can craft any
  JSONL they want. They are already that user; we are not a security
  boundary against them.
- A user who can bind-mount a directory into the container can replace
  it at any time. Container provides isolation from the *network*,
  not from the *operator*.
- `user: "0:0"` in `docker-compose.*.yml` because Claude's transcript
  files are mode 600 owned by the host user; reading them requires
  matching uid or root. We pick root inside the container; container
  isolation still applies to the network/process surface.
