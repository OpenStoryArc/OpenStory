//! K8s integration tests — deploy Open Story manifests to K3s testcontainer.
//!
//! These tests boot a real K3s cluster inside a Docker container, deploy
//! Open Story's K8s manifests, and verify the system works end-to-end.
//!
//! Prerequisites:
//!   - Docker (not Podman — K3s needs cgroups v2 and privileged mode)
//!   - Run from Linux or WSL (K3s is a Linux binary)
//!   - `docker pull rancher/k3s:v1.31.5-k3s1` (first run pulls ~200MB)
//!
//! Run: cargo test -p open-story --test test_k8s -- --ignored --nocapture
//!
//! === SESSION HANDOFF MARKER ===
//! Session ID: 7d286a47-5b66-45cb-9715-07cd5f100dfc
//! Branch: feature/claude-integration-testing
//! Context: Plan 080 (K8s deployment) spike — proving K3s boots in testcontainers.
//! Next steps after spike passes:
//!   1. Write K8s manifests (k8s/ directory)
//!   2. Load open-story:test image into K3s
//!   3. Deploy NATS + consumer, assert on /health
//!   4. Deploy agent pod with publisher sidecar, assert events flow
//! Search Open Story for this session to read the full implementation context.
//! ===============================

mod helpers;

use helpers::k8s::K3sCluster;

/// Spike: K3s cluster boots and the API server responds.
///
/// This is the foundation test — if K3s doesn't boot, nothing else works.
/// Proves: testcontainers can run K3s, we can extract kubeconfig,
/// kube::Client connects, and at least one node reaches Ready state.
#[tokio::test]
#[ignore] // requires Docker on Linux/WSL
async fn k8s_cluster_boots() {
    let cluster = K3sCluster::start().await
        .expect("K3s cluster should start");

    // Verify we got a valid kubeconfig
    assert!(
        cluster.kubeconfig().contains("server:"),
        "kubeconfig should contain server URL"
    );
    assert!(
        cluster.api_port() > 0,
        "API port should be mapped"
    );

    // Verify we can list namespaces via the kube client
    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::Api;

    let ns_api: Api<Namespace> = Api::all(cluster.client().clone());
    let ns_list = ns_api.list(&Default::default()).await
        .expect("should be able to list namespaces");

    let ns_names: Vec<String> = ns_list.items.iter()
        .filter_map(|ns| ns.metadata.name.clone())
        .collect();

    assert!(ns_names.contains(&"default".to_string()), "should have default namespace");
    assert!(ns_names.contains(&"kube-system".to_string()), "should have kube-system namespace");

    eprintln!("  K3s cluster booted successfully!");
    eprintln!("  API port: {}", cluster.api_port());
    eprintln!("  Namespaces: {:?}", ns_names);
}

/// Spike: Can create a namespace and deploy a simple pod.
///
/// Proves kubectl-equivalent operations work through the kube client.
#[tokio::test]
#[ignore]
async fn k8s_can_create_namespace_and_pod() {
    let cluster = K3sCluster::start().await
        .expect("K3s cluster should start");

    // Create the open-story namespace
    cluster.kubectl_apply(r#"
apiVersion: v1
kind: Namespace
metadata:
  name: open-story-test
"#).await.expect("should create namespace");

    // Verify namespace exists
    use k8s_openapi::api::core::v1::Namespace;
    use kube::api::Api;

    let ns_api: Api<Namespace> = Api::all(cluster.client().clone());
    let ns_list = ns_api.list(&Default::default()).await.unwrap();
    let ns_names: Vec<String> = ns_list.items.iter()
        .filter_map(|ns| ns.metadata.name.clone())
        .collect();

    assert!(
        ns_names.contains(&"open-story-test".to_string()),
        "open-story-test namespace should exist"
    );

    // Deploy a simple pod (busybox, just sleeps)
    cluster.kubectl_apply(r#"
apiVersion: v1
kind: Pod
metadata:
  name: test-pod
  namespace: open-story-test
spec:
  containers:
    - name: busybox
      image: busybox:latest
      command: ["sleep", "3600"]
"#).await.expect("should create pod");

    // Wait for pod to be running
    use k8s_openapi::api::core::v1::Pod;
    let pod_api: Api<Pod> = Api::namespaced(cluster.client().clone(), "open-story-test");

    for attempt in 0..30 {
        if let Ok(pod) = pod_api.get("test-pod").await {
            let phase = pod.status
                .and_then(|s| s.phase)
                .unwrap_or_default();
            if phase == "Running" {
                eprintln!("  Pod is running! (attempt {attempt})");
                return;
            }
            if attempt % 5 == 0 {
                eprintln!("  Pod phase: {phase} (attempt {attempt})");
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }

    panic!("Pod did not reach Running state within 30s");
}
