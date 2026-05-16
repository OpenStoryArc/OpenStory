# Security Test Harnesses — Validating the Cybersecurity Spike

**Status:** Specification of test harnesses for each phase of `docs/research/cybersecurity-spike.md`. Where a property can be tested deterministically, the harness is specified with code skeletons faithful to the project's existing conventions (hermetic subprocess + `#[tokio::test]`, mirror of `rs/bus/tests/harness/`, `rs/store/tests/event_store_conformance.rs`, `rs/server/tests/directory_pluggability.rs`). Where a property cannot be deterministic — workspace isolation, adversarial-agent runs, red-team exercises — the harness is replaced with a concrete evaluation rubric: what to measure, what counts as pass, and what artifact to produce.

This doc is the *spec*, not the test files. The implementing engineer for each phase ports the relevant skeletons into the tree as failing tests, then makes them pass. Red → green → refactor, per `CLAUDE.md`.

---

## Conventions

**Where harnesses live.**
- Hermetic NATS scenarios: `rs/bus/tests/` (extend `harness/mod.rs` — already proven by `nats_permissions.rs`).
- Cross-crate security conformance: new `rs/tests/security_conformance.rs` mirroring `rs/store/tests/event_store_conformance.rs`. Same pattern: BDD-style helpers, run against any compliant backend.
- Server-side integration: `rs/server/tests/` (extend the `directory_pluggability.rs` pattern for OIDC).
- Build-system / supply-chain: `.github/workflows/security.yml` plus standalone scripts in `scripts/security/`.
- Lab adversarial experiments: `docs/research/lab/security/` (each experiment is a directory with a README + runnable script + an expected-outcome artifact).

**Naming.**
- Rust tests: `phase{N}_{N}_{behavior}` — e.g., `phase1_2_cross_principal_subscribe_is_denied`.
- Lab experiments: `experiment_{N}_{N}_{slug}/`.

**Determinism budget.**
Each test must run in under 30 seconds on a developer laptop, or it lives in the `#[ignore]` slow lane (gated behind a CI tag).

**External dependencies.**
- `nats-server` on PATH for the bus harness.
- `sqlcipher` CLI for the encryption asserts (the binary, not just the lib).
- `cosign` for the release-signature assert.
- `cargo-audit`, `cargo-deny`, `cargo-mutants`, `cargo-fuzz`, `trybuild`, `proptest`, `loom`, `testcontainers` (already used in `rs/tests/helpers/container.rs`).

**CI gating.**
- Phase 0 tests block any PR.
- Phase 1+ tests block PRs that touch the relevant phase's files (path-based filters in the workflow).
- The cross-Person attack matrix (Experiment 2.1) runs nightly against the lab deployment and the result is the security dashboard's headline.

---

## Phase 0 — Hotfix Harnesses (all deterministic)

### 0.1 NATS token redaction

**File:** `rs/bus/src/nats_bus.rs` (inline `#[cfg(test)]` module).

**Spec.** A `redact_userinfo(url)` helper masks the secret between `://` and `@` in any NATS URL, and every log call site uses it.

```rust
#[cfg(test)]
mod redact_tests {
    use super::redact_userinfo;

    #[test]
    fn redacts_token_userinfo() {
        let cases = [
            ("nats://abc123@host:4222",        "nats://<redacted>@host:4222"),
            ("nats://user:pw@host:4222",       "nats://<redacted>@host:4222"),
            ("nats://host:4222",               "nats://host:4222"),          // no userinfo
            ("nats://abc123@h1:4222,h2:4222",  "nats://<redacted>@h1:4222,h2:4222"),
            ("",                               ""),
        ];
        for (input, expected) in cases {
            assert_eq!(redact_userinfo(input), expected, "input: {input}");
        }
    }
}
```

**Plus an integration assertion** in `rs/tests/`:

```rust
#[tokio::test]
async fn phase0_1_no_token_appears_in_boot_logs() {
    let token = "phase0_canary_abc123_DO_NOT_LEAK";
    let server = open_story::test_server()
        .with_nats_url(format!("nats://{token}@127.0.0.1:4222"))
        .capture_stderr()
        .start()
        .await;
    server.wait_for_ready().await;
    let captured = server.stderr_so_far();
    assert!(
        !captured.contains(token),
        "boot log leaked NATS token. Captured: {captured}"
    );
}
```

**Pass criteria.** Both deterministic; both must return zero hits for the canary.

---

### 0.2 WebSocket auth correctness

**File:** `rs/server/tests/ws_auth.rs` (new).

**Spec.** With `api_token` configured, browser WebSocket clients (which cannot send `Authorization` headers on the upgrade) must authenticate via `?token=` query parameter. Existing header path still works for non-browser clients.

```rust
use axum::Router;
use tokio_tungstenite::tungstenite::http::StatusCode;

async fn ws_handshake(uri: &str) -> Result<(), StatusCode> {
    match tokio_tungstenite::connect_async(uri).await {
        Ok(_) => Ok(()),
        Err(tokio_tungstenite::tungstenite::Error::Http(resp))
            => Err(resp.status()),
        Err(e) => panic!("unexpected ws error: {e}"),
    }
}

#[tokio::test]
async fn phase0_2_ws_with_valid_query_token_connects() {
    let app = test_app_with_token("secret").await;
    let port = spawn(app).await;
    ws_handshake(&format!("ws://127.0.0.1:{port}/ws?token=secret"))
        .await
        .expect("valid token should connect");
}

#[tokio::test]
async fn phase0_2_ws_with_wrong_query_token_returns_401() {
    let app = test_app_with_token("secret").await;
    let port = spawn(app).await;
    let err = ws_handshake(&format!("ws://127.0.0.1:{port}/ws?token=WRONG"))
        .await
        .expect_err("wrong token should be rejected");
    assert_eq!(err, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn phase0_2_ws_with_no_token_returns_401_when_required() {
    let app = test_app_with_token("secret").await;
    let port = spawn(app).await;
    let err = ws_handshake(&format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect_err("missing token should be rejected");
    assert_eq!(err, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn phase0_2_ws_with_authorization_header_still_works() {
    // Non-browser clients should keep working via the header path.
    /* connect with Authorization: Bearer secret; assert OK */
}

#[tokio::test]
async fn phase0_2_ws_no_token_configured_passes_through() {
    let app = test_app_with_token("").await;
    let port = spawn(app).await;
    ws_handshake(&format!("ws://127.0.0.1:{port}/ws"))
        .await
        .expect("empty token = no auth");
}
```

**Pass criteria.** Five scenarios all green. The `phase0_2_ws_with_authorization_header_still_works` test is the regression guard — fixing the query-param path must not break the header path.

---

### 0.3 Watcher bounds

**File:** `rs/tests/watcher_bounds.rs` (new).

**Spec.** A JSONL file with a single line above the configured cap is skipped with a warning, not OOMed. Symlink following is opt-in.

```rust
#[tokio::test]
async fn phase0_3_oversized_line_is_skipped_not_oomed() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("oversize.jsonl");
    // 200 MB of 'x' on a single line, no newline.
    let big = "x".repeat(200 * 1024 * 1024);
    std::fs::write(&path, &big).unwrap();

    let start_rss = process_rss();
    let result = open_story_core::reader::read_new_lines(&path, &mut Default::default());
    let end_rss = process_rss();

    assert!(result.is_ok(), "reader should skip, not error");
    let rss_growth = end_rss.saturating_sub(start_rss);
    assert!(
        rss_growth < 50 * 1024 * 1024,
        "RSS grew {} MB on oversize line; cap not enforced",
        rss_growth / 1024 / 1024,
    );
}

#[tokio::test]
async fn phase0_3_symlinks_are_not_followed_by_default() {
    let tmp = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("secret.jsonl");
    std::fs::write(&secret, r#"{"type":"io.arc.event"}"#).unwrap();

    std::os::unix::fs::symlink(&secret, tmp.path().join("link.jsonl")).unwrap();

    let walked: Vec<_> = open_story::watcher::walk(tmp.path()).collect();
    assert!(
        walked.iter().all(|p| !p.ends_with("secret.jsonl")),
        "watcher followed symlink outside watch_dir: {walked:?}"
    );
}
```

**Pass criteria.** RSS growth bounded; symlink target absent from walk. The RSS check is the durable signal — line-count bounds can be tweaked, but unbounded memory growth is the bug.

---

### 0.4 CVE clearance

**Spec.** Not a test — a CI step.

```yaml
# .github/workflows/security.yml
- name: cargo audit
  run: cargo audit --deny warnings
- name: assert rustls-webpki past CVE
  run: |
    cargo tree --invert rustls-webpki | grep -E '0\.10[3-9]\.' \
      || (echo "rustls-webpki still on vulnerable 0.102.x" && exit 1)
```

**Pass criteria.** `cargo audit` exit 0. Dependabot alert closed on GitHub. The grep is belt-and-suspenders — if `cargo audit`'s advisory DB lags, the explicit version check still catches it.

---

## Phase 1 — Identity at the Bus

### 1.1 Subject scheme encodes identity

**File:** `rs/core/tests/paths_subject.rs` (extend existing `subject_*` tests).

```rust
#[test]
fn phase1_1_subject_includes_person_and_principal() {
    let person = uuid::uuid!("01234567-89ab-cdef-0123-456789abcdef");
    let principal = uuid::uuid!("11111111-2222-3333-4444-555555555555");
    let project = "openstory";
    let session = uuid::uuid!("aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee");

    let subject = nats_subject(person, principal, project, session);
    assert_eq!(
        subject,
        format!("events.{person}.{principal}.{project}.{session}")
    );
}

#[test]
fn phase1_1_legacy_subject_migration() {
    // Migration: when a JSONL backup carries the old subject, the migrator
    // rewrites it using the session's persisted person_id/principal_id.
    let old = "events.openstory.abc-session";
    let rewritten = migrate_subject(old, person_id_for("abc-session"), principal_id_for("abc-session"));
    assert!(rewritten.starts_with("events."), "rewritten: {rewritten}");
    assert!(rewritten.contains(&person_id_for("abc-session").to_string()));
}
```

**Plus a JSONL migration fixture** at `rs/store/tests/fixtures/legacy_subjects.jsonl` with a known input set + a known expected post-migration set; the migration test diffs the two.

**Pass criteria.** New scheme deterministic; migration is a pure function tested round-trip.

---

### 1.2 Per-principal NATS ACLs

**File:** `rs/bus/tests/nats_permissions.rs` (extend the existing file — the harness is already there).

```rust
fn person_scoped_auth(person_alpha: Uuid, person_beta: Uuid) -> String {
    format!(r#"
authorization {{
  users = [
    {{
      user: "alpha_user", password: "alpha_pw",
      permissions: {{
        publish: {{ allow: ["events.{person_alpha}.>", "$JS.API.>", "_INBOX.>"] }}
        subscribe: {{ allow: ["events.{person_alpha}.>", "_INBOX.>", "_deliver.>"] }}
      }}
    }}
    {{
      user: "beta_user", password: "beta_pw",
      permissions: {{
        publish: {{ allow: ["events.{person_beta}.>", "$JS.API.>", "_INBOX.>"] }}
        subscribe: {{ allow: ["events.{person_beta}.>", "_INBOX.>", "_deliver.>"] }}
      }}
    }}
  ]
}}
"#)
}

#[tokio::test]
#[ignore]
async fn phase1_2_cross_person_subscribe_is_denied() {
    let alpha = Uuid::new_v4();
    let beta = Uuid::new_v4();
    let server = NatsServer::start(&person_scoped_auth(alpha, beta)).unwrap();

    let alpha_client = connect(&server, "alpha_user", "alpha_pw").await;
    let result = alpha_client
        .subscribe(format!("events.{beta}.>"))
        .await;
    assert!(
        result.is_err(),
        "alpha must be denied subscribe on events.{beta}.>"
    );
    // The error message varies across NATS versions — accept any auth-related failure.
}

#[tokio::test]
#[ignore]
async fn phase1_2_cross_person_publish_is_denied() {
    /* mirror: alpha publishes to events.{beta}.foo, expect publish violation */
}

#[tokio::test]
#[ignore]
async fn phase1_2_same_person_subscribe_succeeds() {
    /* alpha subscribes to events.{alpha}.> — must succeed */
}
```

**Plus the bus crate's `NatsBus::connect`** gains a deterministic unit test that round-trips the user/password and creds-file paths.

**Pass criteria.** Three scenarios green against a real `nats-server` subprocess. Mirrors the existing spike's pattern; the deny outcome is the durable signal regardless of NATS version's wording.

---

### 1.3 ViewerCtx + API filter

**File:** `rs/tests/security_conformance.rs` (new — pattern from `event_store_conformance.rs`).

```rust
async fn two_person_fixture(app: &TestApp) -> (Token, Token) {
    let alpha = app.create_person("alpha").await;
    let beta = app.create_person("beta").await;

    // Seed: each Person has a session with a known marker payload.
    app.ingest_event_for(&alpha, "alpha-marker").await;
    app.ingest_event_for(&beta, "beta-marker").await;

    (alpha.token, beta.token)
}

#[tokio::test]
async fn phase1_3_alpha_cannot_read_betas_sessions() {
    let app = TestApp::start().await;
    let (alpha_token, _) = two_person_fixture(&app).await;

    // Alpha lists sessions — only alpha's session appears.
    let sessions = app.get_json("/api/sessions", &alpha_token).await;
    let ids: Vec<_> = sessions["items"].as_array().unwrap()
        .iter().map(|s| s["session_id"].as_str().unwrap()).collect();
    for id in ids {
        let session = app.get_json(&format!("/api/sessions/{id}"), &alpha_token).await;
        assert_eq!(session["person_id"], app.alpha_id().to_string(),
            "alpha can list a session not belonging to alpha: {session:?}");
    }
}

#[tokio::test]
async fn phase1_3_direct_get_for_betas_session_returns_403() {
    let app = TestApp::start().await;
    let (alpha_token, _) = two_person_fixture(&app).await;
    let beta_session = app.beta_session_id();

    let status = app.get_status(&format!("/api/sessions/{beta_session}"), &alpha_token).await;
    assert_eq!(status, 403);
}

#[tokio::test]
async fn phase1_3_search_does_not_leak_beta_markers() {
    let app = TestApp::start().await;
    let (alpha_token, _) = two_person_fixture(&app).await;

    let results = app.get_json("/api/search?q=beta-marker", &alpha_token).await;
    let hits = results["hits"].as_array().unwrap();
    assert!(hits.is_empty(), "search leaked beta data to alpha: {hits:?}");
}
```

**Pass criteria.** Every endpoint that returns session data has a matching `phase1_3_*` test that asserts cross-Person denial. The conformance pattern means new endpoints fail-loud unless they're added to the suite — same lesson as the store conformance suite.

---

### 1.4 Per-client WebSocket filtering

**File:** `rs/tests/security_conformance.rs` (same file as 1.3).

```rust
#[tokio::test]
async fn phase1_4_ws_isolation_between_persons() {
    let app = TestApp::start().await;
    let (alpha_token, beta_token) = two_person_fixture(&app).await;

    let mut alpha_ws = app.ws_connect(&alpha_token).await;
    let mut beta_ws  = app.ws_connect(&beta_token).await;

    // Drain initial_state frames.
    alpha_ws.drain_initial_state().await;
    beta_ws.drain_initial_state().await;

    app.ingest_event_for(&app.beta(), "beta-live-marker").await;

    let beta_frame = beta_ws.next_frame(Duration::from_secs(2)).await
        .expect("beta should see her own event");
    assert!(beta_frame.contains("beta-live-marker"));

    let alpha_frame = alpha_ws.try_next_frame(Duration::from_millis(500)).await;
    assert!(
        alpha_frame.is_none(),
        "alpha received a frame for beta's event: {alpha_frame:?}",
    );
}
```

**Pass criteria.** Beta sees beta's event within 2s; alpha sees nothing within 500ms. The asymmetric timeout is intentional — if alpha leaks, it leaks fast.

---

### 1.5 Person-scoped DELETE

```rust
#[tokio::test]
async fn phase1_5_delete_other_persons_session_returns_403() {
    let app = TestApp::start().await;
    let (alpha_token, _) = two_person_fixture(&app).await;
    let beta_session = app.beta_session_id();

    let status = app.delete_status(&format!("/api/sessions/{beta_session}"), &alpha_token).await;
    assert_eq!(status, 403);

    // Belt-and-suspenders: the session is still there.
    let still_there = app.get_status(&format!("/api/sessions/{beta_session}"), &app.beta_token()).await;
    assert_eq!(still_there, 200);
}
```

---

### 1.6 Doc completeness (evaluation criteria — not deterministic)

The Phase 1 PR must update `docs/research/personhood-and-principals.md` and `docs/soul/architecture.md`. A test cannot fully grade prose, but we can grade *presence*:

**Eval rubric.**
- [ ] `personhood-and-principals.md` contains a "Guarantees" section.
- [ ] That section lists each Phase 1 guarantee (1.1–1.5) with one of three labels: `bus-enforced`, `api-enforced`, `config-only`.
- [ ] `architecture.md` includes a diagram (or ASCII) showing where the ViewerCtx flows through the request lifecycle.
- [ ] An outside reader who reads only these two docs can answer: *"If I am Person A, what stops me from reading Person B's data?"* with a specific code-path-level answer.

The last bullet is the human-judgment gate. Evaluator: any team member who did not write the implementation. Pass = "yes, I can answer the question without reading the code."

A *lightweight automated check* runs as part of `scripts/check_docs.py` (the existing TDD docs validator): assert each guarantee label appears exactly once and resolves to an existing file path.

---

## Phase 2 — Encryption Real

### 2.1 SQLCipher on by default

**File:** `rs/store/tests/encryption.rs` (new).

```rust
#[tokio::test]
async fn phase2_1_db_file_is_binary_encrypted() {
    let dir = tempfile::tempdir().unwrap();
    let _store = SqliteStore::with_generated_key(dir.path()).unwrap();

    let path = dir.path().join("open-story.db");
    let bytes = std::fs::read(&path).unwrap();
    let header = &bytes[..16];
    assert_ne!(
        header, b"SQLite format 3\0",
        "DB header is plaintext SQLite — encryption not engaged",
    );
}

#[tokio::test]
async fn phase2_1_sqlite_cli_cannot_open_without_key() {
    let dir = tempfile::tempdir().unwrap();
    let _store = SqliteStore::with_generated_key(dir.path()).unwrap();

    let output = std::process::Command::new("sqlite3")
        .arg(dir.path().join("open-story.db"))
        .arg(".tables")
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "vanilla sqlite3 opened the encrypted DB — encryption not engaged",
    );
}

#[tokio::test]
async fn phase2_1_sqlcipher_with_key_can_open() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::with_generated_key(dir.path()).unwrap();
    let key = store.key_for_test();

    let output = std::process::Command::new("sqlcipher")
        .arg(dir.path().join("open-story.db"))
        .arg(format!("PRAGMA key = '{key}'; SELECT count(*) FROM events;"))
        .output()
        .unwrap();
    assert!(output.status.success(), "sqlcipher should open with the right key");
}

#[tokio::test]
async fn phase2_1_keystore_is_0600() {
    let dir = tempfile::tempdir().unwrap();
    let _store = SqliteStore::with_generated_key(dir.path()).unwrap();

    let keystore = dir.path().join("keystore");
    let meta = std::fs::metadata(&keystore).unwrap();
    use std::os::unix::fs::PermissionsExt;
    let mode = meta.permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "keystore mode is {mode:o}, want 600");
}
```

**Pass criteria.** All four green. The CLI-cannot-open test is the canonical signal — if a hostile reader with shell access can't read the file, encryption is real.

---

### 2.2 JSONL AEAD round-trip

```rust
#[tokio::test]
async fn phase2_2_encrypted_jsonl_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let key = master_key_for_test();
    let store = EncryptedJsonlStore::new(dir.path(), &key);

    let session_id = uuid::Uuid::new_v4();
    let evt = sample_event();
    store.append(session_id, &evt).await.unwrap();

    // Raw file on disk is not plaintext JSON.
    let path = dir.path().join(format!("{session_id}.jsonl.enc"));
    let raw = std::fs::read(&path).unwrap();
    assert!(serde_json::from_slice::<serde_json::Value>(&raw).is_err(),
        "encrypted JSONL parses as JSON — not encrypted");

    // Decryption recovers the event byte-equal.
    let decrypted = store.read_all(session_id).await.unwrap();
    assert_eq!(decrypted, vec![evt]);
}

#[tokio::test]
async fn phase2_2_decrypt_jsonl_cli_works() {
    /* shell out to the new `open-story decrypt-jsonl` subcommand;
       assert it emits valid plaintext JSONL that round-trips with `jq`. */
}
```

**Pass criteria.** Round-trip; encrypted bytes don't parse as JSON; CLI escape hatch works.

---

### 2.3 TLS on HTTP+WS

```rust
#[tokio::test]
async fn phase2_3_https_succeeds_http_refused_when_required() {
    let app = TestApp::start_with_tls(/*tls_required=*/true).await;

    // HTTPS works.
    let https = reqwest::Client::builder()
        .danger_accept_invalid_certs(true) // self-signed in test
        .build().unwrap();
    let resp = https.get(format!("https://127.0.0.1:{}/api/sessions", app.port))
        .send().await.unwrap();
    assert_eq!(resp.status(), 200);

    // HTTP refused or redirected.
    let http_resp = reqwest::Client::new()
        .get(format!("http://127.0.0.1:{}/api/sessions", app.port))
        .send().await;
    assert!(
        http_resp.is_err() || matches!(http_resp.unwrap().status().as_u16(), 301 | 308 | 400),
        "plain HTTP should be refused or redirected when TLS is required",
    );
}
```

---

### 2.4 NATS TLS + creds

Extend the existing `harness/mod.rs` to template a TLS-enabled config. Add:

```rust
#[tokio::test]
#[ignore]
async fn phase2_4_nats_without_creds_fails() {
    let server = NatsServer::start_tls(&tls_required_auth()).unwrap();
    let result = async_nats::ConnectOptions::new() // no creds
        .connect(server.url())
        .await;
    assert!(result.is_err(), "TLS-required broker should reject no-creds connect");
}

#[tokio::test]
#[ignore]
async fn phase2_4_nats_with_valid_creds_succeeds() { /* ... */ }
```

---

### 2.5 JSONL concurrent-write corruption

**File:** `rs/store/tests/persistence_concurrency.rs` (new).

```rust
#[tokio::test]
async fn phase2_5_concurrent_appends_produce_valid_jsonl() {
    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::new(dir.path());

    let session_id = uuid::Uuid::new_v4();
    let n_writers = 16;
    let events_per_writer = 200;

    let mut handles = Vec::new();
    for w in 0..n_writers {
        let store = store.clone();
        handles.push(tokio::spawn(async move {
            for i in 0..events_per_writer {
                let evt = sample_event_with_marker(format!("w{w}-i{i}"));
                store.append(session_id, &evt).await.unwrap();
            }
        }));
    }
    for h in handles { h.await.unwrap(); }

    // Every line must parse as JSON. Any line that doesn't = corruption.
    let path = dir.path().join(format!("{session_id}.jsonl"));
    let content = std::fs::read_to_string(&path).unwrap();
    let total = content.lines().count();
    let valid = content.lines()
        .filter(|l| serde_json::from_str::<serde_json::Value>(l).is_ok())
        .count();
    assert_eq!(valid, total, "{} of {} JSONL lines are corrupted", total - valid, total);
    assert_eq!(total, n_writers * events_per_writer);
}
```

**Pass criteria.** Zero corrupted lines out of 3200 concurrent writes. This is the regression test for the BACKLOG "Malformed JSONL escape hatch" bug.

---

## Phase 3 — Per-Person Encryption Keys

### 3.1 BYOK Mode B replication isolation

This is the architectural payoff of Phase 3. The test models the Mode B scenario: a fleet machine receives another Person's events but cannot read them.

**File:** `rs/tests/byok_replication.rs` (new). Uses `testcontainers` per the existing pattern in `rs/tests/helpers/container.rs`.

```rust
#[tokio::test]
async fn phase3_1_cross_person_data_on_disk_is_ciphertext() {
    // Two OpenStory instances sharing a NATS hub — simulates Mode B fleet.
    let hub = start_nats_hub().await;
    let alpha_node = open_story::start(/*person=*/"alpha", /*hub=*/&hub).await;
    let beta_node  = open_story::start(/*person=*/"beta",  /*hub=*/&hub).await;

    // Alpha writes a session with a known marker.
    alpha_node.ingest_local("alpha-private-marker").await;

    // Wait for JetStream propagation. (Bounded poll, not arbitrary sleep —
    // follows the project's eventual-consistency convention.)
    poll_until(Duration::from_secs(5), || async {
        beta_node.db_contains_session_for_person("alpha").await
    }).await.expect("propagation");

    // Beta's machine has alpha's session row on disk.
    let beta_db_path = beta_node.db_path();

    // Open the file directly with sqlcipher + beta's key.
    let output = std::process::Command::new("sqlcipher")
        .arg(&beta_db_path)
        .arg(format!(
            "PRAGMA key = '{}'; SELECT payload FROM events WHERE session_id = '{}';",
            beta_node.master_key(),
            alpha_node.session_id(),
        ))
        .output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        !stdout.contains("alpha-private-marker"),
        "beta read alpha's plaintext from disk with her own master key — Person subkey not engaged",
    );

    // And beta's API also denies it (Phase 1.3 belt-and-suspenders).
    let status = beta_node.api_get_status(
        &format!("/api/sessions/{}", alpha_node.session_id()),
        beta_node.api_token(),
    ).await;
    assert_eq!(status, 403);
}
```

**Pass criteria.** Two checks: (a) marker absent from disk-level read with the wrong Person subkey; (b) API denies at 403. Either alone is partial; both together is the guarantee.

---

### 3.2 Key rotation

```rust
#[tokio::test]
async fn phase3_2_rotation_reencrypts_in_place_and_invalidates_old_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = SqliteStore::with_generated_key(dir.path()).unwrap();
    let person = Uuid::new_v4();

    // Seed with events.
    for i in 0..100 {
        store.put_event(&fixture_event_for_person(person, i)).await.unwrap();
    }

    let old_key = store.person_key(person);
    store.rotate_person_key(person).await.unwrap();
    let new_key = store.person_key(person);
    assert_ne!(old_key, new_key);

    // New key reads succeed.
    let read = store.read_events_for_person(person, &new_key).await.unwrap();
    assert_eq!(read.len(), 100);

    // Old key reads fail.
    let fail = store.read_events_for_person(person, &old_key).await;
    assert!(fail.is_err(), "old key still decrypts after rotation");
}

#[tokio::test]
async fn phase3_2_rotation_mid_session_loses_no_events() {
    // Rotate while an ingest is in flight; assert no event vanishes,
    // no event is duplicated.
}
```

**Pass criteria.** Atomic rotation; no event loss under concurrent ingest.

---

## Phase 4 — Supply Chain & Build Integrity

### 4.1 `cargo audit` + `cargo deny` in CI

```yaml
# .github/workflows/security.yml
jobs:
  audit:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: rustsec/audit-check@v2
        with:
          token: ${{ secrets.GITHUB_TOKEN }}
      - run: cargo install --locked cargo-deny
      - run: cargo deny check
  npm-audit:
    runs-on: ubuntu-latest
    defaults: { run: { working-directory: ui } }
    steps:
      - uses: actions/checkout@v4
      - run: npm ci
      - run: npm audit --audit-level=moderate --production
```

**Pass criteria.** Both jobs green on every PR. `deny.toml` allowlist requires a 7-day SLA review entry per exception.

---

### 4.2 Reproducible builds (eval criteria — partial determinism)

Bit-identical Rust binaries are hard to guarantee across machines (build timestamps, paths, etc.). The pragmatic check is *reproducibility on the same machine within a sandbox*.

**Eval rubric.**
- [ ] Build the release binary twice from a clean checkout. Compare SHA256 — match.
- [ ] Build on machine A and machine B using the same Docker base image. Compare SHA256 — match.
- [ ] If mismatch, the diff is enumerable to ≤3 known sources (e.g. `LC_ALL`, `SOURCE_DATE_EPOCH`, build path). Document each.

**Concrete check** (`scripts/security/check_reproducible.sh`):

```bash
#!/usr/bin/env bash
set -euo pipefail
docker build --no-cache -f Dockerfile -t openstory:repro-1 .
docker build --no-cache -f Dockerfile -t openstory:repro-2 .
H1=$(docker run --rm openstory:repro-1 sha256sum /usr/local/bin/open-story | awk '{print $1}')
H2=$(docker run --rm openstory:repro-2 sha256sum /usr/local/bin/open-story | awk '{print $1}')
[ "$H1" = "$H2" ] || { echo "non-reproducible build: $H1 != $H2"; exit 1; }
echo "reproducible: $H1"
```

**Pass criteria.** Two builds, same hash. Failure is informative — document which sources of nondeterminism are accepted.

---

### 4.3 Signed releases

```yaml
- name: cosign sign release artifact
  run: cosign sign-blob --yes ./dist/open-story-${{ github.ref_name }}.tar.gz \
       --output-signature ./dist/open-story-${{ github.ref_name }}.tar.gz.sig
- name: cosign verify
  run: cosign verify-blob ./dist/open-story-${{ github.ref_name }}.tar.gz \
       --signature ./dist/open-story-${{ github.ref_name }}.tar.gz.sig \
       --certificate-identity-regexp '.*openstory.*' \
       --certificate-oidc-issuer-regexp '.*'
```

**Pass criteria.** `cosign verify-blob` exit 0 in release CI; the verification step is documented in the release notes so users can run it themselves.

---

### 4.4 SBOM

```yaml
- name: emit SBOM
  run: cargo cyclonedx --format json -o dist/sbom.json
- name: validate SBOM schema
  run: |
    pip install cyclonedx-bom
    cyclonedx-cli validate --input-file dist/sbom.json
```

**Pass criteria.** SBOM emitted; schema-valid; attached to the GitHub Release as an asset.

---

### 4.5 Skill / MCP supply chain (mixed deterministic + eval)

**Deterministic part:** lint skill files.

```rust
// rs/server/tests/skill_lint.rs
#[test]
fn phase4_5_skills_do_not_exec_unexpected_shells() {
    let skills_dir = std::path::Path::new(".claude/skills");
    let suspicious = vec![
        regex::Regex::new(r"curl\s+[^|]*\|\s*(bash|sh)").unwrap(),
        regex::Regex::new(r"eval\s*\(").unwrap(),
        regex::Regex::new(r"\$\(.*\$\(").unwrap(),  // nested command substitution
    ];
    for entry in walkdir::WalkDir::new(skills_dir).into_iter().flatten() {
        if entry.path().extension().map_or(false, |e| e == "md") {
            let content = std::fs::read_to_string(entry.path()).unwrap();
            for pat in &suspicious {
                assert!(!pat.is_match(&content),
                    "suspicious pattern in skill {}: {pat:?}", entry.path().display());
            }
        }
    }
}
```

**Evaluation part:** the MCP server's trust boundary is documented.

**Eval rubric.**
- [ ] `docs/security/mcp-trust-model.md` exists.
- [ ] It explicitly states: the MCP server is local-only by default; exposing it beyond localhost is the operator's risk; the surface is read-only.
- [ ] Each MCP tool is enumerated with its access pattern (read-only, write, executes commands).
- [ ] A reader who installs a skill can answer: *"what can this skill see of my session data?"*

---

## Phase 5 — Multi-Tenant Lab Hardening

### 5.1 NATS accounts close the JetStream leak (THE flagship metric)

This is the test that defines "Mode C is shippable." The existing `nats-permissions-spike.md` proved the leak exists under single-account. Phase 5's job is to make the same attack *fail to leak*.

**File:** `rs/bus/tests/nats_accounts.rs` (new).

```rust
fn per_person_accounts_auth(alpha: Uuid, beta: Uuid) -> String {
    format!(r#"
accounts {{
  ALPHA: {{
    users: [{{ user: "alpha_user", password: "alpha_pw" }}]
    jetstream: {{ store_dir: "/tmp/openstory-acct-alpha" }}
  }}
  BETA: {{
    users: [{{ user: "beta_user", password: "beta_pw" }}]
    jetstream: {{ store_dir: "/tmp/openstory-acct-beta" }}
  }}
}}
"#)
}

#[tokio::test]
#[ignore]
async fn phase5_1_jetstream_consumer_leak_fails_under_accounts() {
    // Same attack as nats_permissions.rs::single_account_jetstream_consumer_leak,
    // but against the per-account config. The attack must now FAIL.
    let alpha = Uuid::new_v4();
    let beta = Uuid::new_v4();
    let server = NatsServer::start(&per_person_accounts_auth(alpha, beta)).unwrap();

    // Admin (impossible without operator role, but simulate with separate connect)
    // seeds a beta-tagged event.
    seed_beta_event(&server, beta, b"beta-private").await;

    // Alpha attempts the cross-tenant consumer attack.
    let alpha_client = connect(&server, "alpha_user", "alpha_pw").await;
    let alpha_js = jetstream::new(alpha_client);

    let consumer_result = alpha_js
        .get_or_create_stream(js_stream::Config { name: "events".into(), ..Default::default() })
        .await;
    // In a multi-account world, alpha simply does not see beta's stream.
    assert!(
        consumer_result.is_err() || stream_is_empty_for_alpha(consumer_result.unwrap()).await,
        "alpha must not see beta's JetStream stream across accounts",
    );

    // Even if alpha tries a pull consumer with filter_subject = "events.{beta}.>"
    // — the attack from the spike — there is no JetStream stream in alpha's
    // account that carries beta's data, so the read returns empty.
}
```

**Pass criteria.** The exact attack that proved the leak in `nats_permissions.rs::single_account_jetstream_consumer_leak` now produces no leaked bytes when run against the accounts configuration. The headline metric for the lab dashboard.

---

### 5.2 OIDC / Keycloak

**File:** `rs/server/tests/oidc.rs` (new — extends `directory_pluggability.rs` pattern; the spike already proved Keycloak works).

```rust
#[tokio::test]
#[ignore]
async fn phase5_2_jwt_sub_claim_maps_to_person() {
    let keycloak = KeycloakContainer::start().await;
    let token = keycloak.issue_jwt("user-alpha").await;

    let app = TestApp::start_with_oidc(&keycloak).await;
    let response = app.get_json("/api/sessions", &token).await;
    assert_eq!(response["viewer"]["person_id"], app.person_id_for("user-alpha"));
}

#[tokio::test]
#[ignore]
async fn phase5_2_forged_jwt_with_wrong_signing_key_rejected() {
    let keycloak = KeycloakContainer::start().await;
    let bad_token = forge_jwt_with_random_key("user-alpha");

    let app = TestApp::start_with_oidc(&keycloak).await;
    let status = app.get_status("/api/sessions", &bad_token).await;
    assert_eq!(status, 401);
}
```

---

### 5.3 Type-system anonymization

**File:** `rs/views/tests/anonymize_compile_fail/` (`trybuild` cases).

```rust
// rs/views/tests/anonymize_compile_fail.rs
#[test]
fn phase5_3_anonymized_record_cannot_be_constructed_from_wire_record() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/anonymize_compile_fail/*.rs");
}
```

```rust
// tests/anonymize_compile_fail/no_from_impl.rs
use open_story_views::{WireRecord, AnonymizedWireRecord};

fn main() {
    let wire: WireRecord = sample();
    // Must fail to compile — there is no From<WireRecord> for AnonymizedWireRecord.
    let _anon: AnonymizedWireRecord = wire.into();
}
```

**Pass criteria.** `cargo test` for `anonymize_compile_fail.rs` passes — i.e., the bad code *fails to compile* as expected. If someone adds a `From<WireRecord> for AnonymizedWireRecord` impl, the test goes red. This is the type-system guarantee, asserted by the test suite.

---

### 5.4 Anonymization byte-absence

```rust
#[test]
fn phase5_4_anonymized_record_has_no_sensitive_substrings() {
    let session = SessionFixture::with_sensitive_fields(SensitiveFixture {
        cwd: "/Users/alice/secrets",
        host: "alice-laptop.local",
        user: "alice",
        file_path: "/Users/alice/.ssh/id_rsa",
        project_name: "alice-private-project",
        email: "alice@example.com",
    });

    let anon = anonymize_for_lab_public(&session);
    let serialized = serde_json::to_string(&anon).unwrap();

    let must_not_appear = [
        "/Users/alice/secrets",
        "alice-laptop.local",
        "alice",            // user
        "id_rsa",
        "alice-private-project",
        "alice@example.com",
    ];
    for needle in &must_not_appear {
        assert!(
            !serialized.contains(needle),
            "anonymized record leaks `{needle}`: {serialized}"
        );
    }
}
```

**Pass criteria.** No structural field of the fixture appears anywhere in the serialized output. Content fields (prompts, tool outputs) pass through unmodified per Architectural Commitment #1 — that's tested *separately*:

```rust
#[test]
fn phase5_4_content_fields_pass_through_unmodified() {
    let session = SessionFixture::with_content("here's an api key: sk-ant-EXAMPLE");
    let anon = anonymize_for_lab_public(&session);
    let serialized = serde_json::to_string(&anon).unwrap();
    assert!(serialized.contains("sk-ant-EXAMPLE"),
        "anonymizer attempted to classify content as secret — violates Commitment #1");
}
```

The second test is the most important Phase 5 invariant: the anonymizer **must not** pattern-match for secrets. This is how we prove we kept our commitment.

---

### 5.5 Sharing scopes

```rust
#[tokio::test]
async fn phase5_5_lab_public_scope_only_broadcasts_via_anonymized_path() {
    let app = TestApp::start_with_lab_mode().await;
    let alpha = app.create_person("alpha").await;

    // Default scope is private — no lab subscriber receives it.
    let mut lab_ws = app.lab_public_ws().await;
    app.ingest_event_for(&alpha, "private-marker").await;
    assert!(lab_ws.try_next_frame(Duration::from_millis(500)).await.is_none());

    // Mark the session LabPublic.
    app.set_scope(alpha.session_id(), Scope::LabPublic).await;
    app.ingest_event_for(&alpha, "labpublic-marker").await;
    let frame = lab_ws.next_frame(Duration::from_secs(2)).await.unwrap();
    assert!(frame.contains("labpublic-marker"));
    // And it came through anonymized.
    assert!(!frame.contains(alpha.host()));
}
```

---

### 5.6 Workspace isolation (evaluation criteria — depends on Coder)

Workspace isolation depends on Coder + Kubernetes namespaces, which are operational concerns, not Rust testable. The eval is run against a deployed lab instance.

**Eval rubric.**
- [ ] A Coder workspace for Person A cannot reach (network) a workspace for Person B without going through the OpenStory API (verified by `kubectl exec` + `curl` from inside the workspace).
- [ ] A workspace cannot escape its CPU/memory/wall-clock budget (verified by a stress fixture that tries to exceed each).
- [ ] BYOK API key is present in the workspace at session boot, absent from all logs (`kubectl logs -n {ns} --tail=-1 | grep $KEY` returns no hits), absent from environment dumps in deliverables.
- [ ] On session end, the workspace is destroyed and the volume reclaimed within 60s.

The rubric runs as a scripted check (`docs/research/lab/security/workspace_isolation.sh`) against the live lab.

---

### 5.7 Federated lab (evaluation criteria — multi-host)

Cross-org leaf-node federation requires multiple Hetzner hosts; not deterministically reproducible in CI without provisioning real infra.

**Eval rubric.**
- [ ] Leaf node from Org A authenticated by mutual TLS; Org B's CA rejects Org A's cert without an explicit trust import.
- [ ] Events from Org A's hub do not appear on Org B's hub unless an explicit `imports` block exists.
- [ ] Audit log of every cross-org event flow exists and is queryable.
- [ ] Run the cross-Person attack matrix (Experiment 2.1) across the federation; same pass criteria as single-hub.

---

## Layer 2 — Adversarial Experiments (rubrics + scripted runs)

### Cross-Person attack matrix

**Where:** `docs/research/lab/security/experiment_2_1_cross_person_matrix/`.

**Script:** `run.sh` provisions two Persons, runs each attack vector, records outcome JSON.

**Output artifact:** `results.json` per run.

```json
{
  "run_id": "2026-06-01T12:00:00Z",
  "config": "mode_b_v1",
  "attacks": [
    { "vector": "api_direct",          "expected": "403",    "actual": "403",    "pass": true },
    { "vector": "forged_jwt",          "expected": "401",    "actual": "401",    "pass": true },
    { "vector": "ws_subscribe",        "expected": "filtered", "actual": "filtered", "pass": true },
    { "vector": "nats_raw_sub",        "expected": "denied", "actual": "denied", "pass": true },
    { "vector": "nats_js_filter_sub",  "expected": "denied", "actual": "LEAK",   "pass": false, "phase_to_close": 5 },
    { "vector": "disk_read_with_master_key", "expected": "ciphertext", "actual": "ciphertext", "pass": true },
    { "vector": "mongo_direct",        "expected": "ciphertext", "actual": "ciphertext", "pass": true },
    { "vector": "process_introspection", "expected": "denied", "actual": "denied", "pass": true }
  ],
  "summary": { "total": 8, "pass": 7, "fail": 1, "headline_blocking_phase": 5 }
}
```

**Pass criteria.** Mode B: 7 of 8 rows green; the `nats_js_filter_sub` row is allowed to fail until Phase 5 ships. Mode C readiness: all 8 green. The `summary.headline_blocking_phase` value is the public dashboard metric — "Mode C blocked on Phase 5."

---

### Adversarial agent

**Eval rubric** (not deterministic — agents are non-deterministic by design).

- **Setup.** Coder workspace + Claude Code, fixed model + max-token budget, prompt: *"You are red-teaming OpenStory in the lab. Person Beta has a session at /api/sessions/{id}. Read its contents."* Time-box 4 hours. Sandboxed network (lab-scope only). No human assistance.
- **Outcome buckets.**
  - **Bucket A — Agent succeeds via known vector.** Add the vector to the cross-Person matrix; close as a regression test.
  - **Bucket B — Agent succeeds via novel vector.** Highest-value outcome: bounty + write-up + regression test + phase reassessment.
  - **Bucket C — Agent fails after exhausting budget.** Catalog every attempted vector; each vector becomes a row in the matrix.
- **Metrics tracked.**
  - Attempts per hour
  - Distinct vectors tried
  - Time-to-first-attempt-against-NATS (proxy for "did the agent figure out the architecture")
  - Time-to-give-up (per-vector and overall)
  - Tokens spent (cost-to-attack)
- **Cadence.** Once per quarter, plus before any major release. Results published to the lab dashboard.

---

### Race conditions

**File:** `rs/server/tests/loom_security.rs` (gated behind `cfg(loom)`).

```rust
#[test]
#[cfg(loom)]
fn phase1_toctou_session_person_id_cannot_change_under_auth_check() {
    loom::model(|| {
        // Two threads: one rotates a session's person_id, one performs an auth check.
        // Assert the auth check either sees the old value and denies, or sees the
        // new value and allows — never half-and-half.
    });
}
```

**Pass criteria.** `loom` model exhaustively explores interleavings and finds no violating order. Slow — runs on a nightly schedule, not per-PR.

---

### Supply-chain dry run

**Script:** `scripts/security/supply_chain_drill.sh`.

```bash
#!/usr/bin/env bash
set -euo pipefail
# Three attack simulations, each in a sandboxed branch.
for attack in typosquat dep_confusion malicious_version_bump; do
  git checkout -b drill-$attack
  ./scripts/security/inject_$attack.sh
  if cargo audit --deny warnings && cargo deny check; then
    echo "DRILL FAILED: $attack was not caught"; exit 1
  fi
  git checkout - && git branch -D drill-$attack
done
echo "DRILL PASSED: all 3 attacks caught"
```

**Cadence.** Once per quarter.

---

### Skill / MCP poisoning

**Eval rubric.**

- [ ] Drop a skill that attempts to enumerate sessions belonging to other Persons via the MCP server. Verify it cannot.
- [ ] Drop a skill that attempts to write to the OpenStory data dir. Verify the trust boundary is documented and the access denied (if denied) or expected (if not — document it as a known trust assumption).
- [ ] Drop a skill that exfiltrates `~/.claude/projects/` to a remote endpoint. Verify network egress is constrained in the lab (or documented as user's responsibility).

---

### Red-team week

**Cadence.** Once per quarter, set aside a full week. Team members rotate; once a year, hire an external tester.

**Deliverable per week.**
- A `red-team-${YYYY-QN}.md` report in `docs/research/lab/security/red-team-reports/`.
- Each finding tagged with: severity (S0–S3), affected phase, regression test added (link), time-to-detect (hours from intrusion to detection in OpenStory's own data).
- `summary.time_to_detect_p50` and `summary.time_to_detect_p90` go to the lab dashboard.

---

## Maturity Ladder

Not all of this lands on day one.

| Tier | What | Cost |
| --- | --- | --- |
| **T1 — Continuous** | Phase 0–4 deterministic harnesses in CI on every PR. | Folds into each phase's work. |
| **T2 — Scheduled** | Phase 5 harnesses; cross-Person matrix nightly; loom on nightly; supply-chain drill quarterly. | 1 week of CI / scheduler setup. |
| **T3 — Lab observatory** | Cross-Person matrix + headline metrics published to a public dashboard; results.json artifacts versioned in the repo. | 1–2 weeks. |
| **T4 — Adversarial** | Quarterly adversarial agent runs + red-team week + external bounty against the lab. | Ongoing operational cost. |

T1 is the minimum bar for the YC pitch. T3 is what makes the security claims *outsider-verifiable*. T4 is what world-class actually looks like.

---

## Related research

- `docs/research/cybersecurity-spike.md` — the roadmap these harnesses validate.
- `docs/research/nats-permissions-spike.md` — the empirical foundation for Phases 1.2 and 5.1; existing harness pattern at `rs/bus/tests/harness/`.
- `docs/research/personhood-and-principals.md` — the identity model whose enforcement these harnesses verify.
- `rs/store/tests/event_store_conformance.rs` — the pattern the new `rs/tests/security_conformance.rs` mirrors.
- `rs/server/tests/directory_pluggability.rs` — the pattern Phase 5.2's OIDC test extends.
