//! Testcontainers helper — spins up `nats:2.10` with JetStream
//! enabled, exposes the broker on a random host port, returns a
//! handle the caller holds for the lifetime of the test.
//!
//! Requires Docker available on the host. Tests using this helper
//! fail with a clear error if Docker isn't reachable.

use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// Start a NATS container with JetStream enabled. Returns the
/// container handle (keep it alive for the duration of the test —
/// dropping it stops the container) and the connect URL.
pub async fn start_nats_container() -> (ContainerAsync<GenericImage>, String) {
    let image = GenericImage::new("nats", "2.10")
        .with_exposed_port(4222.tcp())
        .with_wait_for(WaitFor::message_on_stderr("Server is ready"))
        .with_cmd(["-js"]);

    let container = image
        .start()
        .await
        .expect("docker must be running for testcontainer tests — `docker info` to verify");

    let port = container
        .get_host_port_ipv4(4222)
        .await
        .expect("container port must be mapped");

    let url = format!("nats://127.0.0.1:{port}");
    (container, url)
}
