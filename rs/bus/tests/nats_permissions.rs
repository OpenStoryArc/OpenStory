//! Permissions spike — exercises NATS single-account user/password authorization.
//!
//! These tests spawn their own `nats-server` per scenario (see `harness/`) and
//! prove what user-level subject permissions can and cannot enforce. The
//! headline finding lives in `single_account_jetstream_consumer_leak`: with
//! one account, a tenant who has `$JS.API.>` (which they need to use
//! JetStream at all) can read another tenant's data by creating a pull
//! consumer with a cross-tenant `filter_subject`. To get hard tenant
//! isolation on JetStream you need accounts, not just permissions.
//!
//! Requires `nats-server` on PATH. Run with:
//!     cargo test -p open-story-bus --test nats_permissions -- --ignored

use async_nats::jetstream::{self, consumer::pull, stream as js_stream};
use futures::StreamExt;
use std::time::Duration;

mod harness;
use harness::NatsServer;

const ALPHA: &str = "alpha";
const BETA: &str = "beta";
const TIMEOUT: Duration = Duration::from_millis(800);

/// Two tenant users (`alpha_user`, `beta_user`) each scoped to their own
/// project subject space, plus an `admin` user with full access used to
/// stage cross-tenant fixtures.
fn two_tenant_auth() -> String {
    format!(
        r#"
authorization {{
  users = [
    {{
      user: "alpha_user", password: "alpha_pw",
      permissions: {{
        publish: {{ allow: ["events.{ALPHA}.>", "$JS.API.>", "_INBOX.>"] }}
        subscribe: {{ allow: ["events.{ALPHA}.>", "_INBOX.>", "_deliver.>"] }}
        allow_responses: true
      }}
    }}
    {{
      user: "beta_user", password: "beta_pw",
      permissions: {{
        publish: {{ allow: ["events.{BETA}.>", "$JS.API.>", "_INBOX.>"] }}
        subscribe: {{ allow: ["events.{BETA}.>", "_INBOX.>", "_deliver.>"] }}
        allow_responses: true
      }}
    }}
    {{
      user: "admin", password: "admin_pw",
      permissions: {{
        publish: {{ allow: [">"] }}
        subscribe: {{ allow: [">"] }}
      }}
    }}
  ]
}}
"#
    )
}

async fn connect(server: &NatsServer, user: &str, pass: &str) -> async_nats::Client {
    async_nats::ConnectOptions::with_user_and_password(user.into(), pass.into())
        .connect(server.url())
        .await
        .expect("connect")
}

#[tokio::test]
#[ignore]
async fn wrong_password_is_rejected() {
    let server = NatsServer::start(&two_tenant_auth()).expect("nats-server");
    let res = async_nats::ConnectOptions::with_user_and_password(
        "alpha_user".into(),
        "WRONG".into(),
    )
    .connect(server.url())
    .await;
    assert!(res.is_err(), "expected auth failure with wrong password, got Ok");
}

#[tokio::test]
#[ignore]
async fn tenant_round_trip_on_own_subjects_works() {
    let server = NatsServer::start(&two_tenant_auth()).expect("nats-server");
    let alpha = connect(&server, "alpha_user", "alpha_pw").await;

    let mut sub = alpha
        .subscribe(format!("events.{ALPHA}.>"))
        .await
        .expect("subscribe own");
    alpha.flush().await.expect("flush sub");

    alpha
        .publish(
            format!("events.{ALPHA}.session-1.main"),
            b"hello".to_vec().into(),
        )
        .await
        .expect("publish own");
    alpha.flush().await.expect("flush pub");

    let msg = tokio::time::timeout(TIMEOUT, sub.next())
        .await
        .expect("timeout waiting for own message")
        .expect("subscriber stream ended");
    assert_eq!(msg.payload.as_ref(), b"hello");
}

#[tokio::test]
#[ignore]
async fn tenant_cannot_publish_to_other_tenant_via_jetstream() {
    // JetStream publish awaits a server ack, so a deny surfaces as an error
    // (vs. core NATS publish which is fire-and-forget and silently dropped).
    let server = NatsServer::start(&two_tenant_auth()).expect("nats-server");

    let admin = connect(&server, "admin", "admin_pw").await;
    let admin_js = jetstream::new(admin);
    admin_js
        .create_stream(js_stream::Config {
            name: "events".into(),
            subjects: vec!["events.>".into()],
            retention: js_stream::RetentionPolicy::Limits,
            ..Default::default()
        })
        .await
        .expect("admin create stream");

    let alpha = connect(&server, "alpha_user", "alpha_pw").await;
    let alpha_js = jetstream::new(alpha);

    // Sanity: own-subject JS publish succeeds.
    let ok = tokio::time::timeout(
        TIMEOUT,
        alpha_js.publish(
            format!("events.{ALPHA}.session-1.main"),
            b"mine".to_vec().into(),
        ),
    )
    .await
    .expect("own-subject publish dispatch timeout")
    .expect("own-subject publish call");
    tokio::time::timeout(TIMEOUT, ok)
        .await
        .expect("own-subject ack timeout")
        .expect("own-subject ack");

    // Cross-tenant publish: server denies the underlying core publish, so the
    // ack future never completes. We assert that — either the dispatch errors
    // or the ack times out. Both prove the deny.
    let dispatch = tokio::time::timeout(
        TIMEOUT,
        alpha_js.publish(
            format!("events.{BETA}.session-x.main"),
            b"trespass".to_vec().into(),
        ),
    )
    .await;

    let denied = match dispatch {
        Err(_) => true, // dispatch itself timed out
        Ok(Err(_)) => true, // dispatch errored
        Ok(Ok(ack_fut)) => {
            // Dispatch returned an ack future; the ack should never arrive.
            tokio::time::timeout(TIMEOUT, ack_fut).await.is_err()
                || tokio::time::timeout(TIMEOUT, async {}).await.is_ok() && {
                    // re-check: spawn a fresh ack wait with a fresh timeout
                    false
                }
        }
    };
    assert!(denied, "alpha_user should not be able to JS-publish to events.{BETA}.>");
}

#[tokio::test]
#[ignore]
async fn tenant_core_subscribe_to_other_tenant_receives_nothing() {
    // alpha_user subscribes (core NATS) to events.beta.> — server denies the
    // SUB, but async-nats doesn't error the call; the deny is asynchronous.
    // We assert by *effect*: an admin-published message on events.beta.x must
    // not reach alpha within the timeout.
    let server = NatsServer::start(&two_tenant_auth()).expect("nats-server");

    let alpha = connect(&server, "alpha_user", "alpha_pw").await;
    let mut sub = alpha
        .subscribe(format!("events.{BETA}.>"))
        .await
        .expect("subscribe call accepted");
    alpha.flush().await.expect("flush");

    let admin = connect(&server, "admin", "admin_pw").await;
    admin
        .publish(
            format!("events.{BETA}.session-x.main"),
            b"secret".to_vec().into(),
        )
        .await
        .expect("admin publish");
    admin.flush().await.expect("admin flush");

    let received = tokio::time::timeout(TIMEOUT, sub.next()).await;
    assert!(
        received.is_err(),
        "alpha_user should not receive cross-tenant core subscription, got {:?}",
        received.ok().flatten().map(|m| String::from_utf8_lossy(&m.payload).to_string())
    );
}

/// HEADLINE SPIKE FINDING.
///
/// With a single account, a tenant who has `$JS.API.>` permission (which they
/// need to use JetStream at all — create consumers, fetch messages, etc.) can
/// create a pull consumer on a shared stream with a `filter_subject` that
/// targets another tenant's data, and the server will happily deliver those
/// messages to them. Subject-level subscribe permissions do NOT gate
/// JetStream consumer reads — the server publishes deliveries on the
/// consumer's inbox, which the tenant *is* allowed to receive on.
///
/// This test passes today (the leak exists). When it starts failing, the
/// underlying behavior changed and we should re-evaluate the threat model.
#[tokio::test]
#[ignore]
async fn single_account_jetstream_consumer_leak() {
    let server = NatsServer::start(&two_tenant_auth()).expect("nats-server");

    // Admin stages cross-tenant data into a shared stream.
    let admin = connect(&server, "admin", "admin_pw").await;
    let admin_js = jetstream::new(admin);
    admin_js
        .create_stream(js_stream::Config {
            name: "events".into(),
            subjects: vec!["events.>".into()],
            retention: js_stream::RetentionPolicy::Limits,
            ..Default::default()
        })
        .await
        .expect("create stream");
    admin_js
        .publish(
            format!("events.{BETA}.session-x.main"),
            b"beta-secret".to_vec().into(),
        )
        .await
        .expect("admin publish dispatch")
        .await
        .expect("admin publish ack");

    // alpha_user — has subscribe on events.alpha.> only — creates a consumer
    // filtered to events.beta.> and reads the secret payload.
    let alpha = connect(&server, "alpha_user", "alpha_pw").await;
    let alpha_js = jetstream::new(alpha);
    let stream = alpha_js
        .get_stream("events")
        .await
        .expect("get stream as alpha");
    let consumer = stream
        .create_consumer(pull::Config {
            filter_subject: format!("events.{BETA}.>"),
            ..Default::default()
        })
        .await
        .expect("alpha creates cross-tenant filter_subject consumer");

    let mut batch = consumer
        .fetch()
        .max_messages(1)
        .expires(TIMEOUT)
        .messages()
        .await
        .expect("fetch messages");

    let leaked = tokio::time::timeout(TIMEOUT * 2, batch.next())
        .await
        .expect("fetch timed out")
        .expect("fetch returned None")
        .expect("message error");

    assert_eq!(
        leaked.payload.as_ref(),
        b"beta-secret",
        "leak demonstrates: single-account user/password permissions do not isolate JetStream reads"
    );
}
