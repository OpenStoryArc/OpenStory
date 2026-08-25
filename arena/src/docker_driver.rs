//! Real [`SandboxDriver`] backed by the local Docker daemon.
//!
//! **The seal lives here.** Every sandbox gets its own *internal* Docker
//! network (`arena-sb-{user}`) with only the edge proxy and the LiteLLM
//! gateway attached. An internal network has no route to the outside world
//! and no route to any other user's network, so cross-user reach and raw
//! internet egress both fail at the network layer rather than at a policy
//! layer we would have to keep correct forever.
//!
//! Everything else is defense in depth: read-only rootfs, all capabilities
//! dropped, `no-new-privileges`, a tmpfs `/tmp`, CPU/memory caps, and no
//! published ports — the edge reaches the container over the shared network.
//!
//! Every step is idempotent: check-then-create on the way in, ignore-404 on
//! the way out. A relaunch reuses the user's `$HOME` volume, and a
//! half-finished teardown can simply be re-run.

use std::collections::HashMap;

use async_trait::async_trait;
use bollard::errors::Error as BollardError;
use bollard::exec::{StartExecOptions, StartExecResults};
use bollard::models::{
    ContainerCreateBody, ExecConfig, HostConfig, NetworkConnectRequest, NetworkCreateRequest,
    NetworkDisconnectRequest, RestartPolicy, RestartPolicyNameEnum, VolumeCreateOptions,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, InspectContainerOptions, RemoveContainerOptionsBuilder,
    RemoveVolumeOptions, StartContainerOptions, StopContainerOptionsBuilder,
};
use bollard::Docker;

use crate::driver::{SandboxDriver, SandboxSpec};
use crate::naming;

/// Where the sandbox's `$HOME` volume is mounted inside the container.
const HOME_MOUNT: &str = "/home/dev";

/// How long to wait for a container to stop before the daemon kills it.
const STOP_TIMEOUT_SECS: i32 = 5;

#[derive(Debug, Clone)]
pub struct DockerDriverConfig {
    /// Container runtime. `Some("runsc")` in prod (gVisor); `None` uses the
    /// daemon default, which is what CI and dev machines have.
    pub runtime: Option<String>,
    /// Injected as `ANTHROPIC_BASE_URL` — e.g. `http://litellm:4000`.
    pub litellm_url: String,
    /// Edge proxy container (e.g. `arena-caddy`) joined to every sandbox
    /// network so it can reach the sandbox without published ports.
    pub edge_container: Option<String>,
    /// LiteLLM gateway container joined to every sandbox network — the only
    /// path out of the sandbox, and it only speaks to the model API.
    pub litellm_container: Option<String>,
    /// CPU cap, in cores. Default 2.0.
    pub cpu_limit: f64,
    /// Memory cap, in bytes. Default 2 GiB.
    pub memory_bytes: i64,
    /// Injected as `ARENA_BASE_DOMAIN` — the welcome script prints the user's
    /// dashboard URL from it.
    pub base_domain: String,
    /// Overrides the image's `CMD`. `None` in prod; tests use
    /// `["sleep", "300"]` so a bare `alpine` stays up.
    pub cmd_override: Option<Vec<String>>,
}

impl Default for DockerDriverConfig {
    fn default() -> Self {
        DockerDriverConfig {
            runtime: None,
            litellm_url: String::new(),
            edge_container: None,
            litellm_container: None,
            cpu_limit: 2.0,
            memory_bytes: 2 * 1024 * 1024 * 1024,
            base_domain: String::new(),
            cmd_override: None,
        }
    }
}

pub struct DockerDriver {
    docker: Docker,
    cfg: DockerDriverConfig,
}

/// A 404 from the daemon means "already gone" — every teardown step treats it
/// as success, which is what makes `destroy` re-runnable.
fn is_not_found(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError { status_code: 404, .. }
    )
}

/// 409 Conflict on a create = something else made it between our check and
/// our create. Concurrent launches for the same user race here, and the
/// loser's desired state already holds.
fn is_conflict(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError { status_code: 409, .. }
    )
}

/// 403 Forbidden on a network connect = the container already has an
/// endpoint on this network, which is exactly what we wanted.
fn is_already_connected(e: &BollardError) -> bool {
    matches!(
        e,
        BollardError::DockerResponseServerError { status_code: 403, .. }
    )
}

/// The containers, if any, that must be joined to every sandbox network.
/// Edge first, gateway second — a free function so the ordering is testable
/// without a daemon.
fn peers(cfg: &DockerDriverConfig) -> Vec<String> {
    cfg.edge_container
        .iter()
        .chain(cfg.litellm_container.iter())
        .cloned()
        .collect()
}

fn ok_if_absent(result: Result<(), BollardError>) -> Result<(), BollardError> {
    match result {
        Err(e) if is_not_found(&e) => Ok(()),
        other => other,
    }
}

/// Teardown is best-effort past the container: a network still holding a
/// racing endpoint, or a volume the daemon has not released yet, must not
/// turn a logout into a 500. Say so on stderr and keep going — the reaper
/// will come back around.
fn warn_only(step: &str, result: Result<(), BollardError>) {
    if let Err(e) = result {
        if !is_not_found(&e) {
            eprintln!("arena: docker teardown: {step} failed (continuing): {e}");
        }
    }
}

impl DockerDriver {
    pub fn connect(cfg: DockerDriverConfig) -> anyhow::Result<Self> {
        let docker = Docker::connect_with_local_defaults()?;
        Ok(DockerDriver { docker, cfg })
    }

    /// Build a driver over an already-connected client (tests, or a caller
    /// that wants a non-default connection).
    pub fn with_docker(docker: Docker, cfg: DockerDriverConfig) -> Self {
        DockerDriver { docker, cfg }
    }

    pub async fn network_exists(&self, name: &str) -> anyhow::Result<bool> {
        match self
            .docker
            .inspect_network(name, None::<bollard::query_parameters::InspectNetworkOptions>)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn volume_exists(&self, name: &str) -> anyhow::Result<bool> {
        match self.docker.inspect_volume(name).await {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Run a command inside a sandbox and return its exit code.
    ///
    /// Started detached and polled, so we never have to drain an attached
    /// stream — the exit code is the only thing callers (the seal probe,
    /// ops health checks) actually want.
    pub async fn exec_exit_code(&self, container: &str, cmd: Vec<&str>) -> anyhow::Result<i64> {
        let exec = self
            .docker
            .create_exec(
                container,
                ExecConfig {
                    cmd: Some(cmd.into_iter().map(String::from).collect()),
                    ..Default::default()
                },
            )
            .await?;

        match self
            .docker
            .start_exec(
                &exec.id,
                Some(StartExecOptions {
                    detach: true,
                    ..Default::default()
                }),
            )
            .await?
        {
            StartExecResults::Detached => {}
            StartExecResults::Attached { .. } => {
                anyhow::bail!("docker returned an attached exec for a detached start")
            }
        }

        for _ in 0..150 {
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            let info = self.docker.inspect_exec(&exec.id).await?;
            if info.running != Some(true) {
                if let Some(code) = info.exit_code {
                    return Ok(code);
                }
            }
        }
        anyhow::bail!("exec in {container} did not finish within 30s")
    }
}

#[async_trait]
impl SandboxDriver for DockerDriver {
    async fn create(&self, spec: &SandboxSpec) -> anyhow::Result<String> {
        let user = spec.username.as_str();
        let network = naming::network_name(user);
        let volume = naming::volume_name(user);
        let container = naming::container_name(user);

        let mut labels = HashMap::new();
        labels.insert("arena.user".to_string(), user.to_string());
        labels.insert("arena.event".to_string(), spec.event.clone());

        // 1. The per-user internal network. `internal: true` is the seal.
        if !self.network_exists(&network).await? {
            match self
                .docker
                .create_network(NetworkCreateRequest {
                    name: network.clone(),
                    driver: Some("bridge".to_string()),
                    internal: Some(true),
                    labels: Some(labels.clone()),
                    ..Default::default()
                })
                .await
            {
                Ok(_) => {}
                // Raced with a concurrent launch for the same user.
                Err(e) if is_conflict(&e) => {}
                Err(e) => return Err(e.into()),
            }
        }

        // 2. Join the edge and the gateway. These are the *only* two peers on
        //    the network; nothing else can be reached from inside.
        for peer in peers(&self.cfg) {
            match self
                .docker
                .connect_network(
                    &network,
                    NetworkConnectRequest {
                        container: Some(peer.clone()),
                        endpoint_config: None,
                    },
                )
                .await
            {
                Ok(()) => {}
                Err(e) if is_already_connected(&e) => {}
                Err(e) => {
                    return Err(anyhow::Error::new(e)
                        .context(format!("connecting {peer} to network {network}")))
                }
            }
        }

        // 3. The home volume. Kept across relaunches — losing a competitor's
        //    work because their tab crashed would be unforgivable.
        if !self.volume_exists(&volume).await? {
            match self
                .docker
                .create_volume(VolumeCreateOptions {
                    name: Some(volume.clone()),
                    labels: Some(labels.clone()),
                    ..Default::default()
                })
                .await
            {
                Ok(_) => {}
                Err(e) if is_conflict(&e) => {}
                Err(e) => return Err(e.into()),
            }
        }

        // 4. The container itself. A stale one from a previous launch is
        //    force-removed rather than reused: the API key and the expiry are
        //    baked into its env and labels at create time, so reuse would
        //    silently hand the user a revoked key. The home volume survives.
        ok_if_absent(
            self.docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptionsBuilder::new().force(true).build()),
                )
                .await,
        )?;

        let mut container_labels = labels.clone();
        container_labels.insert("arena.expires".to_string(), spec.expires_at.to_rfc3339());

        let env = vec![
            format!("ANTHROPIC_API_KEY={}", spec.api_key),
            format!("ANTHROPIC_BASE_URL={}", self.cfg.litellm_url),
            format!("ARENA_USERNAME={user}"),
            format!("ARENA_BASE_DOMAIN={}", self.cfg.base_domain),
        ];

        let host_config = HostConfig {
            // The per-user internal network, joined at create time.
            network_mode: Some(network.clone()),
            binds: Some(vec![format!("{volume}:{HOME_MOUNT}")]),
            readonly_rootfs: Some(true),
            cap_drop: Some(vec!["ALL".to_string()]),
            security_opt: Some(vec!["no-new-privileges".to_string()]),
            tmpfs: Some(HashMap::from([("/tmp".to_string(), String::new())])),
            nano_cpus: Some((self.cfg.cpu_limit * 1e9) as i64),
            memory: Some(self.cfg.memory_bytes),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            runtime: self.cfg.runtime.clone(),
            // Deliberately no `port_bindings`: the edge reaches the sandbox
            // over the shared network, so nothing is exposed on the host.
            ..Default::default()
        };

        let created = self
            .docker
            .create_container(
                Some(CreateContainerOptionsBuilder::new().name(&container).build()),
                ContainerCreateBody {
                    image: Some(spec.image.clone()),
                    hostname: Some(user.to_string()),
                    env: Some(env),
                    cmd: self.cfg.cmd_override.clone(),
                    labels: Some(container_labels),
                    host_config: Some(host_config),
                    ..Default::default()
                },
            )
            .await?;

        // 5. Start it.
        self.docker
            .start_container(&created.id, None::<StartContainerOptions>)
            .await?;

        Ok(created.id)
    }

    async fn destroy(&self, username: &str, keep_volume: bool) -> anyhow::Result<()> {
        let network = naming::network_name(username);
        let volume = naming::volume_name(username);
        let container = naming::container_name(username);

        warn_only(
            "stop container",
            self.docker
                .stop_container(
                    &container,
                    Some(StopContainerOptionsBuilder::new().t(STOP_TIMEOUT_SECS).build()),
                )
                .await,
        );
        ok_if_absent(
            self.docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptionsBuilder::new().force(true).build()),
                )
                .await,
        )?;

        // Peers have to leave before the network can go. Both steps race with
        // other users' teardowns, so neither is fatal.
        for peer in peers(&self.cfg) {
            warn_only(
                "disconnect peer",
                self.docker
                    .disconnect_network(
                        &network,
                        NetworkDisconnectRequest {
                            container: Some(peer),
                            force: Some(true),
                        },
                    )
                    .await,
            );
        }
        warn_only("remove network", self.docker.remove_network(&network).await);

        if !keep_volume {
            ok_if_absent(
                self.docker
                    .remove_volume(&volume, None::<RemoveVolumeOptions>)
                    .await,
            )?;
        }

        Ok(())
    }

    async fn is_running(&self, container_id: &str) -> anyhow::Result<bool> {
        match self
            .docker
            .inspect_container(container_id, None::<InspectContainerOptions>)
            .await
        {
            Ok(info) => Ok(info.state.and_then(|s| s.running).unwrap_or(false)),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod config_tests {
    use super::*;

    #[test]
    fn defaults_are_two_cores_and_two_gigs() {
        let cfg = DockerDriverConfig::default();
        assert_eq!(cfg.cpu_limit, 2.0);
        assert_eq!(cfg.memory_bytes, 2 * 1024 * 1024 * 1024);
        assert_eq!(cfg.runtime, None);
        assert_eq!(cfg.cmd_override, None);
    }

    #[test]
    fn only_the_configured_edge_and_gateway_join_a_sandbox_network() {
        let mut cfg = DockerDriverConfig::default();
        assert!(peers(&cfg).is_empty(), "an unconfigured driver joins nobody");
        cfg.edge_container = Some("arena-caddy".into());
        cfg.litellm_container = Some("arena-litellm".into());
        assert_eq!(peers(&cfg), vec!["arena-caddy", "arena-litellm"]);
    }

    #[test]
    fn a_404_is_absence_but_a_500_is_a_failure() {
        let gone = BollardError::DockerResponseServerError {
            status_code: 404,
            message: "no such container".into(),
        };
        let broken = BollardError::DockerResponseServerError {
            status_code: 500,
            message: "daemon on fire".into(),
        };
        assert!(is_not_found(&gone));
        assert!(!is_not_found(&broken));
        assert!(ok_if_absent(Err(gone)).is_ok());
        assert!(ok_if_absent(Err(broken)).is_err());
    }
}

#[cfg(test)]
mod docker_tests {
    use super::*;
    use futures_util::StreamExt;

    fn test_config() -> DockerDriverConfig {
        DockerDriverConfig {
            runtime: None,
            litellm_url: "http://litellm:4000".into(),
            edge_container: None,
            litellm_container: None,
            cpu_limit: 1.0,
            memory_bytes: 512 * 1024 * 1024,
            base_domain: "arena.test".into(),
            cmd_override: Some(vec!["sleep".into(), "300".into()]),
        }
    }

    async fn ensure_image(docker: &Docker, image: &str) {
        if docker.inspect_image(image).await.is_ok() {
            return;
        }
        let opts = bollard::query_parameters::CreateImageOptionsBuilder::new()
            .from_image(image)
            .build();
        let mut stream = docker.create_image(Some(opts), None, None);
        while let Some(item) = stream.next().await {
            item.unwrap_or_else(|e| panic!("pulling {image}: {e}"));
        }
    }

    /// Requires a local Docker daemon. Run: `cargo test -p arena -- --ignored`
    #[tokio::test]
    #[ignore]
    async fn create_is_sealed_and_destroy_cleans_up() {
        let d = DockerDriver::connect(test_config()).unwrap();
        let spec = SandboxSpec {
            username: "arenatest".into(),
            event: "t".into(),
            image: "alpine:3.20".into(),
            api_key: "sk-x".into(),
            expires_at: chrono::Utc::now(),
        };
        ensure_image(&d.docker, &spec.image).await;

        // Leftovers from a previous failed run would poison the assertions.
        d.destroy(&spec.username, false).await.unwrap();

        let id = d.create(&spec).await.unwrap();
        assert!(d.is_running(&id).await.unwrap(), "sandbox should be running");

        // The seal: an internal network has no route off-box, so an outbound
        // fetch cannot resolve or connect. A non-zero exit is the assertion.
        let exit = d
            .exec_exit_code(&id, vec!["wget", "-T", "3", "-q", "-O-", "https://example.com"])
            .await
            .unwrap();
        assert_ne!(exit, 0, "sandbox reached the internet — the seal is broken");

        d.destroy(&spec.username, false).await.unwrap();
        assert!(!d.is_running(&id).await.unwrap(), "sandbox should be gone");
        assert!(
            !d.network_exists(&naming::network_name(&spec.username))
                .await
                .unwrap(),
            "network should be gone"
        );
        assert!(
            !d.volume_exists(&naming::volume_name(&spec.username))
                .await
                .unwrap(),
            "volume should be gone"
        );

        // Teardown is idempotent — a second destroy is a no-op, not an error.
        d.destroy(&spec.username, false).await.unwrap();
    }
}
