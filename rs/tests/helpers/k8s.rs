//! K3s testcontainer helper — boots an ephemeral Kubernetes cluster for integration tests.
//!
//! Uses `rancher/k3s` as a GenericImage in testcontainers. K3s runs a full
//! K8s API server + kubelet in a single container, lightweight enough for CI.
//!
//! Usage:
//!   let cluster = K3sCluster::start().await;
//!   cluster.kubectl_apply(manifest_yaml).await.unwrap();
//!   // ... wait for pods, assert on APIs

use std::time::Duration;

use anyhow::{Context, Result};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage, ImageExt};

/// An ephemeral K3s cluster running inside a testcontainer.
#[allow(dead_code)]
pub struct K3sCluster {
    container: ContainerAsync<GenericImage>,
    kubeconfig: String,
    client: kube::Client,
    api_port: u16,
}

#[allow(dead_code)]
impl K3sCluster {
    /// Start a K3s cluster. Waits for the API server to be ready.
    pub async fn start() -> Result<Self> {
        let image = GenericImage::new("rancher/k3s", "v1.31.5-k3s1")
            .with_privileged(true)
            .with_cmd(vec![
                "server".to_string(),
                "--disable=traefik".to_string(),
                "--disable=servicelb".to_string(),
                "--disable=metrics-server".to_string(),
                "--write-kubeconfig-mode=644".to_string(),
            ])
            .with_env_var("K3S_KUBECONFIG_OUTPUT", "/output/kubeconfig.yaml");

        eprintln!("  Starting K3s cluster...");
        let container = image.start().await
            .context("Failed to start K3s container")?;

        let api_port = container.get_host_port_ipv4(6443).await
            .context("Failed to get K3s API port")?;

        eprintln!("  K3s API on localhost:{api_port}");

        // Wait for K3s to write kubeconfig and extract it
        let kubeconfig = Self::wait_for_kubeconfig(&container, api_port).await
            .context("Failed to get kubeconfig from K3s")?;

        // Create kube client from the kubeconfig
        let client = Self::create_client(&kubeconfig).await
            .context("Failed to create kube client")?;

        // Wait for the cluster to be actually ready
        Self::wait_for_ready(&client).await
            .context("K3s cluster did not become ready")?;

        eprintln!("  K3s cluster ready");

        Ok(Self { container, kubeconfig, client, api_port })
    }

    /// Wait for K3s to produce a kubeconfig, then rewrite the server URL
    /// to point at the mapped host port.
    async fn wait_for_kubeconfig(container: &ContainerAsync<GenericImage>, host_port: u16) -> Result<String> {
        for attempt in 0..60 {
            let mut exec = container
                .exec(testcontainers::core::ExecCommand::new(vec![
                    "cat".to_string(),
                    "/etc/rancher/k3s/k3s.yaml".to_string(),
                ]))
                .await
                .context("exec failed")?;

            let stdout_bytes = exec.stdout_to_vec().await.unwrap_or_default();
            let raw = String::from_utf8_lossy(&stdout_bytes).to_string();

            if raw.contains("server:") && raw.contains("certificate-authority-data") {
                let kubeconfig = raw.replace(
                    "server: https://127.0.0.1:6443",
                    &format!("server: https://127.0.0.1:{host_port}"),
                );
                return Ok(kubeconfig);
            }

            if attempt % 10 == 0 && attempt > 0 {
                eprintln!("  Waiting for K3s kubeconfig... ({attempt}s)");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        anyhow::bail!("Timed out waiting for K3s kubeconfig (60s)")
    }

    /// Create a kube::Client from a kubeconfig string.
    async fn create_client(kubeconfig: &str) -> Result<kube::Client> {
        let kubeconfig_parsed = kube::config::Kubeconfig::from_yaml(kubeconfig)
            .context("Failed to parse kubeconfig YAML")?;

        let config = kube::Config::from_custom_kubeconfig(
            kubeconfig_parsed,
            &kube::config::KubeConfigOptions::default(),
        )
        .await
        .context("Failed to build kube config")?;

        // K3s uses a self-signed CA — the kubeconfig includes the CA cert,
        // so kube::Client trusts it automatically via the kubeconfig.
        kube::Client::try_from(config)
            .context("Failed to create kube client")
    }

    /// Wait for at least one node to be Ready.
    async fn wait_for_ready(client: &kube::Client) -> Result<()> {
        use k8s_openapi::api::core::v1::Node;
        use kube::api::Api;

        let nodes: Api<Node> = Api::all(client.clone());

        for attempt in 0..60 {
            if let Ok(node_list) = nodes.list(&Default::default()).await {
                let ready = node_list.items.iter().any(|node| {
                    node.status.as_ref()
                        .and_then(|s| s.conditions.as_ref())
                        .map(|conditions| {
                            conditions.iter().any(|c| c.type_ == "Ready" && c.status == "True")
                        })
                        .unwrap_or(false)
                });
                if ready {
                    return Ok(());
                }
            }
            if attempt % 10 == 0 && attempt > 0 {
                eprintln!("  Waiting for K3s node ready... ({attempt}s)");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
        anyhow::bail!("Timed out waiting for K3s node to be ready (60s)")
    }

    /// Apply a YAML manifest using kubectl inside the K3s container.
    pub async fn kubectl_apply(&self, yaml: &str) -> Result<String> {
        // Write yaml to a file inside the container, then kubectl apply
        // Using base64 to avoid shell escaping issues
        let b64 = base64_encode(yaml);
        let cmd = testcontainers::core::ExecCommand::new(vec![
            "sh".to_string(),
            "-c".to_string(),
            format!("echo '{}' | base64 -d | kubectl apply -f -", b64),
        ]);

        let mut result = self.container.exec(cmd).await
            .context("Failed to exec kubectl apply")?;

        let stdout_bytes = result.stdout_to_vec().await.unwrap_or_default();
        let stderr_bytes = result.stderr_to_vec().await.unwrap_or_default();
        let stdout = String::from_utf8_lossy(&stdout_bytes).to_string();
        let stderr = String::from_utf8_lossy(&stderr_bytes).to_string();

        if stderr.contains("error") || stderr.contains("Error") {
            anyhow::bail!("kubectl apply failed:\nstdout: {stdout}\nstderr: {stderr}");
        }

        eprintln!("  kubectl apply: {}", stdout.trim());
        Ok(stdout)
    }

    /// Get the kube client for direct API access.
    pub fn client(&self) -> &kube::Client {
        &self.client
    }

    /// Get the raw kubeconfig YAML.
    pub fn kubeconfig(&self) -> &str {
        &self.kubeconfig
    }

    /// Get the host-mapped K8s API port.
    pub fn api_port(&self) -> u16 {
        self.api_port
    }
}

/// Simple base64 encoding (avoid pulling in a crate for this).
fn base64_encode(input: &str) -> String {
    use std::io::Write;
    let mut buf = Vec::new();
    {
        let mut encoder = Base64Encoder::new(&mut buf);
        encoder.write_all(input.as_bytes()).unwrap();
        encoder.finish().unwrap();
    }
    String::from_utf8(buf).unwrap()
}

/// Minimal base64 encoder.
struct Base64Encoder<'a> {
    out: &'a mut Vec<u8>,
    buf: [u8; 3],
    len: usize,
}

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

impl<'a> Base64Encoder<'a> {
    fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out, buf: [0; 3], len: 0 }
    }

    fn flush_buf(&mut self) {
        if self.len == 0 { return; }
        let b = &self.buf;
        self.out.push(B64_CHARS[(b[0] >> 2) as usize]);
        self.out.push(B64_CHARS[((b[0] & 0x03) << 4 | b[1] >> 4) as usize]);
        if self.len > 1 {
            self.out.push(B64_CHARS[((b[1] & 0x0f) << 2 | b[2] >> 6) as usize]);
        } else {
            self.out.push(b'=');
        }
        if self.len > 2 {
            self.out.push(B64_CHARS[(b[2] & 0x3f) as usize]);
        } else {
            self.out.push(b'=');
        }
        self.buf = [0; 3];
        self.len = 0;
    }

    fn finish(mut self) -> std::io::Result<()> {
        self.flush_buf();
        Ok(())
    }
}

impl std::io::Write for Base64Encoder<'_> {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        for &byte in data {
            self.buf[self.len] = byte;
            self.len += 1;
            if self.len == 3 {
                self.flush_buf();
            }
        }
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
