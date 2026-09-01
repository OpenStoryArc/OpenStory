//! Real [`SandboxDriver`] backed by the local Docker daemon.
//!
//! **The seal lives here.** Every sandbox gets its own *internal* Docker
//! network (`arena-sb-{user}`) with only the edge proxy and the LiteLLM
//! gateway attached. An internal network has no route to the outside world
//! and no route to any other user's network, so cross-user reach and raw
//! internet egress both fail at the network layer rather than at a policy
//! layer we would have to keep correct forever.
//!
//! **What `internal` does not cover:** an internal network still routes to
//! the host's own bridge gateway address, so anything the host binds on
//! `0.0.0.0` remains reachable from inside every sandbox. Deployment must
//! bind host services to loopback — the seal stops sandbox-to-sandbox and
//! sandbox-to-internet traffic, not sandbox-to-host.
//!
//! Everything else is defense in depth: read-only rootfs, all capabilities
//! dropped, `no-new-privileges`, a `noexec,nosuid,nodev` tmpfs `/tmp`,
//! CPU/memory/PID caps with no swap headroom, and no published ports — the
//! edge reaches the container over the shared network.
//!
//! Every step is idempotent: check-then-create on the way in, ignore-404 on
//! the way out. A relaunch reuses the user's `$HOME` volume, and a
//! half-finished teardown can simply be re-run.

use std::collections::HashMap;

use async_trait::async_trait;
use bollard::errors::Error as BollardError;
use bollard::exec::{StartExecOptions, StartExecResults};
use bollard::models::{
    ContainerCreateBody, ExecConfig, HostConfig, Network, NetworkConnectRequest,
    NetworkCreateRequest, NetworkDisconnectRequest, RestartPolicy, RestartPolicyNameEnum,
    VolumeCreateOptions,
};
use bollard::query_parameters::{
    CreateContainerOptionsBuilder, InspectContainerOptions, InspectNetworkOptions,
    RemoveContainerOptionsBuilder, RemoveVolumeOptions, StartContainerOptions,
    StopContainerOptionsBuilder,
};
use bollard::Docker;

use crate::driver::{SandboxDriver, SandboxSpec};
use crate::naming;

/// Where the sandbox's `$HOME` volume is mounted inside the container.
const HOME_MOUNT: &str = "/home/dev";

/// How long to wait for a container to stop before the daemon kills it.
const STOP_TIMEOUT_SECS: i32 = 5;

/// Mount options for the sandbox's writable `/tmp`. Small, and explicitly not
/// a place to stage and run a binary around the read-only rootfs.
const TMPFS_OPTS: &str = "size=64m,mode=1777,noexec,nosuid,nodev";

/// Process cap. A fork bomb inside a sandbox should exhaust its own budget,
/// not the host's process table.
const PIDS_LIMIT: i64 = 512;

/// The only network driver we will run a sandbox on. Anything else (macvlan,
/// host, a third-party plugin) has different reachability rules than the ones
/// this module reasons about.
const SEALED_DRIVER: &str = "bridge";

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

/// Record a teardown step's outcome without short-circuiting.
///
/// `destroy` must attempt *every* step before it returns, because the reaper
/// treats a `destroy` error as "log it and proceed" — it revokes the key and
/// deletes the row regardless. A step that bailed early would therefore
/// orphan its container, network, or volume permanently, with no row left to
/// find them by. So: try everything, collect what failed, and report at the
/// end. 404 is not a failure — it means the thing is already gone.
fn note(failures: &mut Vec<String>, step: &str, result: Result<(), BollardError>) {
    if let Err(e) = result {
        if !is_not_found(&e) {
            eprintln!("arena: docker teardown: {step} failed (continuing): {e}");
            failures.push(format!("{step}: {e}"));
        }
    }
}

/// A pre-existing network is only usable if we can *prove* it is sealed.
/// Trusting the name alone would let anything that got there first — a stale
/// non-internal network from a hand-run `docker network create`, or a
/// deliberately planted one — silently un-seal a sandbox.
fn assert_sealed(name: &str, net: &Network) -> anyhow::Result<()> {
    let internal = net.internal;
    let driver = net.driver.as_deref();
    if internal != Some(true) || driver != Some(SEALED_DRIVER) {
        anyhow::bail!(
            "network {name} exists but is not sealed \
             (internal={internal:?}, driver={driver:?}) — refusing to start sandbox"
        );
    }
    Ok(())
}

/// Resource caps must be real caps. A zero, negative, NaN, or infinite value
/// would be sent to the daemon as "unlimited", which is the opposite of what
/// a misconfigured field should mean. Fail closed, at construction.
fn validate_limits(cfg: &DockerDriverConfig) -> anyhow::Result<()> {
    if !cfg.cpu_limit.is_finite() || cfg.cpu_limit <= 0.0 {
        anyhow::bail!(
            "cpu_limit must be a finite positive number of cores, got {}",
            cfg.cpu_limit
        );
    }
    if cfg.memory_bytes <= 0 {
        anyhow::bail!(
            "memory_bytes must be a positive byte count, got {}",
            cfg.memory_bytes
        );
    }
    Ok(())
}

impl DockerDriver {
    pub fn connect(cfg: DockerDriverConfig) -> anyhow::Result<Self> {
        validate_limits(&cfg)?;
        let docker = Docker::connect_with_local_defaults()?;
        Ok(DockerDriver { docker, cfg })
    }

    /// Build a driver over an already-connected client (tests, or a caller
    /// that wants a non-default connection).
    pub fn with_docker(docker: Docker, cfg: DockerDriverConfig) -> anyhow::Result<Self> {
        validate_limits(&cfg)?;
        Ok(DockerDriver { docker, cfg })
    }

    /// Plain existence check — says nothing about whether the network is
    /// sealed. Use [`DockerDriver::sealed_network_exists`] before running a
    /// sandbox on one.
    pub async fn network_exists(&self, name: &str) -> anyhow::Result<bool> {
        match self
            .docker
            .inspect_network(name, None::<InspectNetworkOptions>)
            .await
        {
            Ok(_) => Ok(true),
            Err(e) if is_not_found(&e) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// `Ok(true)` = the network is there *and* sealed. `Ok(false)` = absent.
    /// `Err` = it is there and **not** sealed, which must abort the launch.
    pub async fn sealed_network_exists(&self, name: &str) -> anyhow::Result<bool> {
        match self
            .docker
            .inspect_network(name, None::<InspectNetworkOptions>)
            .await
        {
            Ok(net) => {
                assert_sealed(name, &net)?;
                Ok(true)
            }
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

        // 1. The per-user internal network. `internal: true` is the seal, and
        //    a network we did not just create has to *prove* it — a launch
        //    that cannot establish the seal must not start a container.
        if !self.sealed_network_exists(&network).await? {
            match self
                .docker
                .create_network(NetworkCreateRequest {
                    name: network.clone(),
                    driver: Some(SEALED_DRIVER.to_string()),
                    internal: Some(true),
                    labels: Some(labels.clone()),
                    ..Default::default()
                })
                .await
            {
                Ok(_) => {}
                // Raced with a concurrent launch for the same user. The
                // winner's network gets held to exactly the same bar.
                Err(e) if is_conflict(&e) => {
                    if !self.sealed_network_exists(&network).await? {
                        anyhow::bail!(
                            "network {network} reported a create conflict but does not exist"
                        );
                    }
                }
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
            tmpfs: Some(HashMap::from([(
                "/tmp".to_string(),
                TMPFS_OPTS.to_string(),
            )])),
            nano_cpus: Some((self.cfg.cpu_limit * 1e9) as i64),
            memory: Some(self.cfg.memory_bytes),
            // Equal to `memory` = no swap headroom, so the memory cap is the
            // real ceiling rather than a soft one the kernel pages around.
            memory_swap: Some(self.cfg.memory_bytes),
            pids_limit: Some(PIDS_LIMIT),
            restart_policy: Some(RestartPolicy {
                name: Some(RestartPolicyNameEnum::UNLESS_STOPPED),
                maximum_retry_count: None,
            }),
            runtime: self.cfg.runtime.clone(),
            // ttyd is not a reaper: it doesn't wait(2) on orphaned children
            // (a tmux session's descendants when a pane tears down). Ask the
            // container runtime to inject tini as PID 1 so those get reaped
            // instead of accumulating as zombies.
            init: Some(true),
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

        // Every step runs unconditionally. See `note` for why nothing here is
        // allowed to short-circuit.
        let mut failures = Vec::new();

        note(
            &mut failures,
            "stop container",
            self.docker
                .stop_container(
                    &container,
                    Some(StopContainerOptionsBuilder::new().t(STOP_TIMEOUT_SECS).build()),
                )
                .await,
        );
        note(
            &mut failures,
            "remove container",
            self.docker
                .remove_container(
                    &container,
                    Some(RemoveContainerOptionsBuilder::new().force(true).build()),
                )
                .await,
        );

        // Peers have to leave before the network can go.
        for peer in peers(&self.cfg) {
            note(
                &mut failures,
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
        note(
            &mut failures,
            "remove network",
            self.docker.remove_network(&network).await,
        );

        if !keep_volume {
            note(
                &mut failures,
                "remove volume",
                self.docker
                    .remove_volume(&volume, None::<RemoveVolumeOptions>)
                    .await,
            );
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(anyhow::anyhow!(
                "teardown for {username} attempted every step; {} failed: {}",
                failures.len(),
                failures.join("; ")
            ))
        }
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

    fn server_error(status_code: u16, message: &str) -> BollardError {
        BollardError::DockerResponseServerError {
            status_code,
            message: message.into(),
        }
    }

    #[test]
    fn a_404_is_absence_but_a_500_is_a_failure() {
        let gone = server_error(404, "no such container");
        let broken = server_error(500, "daemon on fire");
        assert!(is_not_found(&gone));
        assert!(!is_not_found(&broken));
        assert!(ok_if_absent(Err(gone)).is_ok());
        assert!(ok_if_absent(Err(broken)).is_err());
    }

    #[test]
    fn teardown_collects_every_failure_instead_of_stopping_at_the_first() {
        let mut failures = Vec::new();
        note(&mut failures, "stop container", Ok(()));
        note(&mut failures, "remove container", Err(server_error(500, "busy")));
        // A 404 is "already gone", not a failure — this is what keeps a
        // second `destroy` a clean no-op.
        note(&mut failures, "remove network", Err(server_error(404, "no such network")));
        note(&mut failures, "remove volume", Err(server_error(409, "volume in use")));

        assert_eq!(failures.len(), 2, "404 must not be recorded: {failures:?}");
        assert!(failures[0].starts_with("remove container"));
        assert!(failures[1].starts_with("remove volume"));
    }

    #[test]
    fn a_clean_teardown_records_nothing() {
        let mut failures = Vec::new();
        for step in ["stop container", "remove container", "remove network"] {
            note(&mut failures, step, Err(server_error(404, "already gone")));
        }
        assert!(failures.is_empty());
    }

    fn network(internal: Option<bool>, driver: Option<&str>) -> Network {
        Network {
            internal,
            driver: driver.map(String::from),
            ..Default::default()
        }
    }

    #[test]
    fn a_sealed_network_is_internal_and_bridge() {
        assert!(assert_sealed("arena-sb-k", &network(Some(true), Some("bridge"))).is_ok());
    }

    #[test]
    fn an_unsealed_network_is_refused_with_its_actual_shape() {
        // The dangerous case: right name, wrong seal.
        let err = assert_sealed("arena-sb-k", &network(Some(false), Some("bridge")))
            .unwrap_err()
            .to_string();
        assert!(err.contains("is not sealed"), "{err}");
        assert!(err.contains("internal=Some(false)"), "{err}");
        assert!(err.contains("refusing to start sandbox"), "{err}");

        // A missing field is not a pass either — absence of proof is not proof.
        assert!(assert_sealed("arena-sb-k", &network(None, Some("bridge"))).is_err());
        // Right sealing flag, wrong driver: different reachability rules.
        assert!(assert_sealed("arena-sb-k", &network(Some(true), Some("macvlan"))).is_err());
        assert!(assert_sealed("arena-sb-k", &network(Some(true), None)).is_err());
    }

    #[test]
    fn nonsense_resource_caps_fail_closed_rather_than_meaning_unlimited() {
        let with = |cpu: f64, mem: i64| DockerDriverConfig {
            cpu_limit: cpu,
            memory_bytes: mem,
            ..Default::default()
        };
        assert!(validate_limits(&with(2.0, 1024)).is_ok());
        for (cpu, mem) in [
            (0.0, 1024),
            (-1.0, 1024),
            (f64::NAN, 1024),
            (f64::INFINITY, 1024),
            (2.0, 0),
            (2.0, -1),
        ] {
            assert!(
                validate_limits(&with(cpu, mem)).is_err(),
                "cpu={cpu} mem={mem} should be rejected"
            );
        }
    }
}

#[cfg(test)]
mod docker_tests {
    use super::*;
    use anyhow::Context;
    use futures_util::StreamExt;

    const TEST_CPUS: f64 = 1.0;
    const TEST_MEMORY: i64 = 512 * 1024 * 1024;

    /// Usernames containing `--` are rejected by [`naming::validate_username`],
    /// so no real competitor can ever hold one. A stray `--ignored` run
    /// against a live host therefore cannot collide with — or destroy — a
    /// real user's sandbox.
    const PROBE_USER: &str = "arenatest--rt";
    const UNSEALED_PROBE_USER: &str = "arenatest--unsealed";

    fn test_config() -> DockerDriverConfig {
        DockerDriverConfig {
            runtime: None,
            litellm_url: "http://litellm:4000".into(),
            edge_container: None,
            litellm_container: None,
            cpu_limit: TEST_CPUS,
            memory_bytes: TEST_MEMORY,
            base_domain: "arena.test".into(),
            cmd_override: Some(vec!["sleep".into(), "300".into()]),
        }
    }

    fn spec_for(user: &str) -> SandboxSpec {
        SandboxSpec {
            username: user.into(),
            event: "t".into(),
            image: "alpine:3.20".into(),
            api_key: "sk-x".into(),
            expires_at: chrono::Utc::now(),
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
        let spec = spec_for(PROBE_USER);
        ensure_image(&d.docker, &spec.image).await;

        // Leftovers from a previous failed run would poison the assertions.
        let _ = d.destroy(&spec.username, false).await;

        let id = d.create(&spec).await.unwrap();
        assert!(d.is_running(&id).await.unwrap(), "sandbox should be running");

        // Positive control FIRST. Without it, "wget failed" could mean the
        // binary is missing or exec is broken, and the seal assertion below
        // would pass for a reason that has nothing to do with the network.
        let control = d
            .exec_exit_code(&id, vec!["sh", "-c", "command -v wget"])
            .await
            .unwrap();
        assert_eq!(control, 0, "probe binary missing or exec plumbing broken");

        // The seal: an internal network has no route off-box, so an outbound
        // fetch cannot resolve or connect. A non-zero exit is the assertion.
        let exit = d
            .exec_exit_code(&id, vec!["wget", "-T", "3", "-q", "-O-", "https://example.com"])
            .await
            .unwrap();
        assert_ne!(exit, 0, "sandbox reached the internet — the seal is broken");

        // The hardening set, read back off the running container so a future
        // edit that quietly drops one of these fails here.
        let hc = d
            .docker
            .inspect_container(&id, None::<InspectContainerOptions>)
            .await
            .unwrap()
            .host_config
            .expect("inspect should report a host config");
        assert_eq!(hc.readonly_rootfs, Some(true), "rootfs must be read-only");
        assert_eq!(hc.cap_drop, Some(vec!["ALL".to_string()]));
        assert_eq!(
            hc.security_opt,
            Some(vec!["no-new-privileges".to_string()])
        );
        assert_eq!(hc.memory, Some(TEST_MEMORY));
        assert_eq!(hc.memory_swap, Some(TEST_MEMORY), "no swap headroom");
        assert_eq!(hc.nano_cpus, Some((TEST_CPUS * 1e9) as i64));
        assert_eq!(hc.pids_limit, Some(PIDS_LIMIT));
        assert_eq!(
            hc.tmpfs,
            Some(HashMap::from([(
                "/tmp".to_string(),
                TMPFS_OPTS.to_string()
            )]))
        );
        assert_eq!(hc.network_mode.as_deref(), Some(naming::network_name(PROBE_USER).as_str()));
        assert_eq!(hc.init, Some(true), "tini must be injected as PID1 to reap orphans");

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

    /// The seal must be *proved*, not assumed from a name. If something got
    /// there first with a non-internal network, the launch has to abort
    /// before any container exists.
    #[tokio::test]
    #[ignore]
    async fn create_refuses_a_pre_existing_network_that_is_not_sealed() {
        let d = DockerDriver::connect(test_config()).unwrap();
        let spec = spec_for(UNSEALED_PROBE_USER);
        ensure_image(&d.docker, &spec.image).await;
        let network = naming::network_name(UNSEALED_PROBE_USER);
        let container = naming::container_name(UNSEALED_PROBE_USER);

        let _ = d.destroy(&spec.username, false).await;

        // Plant a network with the right name and the wrong seal.
        d.docker
            .create_network(NetworkCreateRequest {
                name: network.clone(),
                driver: Some("bridge".to_string()),
                internal: Some(false),
                ..Default::default()
            })
            .await
            .unwrap();

        let err = d
            .create(&spec)
            .await
            .expect_err("create must refuse an unsealed network")
            .to_string();
        assert!(err.contains("is not sealed"), "{err}");
        assert!(err.contains(&network), "{err}");

        // Nothing was started, and nothing was left half-built.
        assert!(
            !d.is_running(&container).await.unwrap(),
            "no container may exist when the seal cannot be proved"
        );
        assert!(
            !d.volume_exists(&naming::volume_name(UNSEALED_PROBE_USER))
                .await
                .unwrap(),
            "create must abort before the volume step"
        );

        d.docker.remove_network(&network).await.unwrap();
        let _ = d.destroy(&spec.username, false).await;
    }

    /// Requires a local Docker daemon. Run: `cargo test -p arena -- --ignored`
    ///
    /// The single-user test above proves ONE sandbox is sealed from the
    /// outside world. This proves the multi-user shape the product actually
    /// runs: N users provisioned *concurrently* land on distinct, mutually
    /// unreachable sandboxes, and torn down leave nothing behind. Usernames
    /// use `--` for the same reason `PROBE_USER` does — `validate_username`
    /// rejects it, so these can never collide with a real user's sandbox.
    #[tokio::test]
    #[ignore]
    async fn multiple_users_get_isolated_sandboxes_and_clean_teardown() {
        const USERS: [&str; 3] = ["mutest--a", "mutest--b", "mutest--c"];

        let d = DockerDriver::connect(test_config()).unwrap();
        let specs: Vec<SandboxSpec> = USERS
            .iter()
            .enumerate()
            .map(|(i, u)| SandboxSpec {
                username: (*u).to_string(),
                event: "mutest".into(),
                image: "alpine:3.20".into(),
                api_key: format!("sk-mutest-{i}"),
                expires_at: chrono::Utc::now(),
            })
            .collect();
        ensure_image(&d.docker, &specs[0].image).await;

        // Pre-clean: leftovers from a previous failed run would poison the
        // assertions below, same reasoning as the single-user test.
        for spec in &specs {
            let _ = d.destroy(&spec.username, false).await;
        }

        // Every assertion in here reports failure through `Result`
        // (`anyhow::ensure!`/`?`) instead of `assert!`/`panic!`, so a
        // failure partway through still falls through to the unconditional
        // teardown below rather than aborting the test with sandboxes left
        // running on the daemon.
        let outcome: anyhow::Result<()> = async {
            // Concurrent provisioning: all N `create` calls are in flight
            // together, not awaited one at a time. That's what actually
            // exercises cross-user interleaving on the daemon — different
            // users' network-create / volume-create / container-create
            // calls racing each other — as opposed to the same-user race
            // the 409 handling inside `create` already covers.
            let results = futures_util::future::join_all(specs.iter().map(|s| d.create(s))).await;

            let mut ids = Vec::with_capacity(specs.len());
            for (spec, r) in specs.iter().zip(results.into_iter()) {
                let id = r.with_context(|| format!("create failed for {}", spec.username))?;
                ids.push(id);
            }

            // No two users landed on the same container under concurrent
            // creation.
            let mut sorted_ids = ids.clone();
            sorted_ids.sort();
            sorted_ids.dedup();
            anyhow::ensure!(
                sorted_ids.len() == ids.len(),
                "concurrent create returned colliding container ids: {ids:?}"
            );

            for (spec, id) in specs.iter().zip(ids.iter()) {
                anyhow::ensure!(
                    d.is_running(id).await?,
                    "{} sandbox should be running",
                    spec.username
                );
                anyhow::ensure!(
                    d.sealed_network_exists(&naming::network_name(&spec.username)).await?,
                    "{} network should exist and be sealed",
                    spec.username
                );
                anyhow::ensure!(
                    d.volume_exists(&naming::volume_name(&spec.username)).await?,
                    "{} volume should exist",
                    spec.username
                );
            }

            // Positive control FIRST — same reasoning as the single-user
            // test's seal probe. Without confirming the probe binary is
            // present, a nonzero exit on the cross-reach check below could
            // mean "wget is missing" rather than "the network blocked it",
            // which would make the isolation assertion vacuously true.
            let control = d
                .exec_exit_code(
                    &naming::container_name("mutest--a"),
                    vec!["sh", "-c", "command -v wget"],
                )
                .await?;
            anyhow::ensure!(
                control == 0,
                "probe binary missing or exec plumbing broken in mutest--a"
            );

            // The isolation proof: each user has their own *internal*
            // network (see module docs), so a's container cannot resolve or
            // route to b's container by its Docker DNS name (its container
            // name). This was confirmed discriminating, not vacuous, by
            // temporarily inverting the expectation (`exit == 0`) during
            // development and watching it fail — see the report for the
            // transcript. It would also pass for the wrong reason if two
            // users' `network_mode` ever pointed at the same network name,
            // which the per-user `naming::network_name` derivation and the
            // per-user sealed-network check above both rule out.
            for (a, b) in [
                ("mutest--a", "mutest--b"),
                ("mutest--b", "mutest--c"),
                ("mutest--c", "mutest--a"),
            ] {
                let target = naming::container_name(b);
                let exit = d
                    .exec_exit_code(
                        &naming::container_name(a),
                        vec!["wget", "-T", "3", "-q", "-O-", &format!("http://{target}")],
                    )
                    .await?;
                anyhow::ensure!(
                    exit != 0,
                    "{a} could reach {b} across sandboxes — cross-user isolation is broken"
                );
            }

            Ok(())
        }
        .await;

        // Teardown happens unconditionally, whether the assertions above
        // succeeded or failed — a failed run must not leave sandboxes,
        // networks, or volumes running on the daemon.
        for spec in &specs {
            let _ = d.destroy(&spec.username, false).await;
        }

        outcome.unwrap();

        // Clean teardown, verified per user: nothing left behind.
        for spec in &specs {
            let container = naming::container_name(&spec.username);
            assert!(
                !d.is_running(&container).await.unwrap(),
                "{} sandbox should be gone",
                spec.username
            );
            assert!(
                !d.network_exists(&naming::network_name(&spec.username)).await.unwrap(),
                "{} network should be gone",
                spec.username
            );
            assert!(
                !d.volume_exists(&naming::volume_name(&spec.username)).await.unwrap(),
                "{} volume should be gone",
                spec.username
            );
        }

        // Teardown is idempotent — destroying an already-torn-down user
        // again is a clean no-op, not an error.
        d.destroy(&specs[0].username, false).await.unwrap();
    }
}
