# Cybersecurity Spike — What Would "World-Class" Take?

**Status:** Research spike. Sets the roadmap for security work; individual phases promote to BACKLOG entries when sequenced. Companion to the empirical `nats-permissions-spike.md`, which provides the NATS evidence Phase 1 builds on.

## Why now

OpenStory's stated soul is **personal sovereignty**: the user observes what their agent does, owns their data, and never has to trust blindly. That promise has direct security implications. Three forces converge:

1. **PersonId v1 just shipped** (`feat/person-id-fleet-view`). The codebase now has an identity model (Person + Principals), but it is *only* a tag for UI grouping — it is not an authorization boundary at the bus, store, or WebSocket. The infrastructure for sovereignty enforcement exists; the enforcement does not.
2. **Distributed deployment is live.** NATS leaf nodes propagate every team member's events to every machine in the fleet, and at least one *verified live secret exposure* exists today: the NATS token prints in plaintext startup logs (BACKLOG: "HOTFIX: Redact NATS token from startup logs").
3. **YC application targets 2026-05-05.** "World-class cybersecurity" is no longer hypothetical — it is part of the credibility bar for a tool that asks users to point a watcher at every coding session they run.

## Two architectural commitments

Two design choices shape every phase below. They shrink the roadmap and make it structurally stronger.

**1. We never classify the content of agent events as "secret."** Pattern-matching for secrets would put OpenStory in a content-judgment role that violates "observe, never interfere" — false positives create false confidence, false negatives miss things, and *what is secret in your work* is not the tool's call to make. The answer to the secrets-in-events problem is **access control, not content inspection**: encrypted at rest, displayed only to the authorized Person. The only redaction OpenStory ever does is *structural metadata* (host, cwd, file paths, project name) and only when the user explicitly opts a session into lab-public sharing.

**2. The identity boundary lives at the NATS bus, not at the API.** The current subject scheme (`events.{project}.{session}.>`) is identity-blind — every consumer receives every event and the application has to trust itself to filter. The right shape is `events.{person_id}.{principal_id}.{project}.{session}.>` with NATS permission ACLs (Mode B) or NATS accounts (Mode C) enforcing per-principal subscription rights at the broker. The bus enforces isolation; the API filter is belt-and-suspenders. This redesign also dissolves the latent subject-sanitization footgun (BACKLOG line 586) since UUIDs lead the new scheme.

These commitments collapse what would have been a content-redaction phase into a single per-Person key derivation, and they push identity enforcement *down* to where the broker can enforce it. Less code, more guarantee.

---

## Threat model: three deployment modes

OpenStory has three trust topologies. The security bar differs per mode.

### Mode A — Solo localhost (the default today)
- Single user, single machine, NATS + server + UI on `127.0.0.1`.
- Attacker model: malware on the same machine, or a malicious file dropped into `~/.claude/projects/`.
- Guarantees: no remote network exposure by default; no plaintext secrets in logs; bounded resource use on adversarial input.

### Mode B — Team hub + leaf nodes (live on Hetzner now)
- NATS leaf nodes over Tailscale push every team member's session to every node + hub.
- Hub dashboard browsable by anyone on the Tailscale net unless `OPEN_STORY_API_TOKEN` is set.
- Attacker model: compromised Tailscale peer, stolen NATS token, malicious teammate trying to read another principal's data.
- Guarantees: credentials not in process args/logs; mutual TLS between leaf and hub; per-principal read filtering on the API + WS; tamper-evident JSONL for audit.

### Mode C — Public lab / multi-tenant (in BACKLOG, not yet shipped)
- Multi-Person, OIDC-backed, opt-in "public" session scopes for an observatory or competition.
- Attacker model: untrusted Person trying to read another's sessions; screenshotted session leaking host/cwd; supply-chain attack on the broadcast path.
- Guarantees: authorization gate at every read path; structural anonymization that is type-system enforced; BYOK with per-Person encryption at rest; compile-time impossible to broadcast a non-anonymized record to a lab-public subscriber.

---

## Current posture — verified inventory

What is actually in place today (file paths verified against current code):

**Authentication.**
- Bearer token middleware at `rs/server/src/auth.rs:28-55`, constant-time comparison at lines 13-22.
- Wraps all routes including `/ws` at `rs/server/src/router.rs:209-213`.
- Empty token = pass-through (default — no auth).
- `/metrics` is intentionally outside auth (line 218-227, for Prometheus).
- **Documented-but-not-implemented gap:** `auth.rs:5` claims `?token=` query-param fallback for WebSocket (since browsers cannot set headers on a WS upgrade); the middleware reads only `Authorization`, and `ws.rs` does not extract a query token. Net effect: setting `api_token` breaks browser WebSocket connectivity.

**Encryption at rest.**
- SQLCipher integration exists at `rs/store/src/sqlite_store.rs:37-66` but is feature-gated behind `open-story-store/encryption`. Without the feature, the key is accepted, logged-as-warning, and silently ignored.
- JSONL persistence (`rs/store/src/persistence.rs`): always plaintext.
- Plans directory: plaintext Markdown.
- Mongo backend: no encryption hooks.
- NATS JetStream on-disk: no encryption.

**Encryption in transit.**
- HTTP server: plain Axum, no TLS path in the codebase.
- WebSocket: same socket as HTTP, no TLS.
- NATS: `nats://TOKEN@host:port` URL form supported (`rs/bus/src/nats_bus.rs:27-43`); no TLS option exposed.

**CORS / network.**
- Default bind: `127.0.0.1` on desktop; `0.0.0.0` in containers/WSL.
- CORS defaults to localhost-only; configured origins allow `Any` methods/headers.
- Body limit: `DefaultBodyLimit::max(50 MB)`.
- **No rate limiting anywhere.**

**Input handling.**
- Watcher follows symlinks (`WalkDir::follow_links(true)`).
- No line-length cap when reading JSONL — a 1 GB single line OOMs the watcher.
- `serde_json` parses arbitrary depth/size — no recursion or field-count guards.
- FTS search is parameterized — no SQL injection.

**Logging.**
- Tracing logs metadata only (`rs/server/src/logging.rs`) — no event payloads.
- **Confirmed live secret leak:** NATS connection URL is printed to stderr at server boot, including the token, in production on the Hetzner deploy. Trivially fixed but unfixed today.

**Identity enforcement.**
- `person_id` and `principal_id` stamped on every CloudEvent at ingest; persisted on the `sessions` table.
- **Not enforced:** every WebSocket client receives every event; `/api/sessions` and `/api/events` do not filter by Person; `BroadcastConsumer` does no authorization.
- **The NATS subject scheme is identity-blind.** Events publish to `events.{project}.{session}.>`. The `nats-permissions-spike.md` already proved that adding per-subject perms works for the data plane but that single-account JetStream consumers can leak across tenants. That finding shapes Phase 1.
- The Fleet view is *visual organization*, not a security boundary.

**Supply chain.**
- Open Dependabot CVE on `rustls-webpki 0.102.8` (GHSA-4cqp-r62p-h3hg) pinned through `async-nats 0.38` (BACKLOG line 543).
- No `cargo audit`, `cargo deny`, or `npm audit` in CI today.
- No SBOM, no signed releases.

**Subprocess / shell.** No `Command::spawn` or shell interpolation found in the server crate. Agent-issued shell commands are *recorded*, not executed, by OpenStory.

---

## Gap analysis & roadmap

Each phase is sized to ship as one PR (or a small stack). Verification is concrete and runs in CI.

### Phase 0 — Bleeding-stops hotfixes (1–2 days)

**0.1 Redact NATS token from startup logs.** Extend `NatsBus::connect` or the call site in `rs/cli/src/main.rs` with a `redact_userinfo(url: &str) -> String` helper. Apply to success + error log paths. ~30 LOC. *Already specced in BACKLOG line 407.*

**0.2 Fix WebSocket auth correctness.** Add `?token=` query-param extraction in `rs/server/src/ws.rs::ws_handler` (constant-time compare against `state.config.api_token`); leave the header path for non-browser clients. The docstring at `auth.rs:5` already promises this — close the loop. ~40 LOC + 3 tests.

**0.3 Bound the watcher.** Add per-line max-size to `rs/src/reader.rs` (default 4 MB; configurable). Make symlink following opt-in (`WalkDir::follow_links(false)` by default; explicit `--follow-symlinks` config flag). Fuzz-style integration test: drop a 100 MB single-line JSONL into a temp watch dir, assert the server doesn't OOM.

**0.4 Bump async-nats past the rustls-webpki CVE.** Walk minor bumps until `rustls-webpki >= 0.103.10` lands transitively. *Already specced in BACKLOG line 543.*

Phase 0 closes every currently-verified exposure plus the WS auth bug that blocks later phases.

### Phase 1 — Identity becomes a boundary at the bus (1.5 weeks)

The `nats-permissions-spike` already laid the rungs out. For Mode B (trusted teammates), per-Person *user/password + subject permissions* are sufficient and already proven to work. For Mode C (adversarial Persons), the spike found a single-account JetStream consumer leak — that means Mode C will need **accounts**, not just permissions. Phase 1 ships the Mode B rung; Phase 5 escalates to accounts.

**1.1 Redesign the NATS subject scheme to encode identity.** Move from `events.{project}.{session}.>` to `events.{person_id}.{principal_id}.{project}.{session}.>`. UUIDs lead — NATS-safe and naturally per-principal. Touch: `rs/core/src/paths.rs::nats_subject_from_path`, consumer setup in `rs/server/src/consumers/*.rs` (subscribe to authorized prefix, not bare `events.>`), stream config in `rs/bus/src/nats_bus.rs::ensure_streams`. Migration: existing JSONL backups carry old subject strings — a one-shot migration rewrites them on first boot of the new build. ~200 LOC including the migration.

**1.2 Per-principal NATS subject permissions.** Plumb user/password (and `.creds` file) into `NatsBus::connect()` per the spike's "Recommended next steps" §1–2. Generate `users { permissions { subscribe = ["events.{person_id}.>", "_INBOX.>"] } }` blocks for `nats.conf` (and `deploy/nats-leaf.conf`) from the Person/Principal directory. A consumer authenticated as Principal X gets `-ERR Permissions Violation` if it tries `SUB events.other-person.>`. ~80 LOC + config templating + rotation docs in `docs/deploy/distributed.md`. *Caveat documented in Phase 5: single-account JetStream consumers can still leak across tenants. Acceptable for Mode B's "trust the teammates" model; not for Mode C.*

**1.3 ViewerCtx + per-Person API filtering (belt-and-suspenders).** Extract a `ViewerCtx` at the auth middleware (today: `api_token` maps to Person via config; later: JWT `sub`). Gate `/api/sessions`, `/api/sessions/{id}/*`, `/api/search`, `/api/agent/search` on Person match. Touch: new `rs/server/src/viewer.rs`, `rs/server/src/api.rs`, `rs/store/src/sqlite_store.rs` (add `person_id` predicate — column already exists). ~300 LOC.

**1.4 Per-client WebSocket filtering.** With 1.1/1.2 in place, the BroadcastConsumer's bus subscription is already prefix-scoped. The connection-level filter is the second layer, against the case where a future feature shares a consumer across principals or a misconfigured ACL leaks. Touch: `rs/server/src/ws.rs`, `rs/server/src/consumers/broadcast.rs`.

**1.5 Person-scoped DELETE.** `DELETE /api/sessions/{id}` returns 403 if `session.person_id != viewer.person_id`. One predicate; the test is the value.

**1.6 Document the policy boundary.** Update `docs/research/personhood-and-principals.md` and `docs/soul/architecture.md` to mark which guarantees are bus-enforced, which API-enforced, and which remain config-only. This is what makes the soul commitment falsifiable.

After Phase 1, OpenStory has its first real authorization boundary, and it lives at the broker.

### Phase 2 — Encryption becomes real (1–2 weeks)

**2.1 SQLCipher on by default.** Build the standard binary with `--features open-story-store/encryption`. Generate a fresh key on first boot, store under `${data_dir}/keystore` with `0600` mode; keep the existing `db_key` config path for explicit override. Validate: `file open-story.db` reports binary, `sqlite3` cannot open it, the `with-key` path can. *Already partially specced in BACKLOG line 458.*

**2.2 JSONL-at-rest encryption (opt-in).** New `rs/store/src/encrypted_jsonl.rs` wrapping `SessionStore` with per-session AEAD (XChaCha20-Poly1305 via `chacha20poly1305` — small audited surface). Key derivation from the master key via HKDF over the session_id. Add a `decrypt-jsonl` CLI subcommand so the sovereignty escape hatch still works. Plaintext JSONL path remains as a fallback for a release cycle; flip the default once it bakes.

**2.3 TLS on HTTP+WS.** Add `axum-server` with `rustls` behind a `[tls]` config section. Optional today; required-when-`host != 127.0.0.1` enforced after a release. Self-signed cert generation script for local dev so the team doesn't disable TLS by reflex.

**2.4 NATS TLS + credentials file.** Add `nats_creds` config path; pass through to `async_nats::ConnectOptions::credentials_file`. Add `nats_tls_required: bool` that fails fast if the broker doesn't negotiate TLS. *Already specced in BACKLOG line 443.* Pair with cert-renewal docs in `docs/deploy/distributed.md`.

**2.5 Fix the JSONL concurrent-write corruption.** Re-architect `SessionStore::append` to hold an exclusive lock across `write + newline`. The "always-on JSONL" promise is currently violated in production by ~273 corrupted lines (BACKLOG: "Malformed JSONL escape hatch"). Encryption layers stack on top — this must work first.

### Phase 3 — Per-Person encryption keys (3–4 days)

This phase was "secrets hygiene via content redaction" in the spike's first draft. That has been retired per Architectural Commitment #1 — OpenStory does not classify content. What remains is the cryptographic half: in Mode B, every machine in the fleet has every event on disk. Phase 2 encrypts the disk with the *master* key. Phase 3 encrypts each Person's events with a *Person-scoped subkey*, so even a fleet machine that holds another Person's events cannot read them.

**3.1 BYOK + per-Person SQLCipher subkey.** Derive a Person-scoped key from the master keystore via HKDF over `person_id`. The store layer carries a `PersonKey` in the `ViewerCtx`. Events tagged with a `person_id` the viewer doesn't hold a key for are structurally unreadable from disk — even if the Phase 1 API gate is bypassed somehow, the payload is ciphertext under a key the requesting context doesn't have.

In Mode B fleet terms: a teammate's leaf node has my events on disk (JetStream propagation), but they're encrypted under my Person key, which their machine doesn't hold. The leaf becomes a *transport replica*, not a *data replica*.

Touch: new `rs/store/src/byok.rs`, modifications to `rs/store/src/sqlite_store.rs` and `rs/store/src/encrypted_jsonl.rs` (from Phase 2). ~250 LOC + key-rotation docs. Already specced in BACKLOG line 678 for the lab.

That's the whole phase. The redact-at-view layer, the `RedactedString` newtype, the `?reveal_secrets=true` audit log — all retired. The architecture answers the secrets question with access control + cryptography, not content inspection.

### Phase 4 — Supply chain & build integrity (1 week)

**4.1 `cargo audit` + `cargo deny` in CI.** Block merges on advisory matches. Configure `deny.toml` with allowlist for known-accepted unfixed issues, with a 7-day SLA on review. Same for `npm audit --production` on the UI.

**4.2 Reproducible builds.** Pin `rust-toolchain.toml`, vendor the `Cargo.lock`, set `--frozen` in CI. Document the recipe.

**4.3 Signed releases.** Tag releases sign with `cosign`; provide SHA256 + signature in release notes. Distribute via `gh release` only — no curl-pipe-bash install path.

**4.4 SBOM.** `cargo sbom` (or `cyclonedx-bom`) emits CycloneDX JSON per release. Attached to GitHub Release. This is the bar most enterprise customers ask for.

**4.5 Skill / hook / MCP supply chain.** The `.claude/skills/` directory and the OpenStory MCP server endpoint are entry points an attacker could poison. Document the trust boundary for the MCP server explicitly (it's local-only by default; if anyone exposes it, they own that). Lint skill files for unexpected shell-out patterns.

### Phase 5 — Lab / multi-tenant hardening (Mode C, 3–6 weeks)

These items are largely already designed in BACKLOG (lines 642-693) — Phase 5 confirms they remain coherent under the threat model above. The cross-cutting change vs the first draft: structural-only anonymization, no content classification.

- **NATS accounts for hard cryptographic isolation.** Per `nats-permissions-spike.md`, single-account JetStream cannot prevent cross-tenant reads when the JetStream API surface is shared. Mode C requires one **account per Person**, each with its own streams. Use NATS's JWT/operator/account chain. The leaf-node Mode B story stays compatible — leaf nodes can bridge between accounts via explicit exports/imports if a workflow legitimately needs cross-Person flow.
- **OIDC via Keycloak** with the static bearer as a local-dev fallback (BACKLOG line 671). Map JWT `sub` → Person.id. Per `rs/server/tests/directory_pluggability.rs`, the pluggability spike already proved Keycloak conformance.
- **Compile-time-enforced *structural* anonymization** via a distinct `AnonymizedWireRecord` type with no `From<WireRecord>` impl — broadcasting a non-anonymized record to a lab-public subscriber is a *compile error*, not a runtime check. The anonymization is deterministic and structural: `cwd`, `host`, `user`, file paths (→ `path_hash:<sha256[..12]>`), project name, MCP server names, branch names, `transcript_path`, `principal_id`. **Content fields (prompts, tool outputs) pass through unmodified** — if the user opted a session into lab-public, they accept that they wrote it. No pattern-based secret scrubbing (Architectural Commitment #1). The user owns the publication decision. BACKLOG line 656 already specs this design; clarification when implementing: drop the `redact_secrets` pass that line mentions, since it conflicts with the content-classification non-goal.
- **Sharing scopes** (`Private`, `TeamRead`, `LabPublic`) with the policy gate in the broadcast and API layers (BACKLOG line 642).
- **Workspace isolation** for BYOK browser-based sessions via Coder + per-Person resource quotas (BACKLOG line 678).
- **Federated lab leaf-node mode** with mutual TLS and audited cross-org event routing (BACKLOG line 693).

---

## Cross-cutting practices

Discipline, not features.

- **Threat-model PR template.** Anything that adds a new endpoint, broadcast variant, or persistence path requires a 3-line threat note in the PR description.
- **Conformance tests as the contract.** The store conformance suite at `rs/store/tests/event_store_conformance.rs` already enforces semantic parity across backends. Add a `security_conformance.rs` suite that asserts: (a) tokens redacted in all log paths; (b) ViewerCtx denial → 403, not 200 with empty body; (c) anonymization is byte-absent of inputs; (d) WebSocket isolation between Persons.
- **Dogfood the audit.** Run a `/cso`-style audit pass at the end of each phase, not just at the start.
- **Sovereignty escape hatch must work.** Every encryption phase ships with a documented + tested plaintext-export path. The user must always be able to walk away with their data.

---

## Non-goals

So the roadmap stays honest about what we are *not* building:

- **We do not classify the content of agent events as "secret" or "non-secret."** Architectural Commitment #1. False positives create false confidence, false negatives miss things, and "what is secret in your work" is not the tool's call to make. The contract is access control + encryption, not content inspection. The only redaction OpenStory ever does is *structural metadata* on user-explicit lab-public opt-in.
- **We are not building an enterprise IAM.** OpenStory federates to an identity provider (Keycloak in Mode C); it does not manage users, groups, or roles internally beyond Person/Principal.
- **We are not encrypting against the user's own machine.** A user with root on their own laptop can read their own SQLite. The threat model is *remote* attackers and *cross-Person* attackers, not the user themselves.
- **We are not building zero-trust networking.** Mode B assumes Tailscale or equivalent; we harden the application layer, not the transport substrate.
- **We do not run a bug bounty yet.** That comes after Phase 4 (signed releases) — it's irresponsible to ask researchers to file reports against an unsigned binary distribution.

---

## Critical files to touch

| File | Phase | What changes |
| --- | --- | --- |
| `rs/cli/src/main.rs`, `rs/bus/src/nats_bus.rs` | 0.1 | `redact_userinfo` helper; apply at every log call site for NATS URLs. |
| `rs/server/src/ws.rs`, `rs/server/src/auth.rs` | 0.2 | Extract `?token=` query param in `ws_handler`; constant-time compare. |
| `rs/src/reader.rs`, `rs/src/watcher.rs` | 0.3 | Line-length cap; symlink follow opt-in. |
| `rs/bus/Cargo.toml` | 0.4 | Bump `async-nats` to clear `rustls-webpki` CVE. |
| `rs/core/src/paths.rs`, `rs/server/src/consumers/*.rs`, `rs/bus/src/nats_bus.rs`, `nats.conf`, `deploy/nats-leaf.conf` | 1.1–1.2 | Subject scheme encodes `person_id` / `principal_id`; per-principal NATS subject perms; stream config updated. |
| `rs/server/src/viewer.rs` (new), `rs/server/src/api.rs`, `rs/server/src/ws.rs`, `rs/server/src/consumers/broadcast.rs` | 1.3–1.5 | ViewerCtx; per-Person filter at API + WS (belt-and-suspenders); DELETE gate. |
| `rs/store/src/sqlite_store.rs`, build config | 2.1 | Default-on SQLCipher; key generation + 0600 keystore. |
| `rs/store/src/encrypted_jsonl.rs` (new), `rs/store/src/persistence.rs` | 2.2, 2.5 | AEAD wrapper; fix concurrent-write corruption. |
| `rs/server/src/main.rs`, server config | 2.3 | `axum-server` + rustls; `[tls]` config section. |
| `rs/bus/src/nats_bus.rs`, deploy configs | 2.4 | `.creds` file path + mTLS toggle. |
| `rs/store/src/byok.rs` (new), `rs/store/src/sqlite_store.rs`, `rs/store/src/encrypted_jsonl.rs` | 3.1 | Per-Person key derivation via HKDF; cross-Person events are ciphertext on fleet machines. |
| `.github/workflows/security.yml` (new) | 4.1, 4.4 | `cargo audit`, `cargo deny`, `npm audit`, SBOM emit. |
| `rs/views/src/anonymize.rs` (new), `rs/views/tests/anonymize_round_trip.rs` | 5 | Distinct `AnonymizedWireRecord` type; structural-only redaction; round-trip absence assertions. |
| `rs/server/src/oidc.rs` (new), `rs/server/src/auth.rs` | 5 | JWT validation against Keycloak JWKS; account-per-Person NATS chain. |

---

## Verification — how we know it worked

Each phase ships its own gate.

**Phase 0:**
- Boot the server, grep stderr for the NATS token: zero hits.
- With `api_token` configured, a fresh browser connects to the dashboard, WebSocket subscribes successfully, events stream.
- Drop a synthetic 200 MB single-line JSONL into the watch dir; server logs a skip + warning, memory stays bounded.
- `cargo audit` exits 0; the GHSA-4cqp-r62p-h3hg alert closes on GitHub.

**Phase 1:**
- **Bus-level isolation:** A NATS consumer authenticated as Principal X issues `SUB events.{other-person-id}.>` and is rejected with `-ERR Permissions Violation`. The denial comes from `nats.conf`, not application code. Verifiable via `nats sub` against a configured server. (Pattern matches the existing tests in `rs/bus/tests/nats_permissions.rs`.)
- **API-level isolation:** Two-Person integration test in `rs/tests/`: Person A's token cannot read Person B's sessions via `/api/sessions/{id}`, `/api/search`, or the WebSocket. All three return 403 / filtered.
- **DELETE across Persons** returns 403.
- **Subject migration:** Existing JSONL backups (old subject strings) replay cleanly into the new scheme; the migration script ships with the PR and has its own test fixture.

**Phase 2:**
- After boot, `file data/open-story.db` reports binary; `sqlite3 data/open-story.db` cannot open without `PRAGMA key`.
- `curl https://localhost:3002/api/sessions` succeeds; `curl http://localhost:3002/api/sessions` is refused when TLS is required.
- NATS leaf node refuses connection from a node without a valid `.creds` file.
- 1000-session concurrent-write test: zero corrupted JSONL lines; `jq` succeeds on every line.

**Phase 3:**
- **Mode B replication test:** a session written by Person A on machine 1 propagates via NATS leaf to a teammate's machine. The teammate opens the SQLite file directly with `sqlcipher` + their own key — read fails (wrong key). Data is on disk but unreadable without Person A's Person-scoped subkey.
- **Key rotation:** rotating Person A's master key re-encrypts the SQLite payloads in place and updates the keystore; the old key no longer decrypts. Fixture exercises rotation mid-session and asserts no event loss.

**Phase 4:**
- `cargo audit` and `cargo deny check` pass in CI on every PR.
- The release artifact has a `.sig` and an SBOM.
- `cosign verify` succeeds against the published cert.

**Phase 5:**
- Two-tenant Keycloak integration test (extend `rs/server/tests/directory_pluggability.rs`): JWT-issued tokens map to Persons; cross-Person access denied at the API.
- **Account-level isolation:** Person A's NATS account cannot subscribe to Person B's account's subjects even via a crafted JetStream consumer (the leak case from `nats-permissions-spike.md` §2). The conformance test from that spike runs against the multi-account configuration and the leak case now *fails to leak*.
- Lab-public anonymization round trip: byte-grep the broadcast frame for `cwd`, `host`, `user`, the project name, and the email of the fixture session — zero hits. Content fields are *unchanged* (intentionally) per the structural-only commitment.

---

## Effort summary

Calibrated against PersonId v1 (~6 commits across store, API, UI, docs).

| Phase | Estimate | Outcome |
| --- | --- | --- |
| 0 — Hotfixes | 1–2 days | All currently-verified exposures closed. |
| 1 — Identity at the bus | 1.5 weeks | Soul commitment falsifiable; isolation enforced by NATS itself for Mode B. |
| 2 — Encryption real | 1–2 weeks | At-rest + in-transit; Mode B viable for sensitive teams. |
| 3 — Per-Person keys | 3–4 days | Cross-Person data on fleet machines is ciphertext. |
| 4 — Supply chain | 1 week | Enterprise procurement bar cleared. |
| 5 — Multi-tenant hardening | 3–6 weeks | Mode C product-ready; accounts close the JetStream leak; structural-only anonymization. |

Phases 0–4 deliver "world-class for self-hosted sovereignty" — the form the project's soul implies. Phase 5 is the lab future; sequence it when the lab demand is real.

---

## Open questions

These are *not* blockers, but answers would re-rank phases:

1. **YC demo target (2026-05-05).** If the YC pitch shows a multi-Person team scenario, Phase 1 + the credential-file portion of Phase 2.4 jump to immediately-after-Phase-0.
2. **Lab roadmap commitment.** If Mode C is committed for 2026 H2, Phase 5 design work (the distinct-type anonymization especially, plus the per-Person NATS accounts) should land alongside Phase 3, since they share infrastructure.
3. **Compliance ask.** No customer has asked for SOC 2 / ISO 27001 yet. If one does before Phase 4, the SBOM + signed-release work moves earlier.

---

## Related research

- `docs/research/nats-permissions-spike.md` — empirical foundation for Phase 1; documents the JetStream consumer leak that motivates Phase 5's accounts requirement. Test harness at `rs/bus/tests/`.
- `docs/research/personhood-and-principals.md` — design of the identity model whose boundary Phases 1 + 3 enforce.
- `docs/research/CONSTELLATION.md` — Mode B distributed deployment context.
- `docs/research/lab/` — Mode C public lab incubation.
- `docs/BACKLOG.md` — sections cited inline (HOTFIX line 407, Distributed Deployment Hardening line 438, End-to-End Encryption line 458, `rustls-webpki` CVE line 543, NATS subject sanitization line 586, lab-public scopes line 642, anonymization line 656, OIDC line 671, BYOK line 678, federated lab line 693).
