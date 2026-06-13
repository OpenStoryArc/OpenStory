# Kubernetes Test Plan — actionable build plan for tailnet-federation in k8s

**Companion to:** [KUBERNETES.md](KUBERNETES.md) (the *why* / design report) and
[VALIDATION.md](VALIDATION.md) (the proven docker method this mirrors).
**This doc:** the *how* — concrete phases, files, and runnable tests, grounded in
infrastructure the repo **already has**.

## What we're reusing (the report assumed greenfield Go — the repo is richer)

The research report recommended kind + Cilium + Go `e2e-framework`. But OpenStory
already ships, in Rust:

- **`rs/tests/helpers/k8s.rs` → `K3sCluster`** — boots a real K3s cluster in a
  testcontainer, extracts kubeconfig, gives a `kube::Client`, `kubectl_apply(yaml)`,
  and container `exec`. (See `rs/tests/test_k8s.rs`.)
- **`kube = 0.98` + `k8s-openapi = 0.24` + `testcontainers = 0.27`** in dev-deps.
- The established `#[tokio::test] #[ignore]` pattern for Docker-requiring tests.

**Decision: build the k8s tailnet tests in Rust on `K3sCluster`, not Go on kind.**
Two consequences vs the report:
1. **K3s enforces NetworkPolicy natively** (kube-router-based controller ships in
   K3s) — unlike `kind`'s kindnet which no-ops policy. So the "false-green" trap
   is *mostly* closed by our base, but we still keep the meta-control guard
   (§Phase 1) as defense-in-depth and to document the assumption.
2. The same `exec`-into-container power the ablations need is already in
   `K3sCluster` — we extend it with `exec_in_pod`, not reinvent it.

## The claim → k8s-control mapping (mirrors the docker `prove` 12/12)

| # | Claim | k8s positive | k8s negative control |
|---|-------|--------------|----------------------|
| 1 | Federation works over the tailnet | publish in cluster A → consume in cluster B | — |
| 2 | Link rides the tailnet, not Pod network | NATS leaf remote IP ∈ `100.64.0.0/10` | (covered by 5/6) |
| 3 | Tailnet identity IS the auth (no token) | leaf connects with **no** credentials | absent/wrong **tag** → ACL denies |
| 4 | The tailnet ACL is the boundary | allow `:7422` → up | **flip Headscale ACL deny `:7422` → leaf drops** |
| 5 | The tailnet carries it | up → leaf socket present | **delete the tailscale sidecar / iptables-partition → leaf drops** |
| 6 | Only `:7422` permitted | packet filter == TCP/UDP `:7422` | probe `:9999` over tailnet → refused |
| 7 | **(k8s-native, NEW)** NetworkPolicy enforces the cluster edge | **allow NetworkPolicy → connect** | **deny NetworkPolicy → connect TIMES OUT** |
| 8 | Identity stable across reschedule | delete Pod → re-federates | no-PVC → identity churns |

Claim 7 is the capability k8s *adds* over the docker harness; claims 4/5/6 are
ports of the proven docker ablations.

## Phasing — cheapest, highest-confidence first

### Phase 1 — NetworkPolicy enforcement (single `K3sCluster`, NO Tailscale yet)

The fastest win and the new k8s-native control. Pure NetworkPolicy, no tailnet.

**Build:**
- `k8s/test/nats-hub.yaml` — a NATS Pod (`nats:2.10-alpine`, leaf listener `:7422`)
  + Service, in namespace `federation`, labelled `app: os-peer`.
- `k8s/test/canary-client.yaml` — a `natsio/nats-box` Pod that can `nc -z` the hub.
- `k8s/networkpolicy/default-deny-ingress.yaml` + `allow-leaf-7422.yaml`
  (the §2.4 manifests from KUBERNETES.md).

**Test (`rs/tests/test_k8s_networkpolicy.rs`):**
```rust
#[tokio::test] #[ignore]
async fn networkpolicy_gates_leaf_port() {
    let c = K3sCluster::start().await.unwrap();
    c.kubectl_apply(NATS_HUB_YAML).await.unwrap();
    c.kubectl_apply(CANARY_YAML).await.unwrap();
    wait_pod_ready(&c, "app=os-peer").await;

    // META-CONTROL (guard against false-green): default-deny MUST block, else
    // K3s isn't enforcing and the whole test is meaningless.
    c.kubectl_apply(DEFAULT_DENY).await.unwrap();
    assert!(!nc_succeeds(&c, "canary", "os-peer.federation.svc", 7422).await,
        "default-deny did NOT block — NetworkPolicy not enforced; test invalid");

    // POSITIVE: allow :7422 -> connect.
    c.kubectl_apply(ALLOW_7422).await.unwrap();
    assert!(nc_succeeds(&c, "canary", "os-peer.federation.svc", 7422).await,
        "allow policy present but :7422 unreachable");

    // NEGATIVE: deny :7422 -> connect TIMES OUT (drop, not refuse).
    c.kubectl_delete(ALLOW_7422).await.unwrap(); // default-deny wins again
    let (ok, elapsed) = nc_timed(&c, "canary", "os-peer.federation.svc", 7422).await;
    assert!(!ok, "deny present but :7422 still reachable");
    assert!(elapsed >= Duration::from_secs(4), "expected a TIMEOUT (drop), got fast refuse");
}
```
The **timeout-vs-refuse** distinction (`elapsed >= 4s`) proves a *policy drop*, not
a missing service — the falsifiability detail.

**Helper additions (`helpers/k8s.rs`):** `exec_in_pod(ns, pod, cmd) -> (code, out)`
(via `kube::api::AttachParams` or `kubectl exec` through the container),
`wait_pod_ready(selector)`, `kubectl_delete(yaml)`, `nc_succeeds`, `nc_timed`.

**Why first:** no Tailscale, no `/dev/net/tun`, no Headscale — just K3s + NATS +
policy. Proves the cluster-edge boundary and the meta-control in ~one cluster boot.

### Phase 2 — Tailscale sidecar gives a Pod its tailnet identity (single cluster)

**Build:**
- `k8s/test/headscale.yaml` — Headscale Deployment + Service (HTTP, in-cluster).
- A bootstrap Job that mints a `tag:os-peer` preauth key → Secret (mirrors the
  docker harness's imperative key-mint).
- `k8s/test/os-peer-pod.yaml` — the **Pod-sidecar** model: `nats` (or a probe)
  container + `tailscale/tailscale` sidecar sharing the Pod netns, `NET_ADMIN` +
  `/dev/net/tun`, `--login-server` at the in-cluster Headscale.

**Test:** assert the Pod's `tailscale0` has a `100.64/10` IP (`exec_in_pod ...
tailscale ip -4`), and the **compiled packet filter == :7422** (`tailscale debug
netmap`, the same instrument as the docker `filter_is_7422_only`).

**Risk to resolve here (do it in this phase, not later):** does `/dev/net/tun` +
`NET_ADMIN` reach a Pod inside the **privileged K3s testcontainer**? If not,
fall back to `TS_USERSPACE=true` *for this probe only* and **document that it
changes the property under test** (userspace breaks the localhost-binding analog;
see KUBERNETES.md §1a/§5.6).

### Phase 3 — Two-cluster federation + the ablations (the full analog)

Two `K3sCluster` instances on a **shared docker network** (so direct WireGuard
works and DERP — which needs TLS we don't have — is never required; see §Risks).
Headscale in one cluster (or a third sidecar container) reachable by both.

- Leaf Pod (cluster A) federates to hub Pod (cluster B) over the tailnet.
- **Port the docker ablations:**
  - **Claim 2/6:** read the hub's `/leafz` (`exec_in_pod ... curl :8222/leafz`),
    assert `.leafs[0].ip ∈ 100.64/10` and **not** a Pod-network CIDR. This is the
    canary that the two clusters didn't collapse into one path.
  - **Claim 4 (ACL ablation):** `kubectl exec` into Headscale, swap the policy to
    deny `:7422`, assert `leafnodes → 0` within a bounded window; restore.
  - **Claim 5 (tailnet ablation):** delete the tailscale sidecar container (or
    `iptables` bidirectional partition of the hub IP inside the leaf Pod, the
    surgical method we landed on — **do not** `tailscale down`, it kills the
    sidecar's containerboot). Assert the leaf socket drops; restore → heals.

### Phase 4 — identity stability across reschedule (claim 8)

With persisted `tailscaled` state (PVC): `kubectl delete pod` the leaf, wait for
reschedule, assert federation recovers **and the tailnet node identity is
unchanged**. The no-PVC variant should show identity churn — proving the
persistence requirement is real, not theoretical.

## Concrete deliverables checklist

- [ ] `k8s/` manifest tree (`test/`, `networkpolicy/`) — currently absent.
- [ ] `helpers/k8s.rs`: `exec_in_pod`, `wait_pod_ready`, `kubectl_delete`,
      `nc_succeeds`/`nc_timed`, `pod_log`.
- [ ] `rs/tests/test_k8s_networkpolicy.rs` (Phase 1) — the highest-value, lowest-
      dependency test; ship this first.
- [ ] `rs/tests/test_k8s_tailnet_sidecar.rs` (Phase 2).
- [ ] `rs/tests/test_k8s_federation.rs` (Phase 3 + 4) — two clusters.
- [ ] CI: gate behind `#[ignore]` + a Linux/privileged runner (see Risks).

## Honest risks & gates (decide before investing past Phase 1)

1. **`/dev/net/tun` + `NET_ADMIN` through nested K3s.** The K3s testcontainer is
   privileged, but a *Pod* requesting tun inside it is another nesting layer.
   **Gate:** resolve in Phase 2 with a spike; if it fails on hosted CI, kernel-mode
   tailnet tests need a self-hosted runner, and hosted CI runs only Phase 1
   (NetworkPolicy, no tun). Userspace fallback measures a *different* architecture.
2. **DERP needs TLS (carried from the docker validation).** A hermetic HTTP
   Headscale **cannot relay**. Phase 3 must keep both clusters on a path where
   **direct** WireGuard works (shared docker network between the K3s node
   containers), so relay is never needed. Real cross-NAT multi-cluster needs TLS
   Headscale + a DERP node — a separate, non-hermetic tier.
3. **K3s NetworkPolicy enforcement.** Assumed native (kube-router). The Phase 1
   meta-control *verifies* it rather than trusting it — if default-deny doesn't
   block, the test fails loudly instead of false-greening.
4. **Two clusters sharing a tailnet in testcontainers.** Each `K3sCluster` is its
   own docker container; they need a shared docker network + a reachable Headscale.
   Non-trivial wiring — budget a spike. The single-cluster Phases 1–2 carry most
   of the value and de-risk before committing to two-cluster.
5. **Image arch / loading.** Phase 1–3 networking proofs need only public images
   (`nats`, `tailscale`, `headscale`, `nats-box`) loaded into K3s
   (`k3s ctr images import` or pull); `open-story:test` is only needed if/when we
   assert on `/api/sessions` end-to-end (optional, like the docker P4 cherry).

## Relationship to the docker validation

The docker `prove` experiment ([VALIDATION.md](VALIDATION.md), 12/12) is the
**reference oracle**: every k8s test that ports a docker ablation should produce
the same verdict. The k8s suite *adds* claim 7 (NetworkPolicy) and claim 8
(reschedule identity) — controls only meaningful in k8s — and otherwise re-proves
H1/H2 in the cluster setting. Same method throughout: **positive + falsifiable
negative control, guarded against false-green.**
