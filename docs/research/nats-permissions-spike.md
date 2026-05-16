# NATS Permissions Spike

**Status:** Spike complete. Test harness in `rs/bus/tests/` (`harness/mod.rs` + `nats_permissions.rs`).

## Question

How can we add permissions to OpenStory's pub/sub layer so that different identities can only publish/subscribe to certain subject patterns (e.g. one project's `events.{project}.>` space)?

## Current state

- `NatsBus::connect()` only knows token-in-URL auth (`nats://TOKEN@host:port`).
- `nats.conf` has no `authorization` block — the bus is wide open.
- `IngestBatch` already carries `project_id` and `session_id`, so the natural tenant token exists in the envelope; nothing on the bus uses it for access control today.
- The HTTP API has a single shared bearer token (all-or-nothing). NATS pub/sub has nothing.

## Findings

### 1. User/password + per-subject permissions works for the data plane

`async-nats` supports user/password via `ConnectOptions::with_user_and_password`. nats-server's `authorization { users = [...] }` block with `permissions { publish.allow, subscribe.allow }` enforces both directions on subject patterns (`*` single token, `>` multi-token).

The harness proves three deny cases against a live `nats-server` subprocess:
- Wrong password → connect fails with `authentication error`.
- Cross-tenant **publish** via JetStream (which awaits an ack) → ack never arrives, server logs `Publish Violation`.
- Cross-tenant **core subscribe** → server logs `Subscription Violation`, no messages delivered.

Each user only needs:
```hocon
publish:   { allow: ["events.{tenant}.>", "$JS.API.>", "_INBOX.>"] }
subscribe: { allow: ["events.{tenant}.>", "_INBOX.>", "_deliver.>"] }
allow_responses: true
```

### 2. Single-account JetStream consumers leak across tenants — *the headline finding*

With one nats-server account and per-user permissions, a tenant who has `$JS.API.>` (which they need to use JetStream at all) **can read another tenant's data** by creating a pull consumer with a cross-tenant `filter_subject`. The server delivers matching messages on the consumer's reply inbox, which the tenant *is* authorized for. Subject-level subscribe permissions don't gate JetStream consumer reads.

The test `single_account_jetstream_consumer_leak` demonstrates this: `alpha_user` (subscribe perm only on `events.alpha.>`) creates a pull consumer filtered to `events.beta.>` and successfully reads `b"beta-secret"`. The test passing today encodes the leak; if it ever fails, the underlying behavior changed and we should re-evaluate.

### 3. Hard tenant isolation requires accounts, not permissions

NATS accounts are isolated subject spaces — JetStream lives per-account, no shared streams unless explicitly imported/exported. The official guidance is "more accounts with few clients" over "one account with complex permissions." If OpenStory ever needs hard multi-tenant isolation, **accounts are the right rung**, not user/password.

## Rungs in escalation order

| Rung | When | Cost |
|---|---|---|
| **Token in URL** (today) | Single-user dev | Built-in; no isolation |
| **User/password + permissions** | Trust the tenants but want soft scoping; data-plane perms fine | ~15 lines of config; `async-nats` already supports it |
| **NKEYs** | No shared secret on disk | Same model, ed25519 instead of password |
| **JWT + accounts** | Hard tenant isolation; external mints users | Operator/account/user trust chain; resolver setup |
| **Auth callout** | Identities live in external IDP | Bridge service to mint claims |

Skip mTLS-only unless we already operate cert PKI. Skip JWT until we need decentralized minting.

## Recommended next steps

If OpenStory wants soft scoping (single-tenant deployments, dev environments):

1. Plumb `user`/`password` (and `creds_file` for completeness) into `NatsBus::connect()` alongside the existing token path. The async-nats API is a one-liner in each branch.
2. Plumb the same fields through `Config` (toml + env vars) — `nats_user`, `nats_password`.
3. Generate a per-tenant `authorization` block from a small Rust helper (`tests/nats_permissions.rs::two_tenant_auth` is a starting template).
4. Document that data-plane perms protect direct pub/sub but not JetStream consumer reads — single-account is suitable for "trust the operators" scenarios, not adversarial multi-tenancy.

If/when OpenStory needs hard multi-tenant isolation:

1. Move to one **account per tenant**. Each tenant gets its own `events`/`patterns`/`changes` streams, fully isolated.
2. Set up a memory or NATS-resolver-backed operator/account/user JWT chain.
3. Each tenant's `NatsBus` instance connects with that tenant's user `.creds`. The Rust code shape barely changes — `ConnectOptions::credentials_file(path)` instead of `with_user_and_password`.
4. Add an account-level export/import only if a workflow legitimately needs cross-tenant flow (e.g. an admin observability feed).

## Test harness

- `rs/bus/tests/harness/mod.rs` — spawns `nats-server` subprocess with templated auth config, free port, JetStream store, cleanup on drop.
- `rs/bus/tests/nats_permissions.rs` — 5 scenarios marked `#[ignore]`. Run with:
  ```
  cargo test -p open-story-bus --test nats_permissions -- --ignored
  ```
  Requires `nats-server` on PATH (`brew install nats-server`).

The harness is reusable — adding a new scenario is one async test that takes a config string and exercises one identity. When the next person tightens `$JS.API.*` perms or adds account-based isolation, they can extend the same harness rather than starting over.
