//! Phase 5.1 — Multi-account NATS isolation (smoke test).
//!
//! The plan's Layer 2 (person isolation via NATS accounts) hinges on the
//! claim that two NATS accounts on the same server cannot see each other's
//! subjects by default. Before scaling fixtures into JetStream-source
//! territory (where cross-account semantics differ from single-account),
//! we prove the simplest possible case at the core-NATS pub/sub level:
//!
//!   given person_max publishes `events.session-X` on her account,
//!   when person_katie subscribes to `events.>` on her account,
//!   then katie receives nothing.
//!
//! Ignored by default — requires Docker. Run explicitly with:
//!   cargo test -p open-story-bus --test test_multi_account_isolation -- --ignored

use std::time::Duration;

use async_nats::Client;
use futures::StreamExt;
use tempfile::TempDir;
use testcontainers::{
    core::{IntoContainerPort, Mount, WaitFor},
    runners::AsyncRunner,
    ContainerAsync, GenericImage, ImageExt,
};

/// The two-account NATS config bundled at compile time.
const NATS_CONF: &str = include_str!("fixtures/nats_two_accounts.conf");

/// One of the two test personas. Each maps to a distinct NATS account
/// with no exports/imports declared between them.
#[derive(Debug, Clone, Copy)]
pub enum Person {
    Max,
    Katie,
}

impl Person {
    fn credentials(&self) -> (&'static str, &'static str) {
        match self {
            Person::Max => ("max", "max-secret"),
            Person::Katie => ("katie", "katie-secret"),
        }
    }
}

/// A running NATS container with two accounts configured. The container,
/// tempdir, and port are all held together so the caller's lifetime keeps
/// the container alive.
pub struct MultiAccountNats {
    _container: ContainerAsync<GenericImage>,
    _conf_dir: TempDir,
    port: u16,
}

impl MultiAccountNats {
    pub async fn start() -> Self {
        // Write the bundled config to a tempfile so testcontainers can bind-mount it.
        let conf_dir = tempfile::tempdir().expect("tempdir");
        let conf_path = conf_dir.path().join("nats-server.conf");
        std::fs::write(&conf_path, NATS_CONF).expect("write nats conf");

        let host_path = conf_path
            .to_str()
            .expect("conf path is utf-8")
            .to_string();

        let container = GenericImage::new("nats", "2-alpine")
            .with_exposed_port(4222.tcp())
            .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
            .with_mount(Mount::bind_mount(host_path, "/etc/nats/nats-server.conf"))
            .with_cmd(["-c", "/etc/nats/nats-server.conf"])
            .start()
            .await
            .expect("start NATS multi-account container");

        let port = container
            .get_host_port_ipv4(4222)
            .await
            .expect("map nats port");

        Self {
            _container: container,
            _conf_dir: conf_dir,
            port,
        }
    }

    pub async fn connect_as(&self, person: Person) -> Client {
        let (user, pass) = person.credentials();
        let url = format!("nats://127.0.0.1:{}", self.port);
        // Pass credentials via ConnectOptions rather than URL: async-nats's
        // URL parser strips userinfo before handshake on some paths, which
        // surfaces as "authorization violation" against a server that's
        // actually correctly configured. The explicit API avoids that
        // ambiguity entirely.
        for attempt in 0..10 {
            match async_nats::ConnectOptions::with_user_and_password(
                user.to_string(),
                pass.to_string(),
            )
            .connect(&url)
            .await
            {
                Ok(c) => return c,
                Err(e) if attempt < 9 => {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    eprintln!("connect retry {attempt} for {user}: {e}");
                }
                Err(e) => panic!("connect as {user} failed: {e}"),
            }
        }
        unreachable!()
    }
}

#[tokio::test]
#[ignore]
async fn account_isolation_prevents_cross_person_visibility() {
    let nats = MultiAccountNats::start().await;

    let max = nats.connect_as(Person::Max).await;
    let katie = nats.connect_as(Person::Katie).await;

    // Katie subscribes to the entire events subject space on HER account.
    let mut katie_sub = katie
        .subscribe("events.>")
        .await
        .expect("katie subscribes to events.>");

    // Round-trip a flush to make sure katie's subscription is established
    // before we publish — async-nats SUB is async-acked.
    katie.flush().await.expect("katie flush");

    // Max publishes a session event on HIS account.
    max.publish("events.session-x", "hello from max".into())
        .await
        .expect("max publish");
    max.flush().await.expect("max flush");

    // Wait long enough that any cross-account delivery would have happened.
    // 500ms is generous — same-host async-nats delivery is sub-millisecond.
    let outcome = tokio::time::timeout(Duration::from_millis(500), katie_sub.next()).await;

    assert!(
        outcome.is_err(),
        "Katie received a message that should have been isolated to Max's account: {outcome:?}"
    );
}

/// Sanity: within ONE account, pub/sub works as expected. If this fails
/// the negative test above is meaningless (might just mean nothing routes).
#[tokio::test]
#[ignore]
async fn same_account_publish_subscribe_round_trips() {
    let nats = MultiAccountNats::start().await;
    let max_publisher = nats.connect_as(Person::Max).await;
    let max_subscriber = nats.connect_as(Person::Max).await;

    let mut sub = max_subscriber
        .subscribe("events.>")
        .await
        .expect("subscribe");
    max_subscriber.flush().await.expect("flush sub");

    max_publisher
        .publish("events.session-x", "hello from the other max".into())
        .await
        .expect("publish");
    max_publisher.flush().await.expect("flush pub");

    let received = tokio::time::timeout(Duration::from_secs(2), sub.next())
        .await
        .expect("did not time out")
        .expect("got message");

    assert_eq!(received.subject.as_str(), "events.session-x");
    assert_eq!(&received.payload[..], b"hello from the other max");
}
