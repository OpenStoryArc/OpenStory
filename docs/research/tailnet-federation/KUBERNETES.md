# Mapping the Tailnet-Federation Architecture onto Kubernetes — and Testing It Scientifically

**Status:** Research / design report. Exploratory garden artifact (`docs/research/tailnet-federation/`).
**Audience:** OpenStory engineers evaluating whether the two-machine, tailnet-federated NATS topology survives translation to Kubernetes, and how to prove it does with runnable, falsifiable CI.
**Verification key:** Each non-obvious claim is tagged **[VERIFIED]** (confirmed via a 2025/2026 web source), **[INFERRED]** (deduced from architecture + general k8s/NATS/Tailscale mechanics), or **[PROJECT]** (from the OpenStory codebase / CLAUDE.md).

---

## 0. The architecture we are mapping (one-paragraph restatement)

Two independent machines each run a Rust app (`open-story-server`) + its own NATS JetStream. Events federate machine-to-machine over a **purpose-built Tailscale tailnet**. Each machine's tailnet identity comes from a **Tailscale sidecar container** that owns the network namespace; the Rust server embeds no `tsnet` — it just `bind`s `0.0.0.0` inside the sidecar's netns and inherits the tailnet interface for free. The **only** inter-machine traffic is a NATS **leaf → hub** connection on **one port, `:7422`**. The **entire trust model** is a four-line tag-based Tailscale ACL — `src tag:os-peer → dst tag:os-peer:7422`, deny-all-else. **There is no NATS token; the tailnet identity is the auth.** Intra-machine traffic stays on loopback. The hermetic validation harness is **Headscale (self-hosted control server) + Docker**, and the test method is **direct path observation plus falsifiable negative controls / ablations**:

- Flip the ACL to deny `:7422` → federation **stops**.
- `tailscale down` → the leaf socket **drops**.
- Assert the live NATS connection's remote IP is a **CGNAT `100.64.0.0/10`** tailnet address, not a Docker bridge IP.
- Assert the **compiled packet filter** is exactly TCP/UDP `:7422`.

The question of this report: what is the faithful k8s analog, what does k8s *add* (a second policy layer, ephemeral identity), and how do we reproduce — and *strengthen* — the ablation method in Kubernetes CI?

---

## 1. Topology translation to Kubernetes

The load-bearing invariant to preserve is the **identity/binding split**: *the sidecar owns the tailnet identity; the app binds localhost and stays oblivious.* This is exactly the OpenStory soul constraint — the app does not embed network machinery, side effects live at the boundary (the sidecar), and the app is replaceable without touching the trust model. Three options, ranked by how well they preserve that split.

### (a) Tailscale sidecar container in the Pod, sharing the Pod netns — **the direct analog**

In Docker, OpenStory uses `--network container:<tailscale>` so the app process literally lives in the sidecar's network namespace. **In Kubernetes, every container in a Pod already shares one network namespace by construction** [INFERRED — core Pod semantics]. So the Docker trick becomes *free*: drop a `tailscaled` sidecar container into the same Pod as `open-story-server`, and the app's `bind 0.0.0.0:4222` (NATS leaf listener) and `:7422` are reachable on the Pod's shared interface set, including the `tailscale0` CGNAT interface the sidecar brings up.

```yaml
# Pod: one app + one tailscale sidecar, ONE shared netns (k8s default)
apiVersion: v1
kind: Pod
metadata:
  name: os-peer
  labels: { app: os-peer }
spec:
  containers:
    - name: open-story-server
      image: open-story:latest
      # binds 0.0.0.0 INSIDE the shared netns; sees tailscale0 for free.
      args: ["serve", "--manage-nats", "--nats-leaf-url", "nats://hub.tailnet:7422"]
    - name: tailscale
      image: tailscale/tailscale:stable
      env:
        - { name: TS_AUTHKEY,   valueFrom: { secretKeyRef: { name: ts-auth, key: authkey } } }
        - { name: TS_EXTRA_ARGS, value: "--advertise-tags=tag:os-peer --login-server=https://headscale.internal" }
        - { name: TS_USERSPACE, value: "false" }   # kernel networking; needs /dev/net/tun
      securityContext:
        capabilities: { add: ["NET_ADMIN"] }
      volumeMounts:
        - { name: dev-net-tun, mountPath: /dev/net/tun }
  volumes:
    - name: dev-net-tun
      hostPath: { path: /dev/net/tun }
```

**Tradeoffs.**
- **Pro:** byte-for-byte faithful to the production Docker shape. App still "binds localhost," sidecar still "gives identity." Zero change to Rust code or the trust model.
- **Con:** requires `/dev/net/tun` + `NET_ADMIN` (kernel mode) or `TS_USERSPACE=true` (userspace, slower; userspace does **not** transparently expose `tailscale0` to a sibling container — it routes through a SOCKS/loopback proxy, breaking "app binds 0.0.0.0 and just works") [INFERRED]. For a faithful analog you want **kernel mode**, with CI-runner implications (§5).
- **Con:** every Pod needing an identity carries a sidecar — fine here (one federating Pod per machine).

**Recommended primary mapping** — the only option preserving all three properties (sidecar gives identity, app binds localhost, app stays tsnet-free) without compromise.

### (b) The Tailscale Kubernetes Operator (ProxyGroup, Tailscale Services, annotations)

Verified current capabilities (2025/2026):
- **ProxyGroup** (operator **≥1.76**) pre-creates a multi-replica proxy set; HA L3 egress, round-robin. **[VERIFIED]**
- **HA Ingress via ProxyGroup + Tailscale Services** (**≥1.84**); the `-0` replica issues the Let's Encrypt cert. **[VERIFIED]**
- **ProxyGroupPolicy** (**≥1.96**): restrict which namespaces may use the proxy-group annotation. **[VERIFIED]**
- **Cluster egress**: in-cluster workloads dial a tailnet service via an operator-managed `ExternalName` Service. **[VERIFIED]**

**Wrong fit for the current invariant.** The operator's proxies are **`tsnet`-based** — identity lives in a *separate* proxy Pod, and the app reaches it through a cluster Service, **not** by sharing a netns. That breaks the identity-collocation property OpenStory deliberately keeps [INFERRED]. You gain HA, declarative management, MagicDNS, certs — at the cost of the clean one-port, one-identity, no-token model. **Keep as a future-scale option, not the faithful port** (matches `feedback_deploy_existing_backlog_new`).

### (c) `tailscaled` DaemonSet, one per node

Identity becomes **per node, not per app**. The model wants a **per-peer** identity that `tag:os-peer` keys on; a DaemonSet makes the *node* the principal and forces re-introducing a NATS token to distinguish workloads — exactly what OpenStory eliminated [INFERRED]. **Rejected** for peer identity; acceptable only as infra (subnet router / exit node).

### Recommendation matrix

| Property to preserve | (a) Pod sidecar | (b) Operator/tsnet | (c) DaemonSet |
|---|---|---|---|
| Sidecar gives identity | ✅ exact | ⚠️ separate proxy Pod | ❌ node-level |
| App binds localhost, tsnet-free | ✅ exact | ❌ app dials cluster Service | ❌ |
| One identity == one peer | ✅ | ✅ (per ProxyGroup) | ❌ shared per node |
| One port `:7422` only | ✅ | ⚠️ + cluster hops | ⚠️ |
| No NATS token needed | ✅ | ⚠️ depends | ❌ likely needs token |
| HA / declarative / MagicDNS / certs | ❌ DIY | ✅ | ⚠️ |

**Pick (a) for fidelity now; hold (b) in the backlog for HA/multi-service scale.**

---

## 2. The trust boundary in Kubernetes — now **two** policy layers

On bare Docker there is one enforcement layer: the **Tailscale tag-ACL**, compiled to a packet filter on each node's `tailscale0`. In Kubernetes a **second, independent layer** appears: **Kubernetes NetworkPolicy** (Calico / Cilium). The two are orthogonal and compose by **intersection** — traffic must satisfy *both*.

### 2.1 What each layer governs

| Layer | Scope | Granularity | Identity | Enforced at |
|---|---|---|---|---|
| **Tailscale tag-ACL** | tailnet (cross-machine / cross-cluster) | per-tag, per-port | **Tailnet node identity** (`tag:os-peer`) | `tailscale0` packet filter [PROJECT] |
| **K8s NetworkPolicy** | Pod-to-Pod *within a cluster*; CIDRs | per-label/namespace, per-port | **Pod labels / namespaces** | CNI dataplane [INFERRED] |

NetworkPolicy selects on Pod labels / IP CIDRs, **not** on `tag:os-peer`. So:
- Tailscale ACL: *"Is the remote tailnet node allowed to reach this port?"* (the real federation boundary).
- NetworkPolicy: *"Is this Pod allowed to talk to that Pod / CIDR on this port?"* (intra-cluster blast radius).

### 2.2 Compose & conflict

**Compose (intersection).** In the sidecar model, the leaf connection arrives at the Pod's `tailscale0` from a `100.64.0.0/10` source; the sidecar terminates the tunnel and hands **plaintext to loopback** — a hop most CNIs do **not** police [INFERRED]. So the decrypted federation traffic can have **zero cluster-network exposure**, and the ACL remains the sole real boundary for the link.

**Conflict / footgun.** The app's *other* ports (API `:3002`, local NATS `:4222`) are loopback-only on one machine. In a Pod, if any binds `0.0.0.0`, it becomes reachable on the **cluster Pod network** — a path the Tailscale ACL never sees. **NetworkPolicy is the only thing that can close that door.** The layers are not redundant: ACL covers the tailnet path (`:7422` cross-machine); NetworkPolicy covers the cluster path (everything Pod-to-Pod that never touches the tailnet).

### 2.3 Is the tailnet ACL still the real boundary?

**For the federation link: yes, primary and sufficient** [INFERRED] — decrypted traffic stays in-Pod. **For the cluster: NetworkPolicy becomes a newly-required second boundary** that didn't exist in Docker, because k8s' default-allow Pod network is a fresh attack surface. **k8s does not replace the tailnet ACL; it adds a co-equal second wall you are now obligated to build.**

### 2.4 NetworkPolicy YAML — equivalent of "src tag:os-peer → dst :7422, deny all else"

No native way to select on a Tailscale tag. Closest faithful expression: **default-deny, then allow only `:7422`**, scoping source to the CGNAT CIDR.

```yaml
# 1) Default-deny ALL ingress to the federating namespace
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: default-deny-ingress, namespace: federation }
spec:
  podSelector: {}
  policyTypes: ["Ingress"]
---
# 2) Allow ONLY :7422 from the tailnet CGNAT range
apiVersion: networking.k8s.io/v1
kind: NetworkPolicy
metadata: { name: allow-leaf-7422, namespace: federation }
spec:
  podSelector: { matchLabels: { app: os-peer } }
  policyTypes: ["Ingress"]
  ingress:
    - from:
        - ipBlock: { cidr: 100.64.0.0/10 }   # Tailscale CGNAT
      ports:
        - { protocol: TCP, port: 7422 }
```

**Honest caveat:** `ipBlock: 100.64.0.0/10` is *weaker* than `tag:os-peer` — it says "any tailnet node," not "any node *tagged* os-peer." Tag-level granularity lives **only** in the Tailscale ACL. The two layers are complementary, not substitutable; conflating them ("we have NetworkPolicy, drop the ACL") is a real regression in trust precision. Cilium can get closer to identity-aware policy [VERIFIED] but still selects on *Kubernetes* identity, not tailnet tags.

### 2.5 Intra-cluster vs cross-cluster (two clusters = two "machines")

- **Intra-cluster:** NetworkPolicy is the boundary; tailnet ACL irrelevant (traffic never hits the tailnet).
- **Cross-cluster:** Tailscale ACL is the boundary; each cluster's NetworkPolicy guards its own edge.

**Model two machines as two clusters** (two `kind`/`k3d` clusters) so the only inter-peer path is the tailnet — preserving the falsifiability of the CGNAT-IP control. A single-cluster two-Pod approximation lets the link short-circuit over the Pod network, *invalidating* the CGNAT assertion.

---

## 3. What Kubernetes changes about the model

### 3.1 NATS leaf/hub via the Helm chart

The official NATS Helm chart supports leafnode remotes via a config merge [VERIFIED]:

```yaml
config:
  jetstream:
    enabled: true
    domain: leaf-a            # distinct JetStream domain per island (required)
  merge:
    leafnodes:
      remotes:
        - url: "nats://hub.tailnet-magicdns:7422"
          # NO credentials block — tailnet identity is the auth (OpenStory model)
```

- **JetStream across the leaf boundary needs domains.** NATS blocks JetStream crossing the leaf boundary unintentionally, keyed on **JetStream domain**; distinct domains per island enable independent, disconnected-capable JetStream. [VERIFIED] Good for OpenStory: each machine keeps its own durable store; federation is an explicit, domain-scoped bridge.
- **No `credentials:` block.** The chart supports omitting it [INFERRED]. CI should assert federation succeeds **with no credentials present** — a positive control for "tailnet identity IS the auth."

### 3.2 Ephemeral Pod IPs vs stable tailnet identity

**Pod IPs are ephemeral; tailnet identity is stable** (a node keeps its `100.x` and tag across restarts given persisted state) [INFERRED]. So:
- Address the hub by **MagicDNS / stable tailnet IP, never Pod IP.**
- **Persist `tailscaled` state in a PVC** (or use stable tagged ephemeral keys) so identity survives reschedule — otherwise each reschedule churns a new tailnet node and breaks per-identity audit. A genuine new failure mode (test it, §4.8).

### 3.3 Multi-cluster federation

The tailnet flattens topology: clusters need no shared VPC, no public ingress, no exchanged CA — just `tag:os-peer` membership. Multi-cluster federation becomes a **membership** problem, not a **networking** problem [INFERRED] — the strongest argument *for* this model in k8s.

### 3.4 Data sovereignty

Unchanged in spirit, sharper in practice [PROJECT + INFERRED]: each cluster keeps its own JetStream + SqliteStore + JSONL appender (back the JSONL with a PVC). The tailnet ACL remains the *only* thing authorizing another machine to receive your events. No cloud broker; federation is peer-to-peer over `:7422`.

---

## 4. Scientific testing in runnable code — the core

Same method as the Docker harness: **direct path observation + falsifiable negative controls.** Every claim gets a positive control AND a negative control / ablation. Kubernetes adds **one native control we lack today**: NetworkPolicy enforcement (allow → connect; deny → timeout).

### 4.1 kind vs k3d — recommendation: **`kind` + a CNI that enforces NetworkPolicy (Cilium)**

| Factor | `kind` | `k3d` (k3s) |
|---|---|---|
| Default NetworkPolicy enforcement | **None** (kindnet no-ops) → forces explicit Cilium/Calico (what we want) | k3s ships a controller; for Cilium you disable traefik/servicelb/kubeproxy/network-policy [VERIFIED] |
| Multi-cluster (two "machines") | one-liner per cluster | also easy |
| `/dev/net/tun` for kernel-mode Tailscale | nest tun host→node→Pod | same |
| CI ubiquity / determinism | upstream-blessed | lighter/faster |

**Choose `kind` + Cilium:** kind has no built-in enforcement, which forces an *explicit, pinned* CNI — making enforcement an observable part of the rig, not an implicit controller. kindnet would silently no-op deny policies — a false-green disaster for a *negative* control.

### 4.2 Hermetic Tailscale / Headscale

- **Control server:** Headscale as a Deployment; sidecars point `--login-server` at its Service.
- **Auth:** pre-generate a **tagged pre-auth key** (`tag:os-peer`) in a CI init step; inject as a Secret.
- **TLS caveat (§5):** hermetic Headscale can be HTTP-only for coordination, but **DERP relay requires TLS** [VERIFIED]. Engineer the topology so peers connect **directly** (kind node containers share a Docker network) and never need DERP. If you force-block the direct path, federation should fall back to DERP and (HTTP-only) **fail** — a known, expected limitation.

### 4.3 Framework: **Go + `sigs.k8s.io/e2e-framework`**, with `cilium connectivity test` as a structural model

- **kuttl** — too declarative for "assert the live socket's remote IP is CGNAT" / "read the compiled packet filter."
- **`cilium connectivity test`** — right *mental model* (deploy probes, run allow/deny matrix, flip policy, re-run); borrow the structure.
- **Go + e2e-framework** *(recommended)* — programmatic: spin up kind clusters, install Cilium + Headscale, deploy Pods, **`exec` into Pods** (needed to read `tailscale debug netmap` and inspect the live NATS connection), apply/delete NetworkPolicies between assertions, express positive+negative controls as subtests. Matches OpenStory's "tests as artifacts" ethos.

### 4.4 Claim → control matrix

| # | Claim | Positive | Negative control (ablation) |
|---|---|---|---|
| 1 | Federation works over the tailnet | publish A → consume B | — |
| 2 | Link rides the tailnet, not the bridge | NATS remote IP ∈ `100.64.0.0/10` | (covered by #5/#6) |
| 3 | Tailnet identity IS the auth (no token) | connects with **no** credentials | wrong/absent **tag** → ACL denies |
| 4 | The ACL is the boundary | allow `:7422` → up | **deny `:7422` → federation STOPS** |
| 5 | `tailscale down` drops the link | up → socket present | **`tailscale down` → socket DROPS** |
| 6 | Only `:7422` permitted | filter == TCP/UDP `:7422` | probe `:7421/:7423` → refused |
| 7 | **(k8s-native, new)** NetworkPolicy enforces the cluster edge | **allow policy → connect** | **deny policy → connect TIMES OUT** |
| 8 | Identity stable across reschedule | delete Pod → re-federates | no-PVC variant → identity churns |

Claims 4, 5, 7 are the most important runnable tests.

### 4.5 Test A — the **k8s-native NetworkPolicy** control (the one we lack today)

The trap: on a CNI that doesn't enforce policy, a *deny* is a silent no-op → **false-green**. The test must be guarded by a **meta-control** proving the CNI enforces at all.

```go
func TestNetworkPolicyEnforcesLeafPort(t *testing.T) {
    feat := features.New("networkpolicy-enforcement").
        // META-CONTROL: prove this CNI ACTUALLY enforces deny, else the test is meaningless.
        Assess("cni enforces deny (guard against false-green)", func(ctx context.Context, t *testing.T, c *envconf.Config) context.Context {
            applyDenyAllIngress(ctx, c, "federation")
            out := execInPod(ctx, c, "canary-client", "federation", "nc", "-z", "-w", "3", "os-peer.federation.svc", "7422")
            require.NotZero(t, exitCode(out), "deny-all did NOT block — CNI not enforcing; test invalid")
            deleteDenyAllIngress(ctx, c, "federation")
            return ctx
        }).
        Assess("allow policy -> connect succeeds", func(ctx context.Context, t *testing.T, c *envconf.Config) context.Context {
            applyAllowLeaf7422(ctx, c, "federation")
            out := execInPod(ctx, c, "canary-client", "federation", "nc", "-z", "-w", "5", "os-peer.federation.svc", "7422")
            require.Zero(t, exitCode(out), "allow present but :7422 unreachable")
            return ctx
        }).
        Assess("deny policy -> connect TIMES OUT", func(ctx context.Context, t *testing.T, c *envconf.Config) context.Context {
            applyDenyLeaf7422(ctx, c, "federation")
            start := time.Now()
            out := execInPod(ctx, c, "canary-client", "federation", "nc", "-z", "-w", "5", "os-peer.federation.svc", "7422")
            require.NotZero(t, exitCode(out), "deny present but :7422 still reachable — NOT enforced")
            require.GreaterOrEqual(t, time.Since(start), 4*time.Second, "expected a TIMEOUT (drop), got fast refuse")
            return ctx
        }).Feature()
    testenv.Test(t, feat)
}
```

The **timeout-vs-refuse distinction** is the scientific tell: a *dropped* packet (policy enforced) → **timeout**; a *refused* connection (no listener) → **fast failure**. Asserting `>= 4s` proves the failure was a **drop by policy**, not a missing service — a falsifiability detail kuttl can't express.

### 4.6 Test B — ACL deny `:7422` ablation (claim 4) + CGNAT-IP observation (claim 2)

Two clusters; the only inter-peer path is the tailnet.

```go
func TestTailnetACLIsTheBoundary(t *testing.T) {
    feat := features.New("tailnet-acl-boundary").
        Assess("federation up over CGNAT, not bridge", func(ctx, t, c) context.Context {
            waitLeafConnected(ctx, c)
            remote := natsLeafRemoteIP(ctx, c)   // GET :8222/leafz -> .leafs[0].ip
            require.True(t, inCIDR(remote, "100.64.0.0/10"), "leaf remote %s NOT CGNAT — short-circuiting over the bridge", remote)
            require.False(t, inCIDR(remote, "172.16.0.0/12"), "remote is a Docker bridge IP — NOT the tailnet")
            return ctx
        }).
        Assess("deny :7422 in ACL -> federation stops", func(ctx, t, c) context.Context {
            headscaleSetACL(ctx, denyAll())
            require.Eventually(t, func() bool { return leafnodeCount(ctx, c) == 0 }, 30*time.Second, time.Second,
                "ACL denies :7422 but leaf STILL connected — ACL is not the boundary")
            headscaleSetACL(ctx, allowPeer7422())
            return ctx
        }).Feature()
    testenv.Test(t, feat)
}
```

The remote IP must be **CGNAT** and explicitly **not** `172.16/12` — proving two-cluster traffic genuinely traverses the tailnet.

### 4.7 Test C — `tailscale down` ablation (claim 5) + packet-filter assertion (claim 6)

```go
func TestTailscaleDownDropsLeaf(t *testing.T) {
    feat := features.New("tailscale-down-ablation").
        Assess("packet filter is exactly :7422", func(ctx, t, c) context.Context {
            nm := execInPod(ctx, c, "os-peer", "federation", "tailscale", "debug", "netmap")
            require.Equal(t, []PortRange{{7422, 7422}}, parsePacketFilter(nm).DstPorts)
            return ctx
        }).
        Assess("tailscale down -> leaf socket drops", func(ctx, t, c) context.Context {
            require.Equal(t, 1, leafnodeCount(ctx, c))
            execInPod(ctx, c, "tailscale", "federation", "tailscale", "down")
            require.Eventually(t, func() bool { return leafnodeCount(ctx, c) == 0 }, 20*time.Second, time.Second,
                "tailscale down but leaf still up — link not actually on the tailnet")
            execInPod(ctx, c, "tailscale", "federation", "tailscale", "up", "--login-server=https://headscale.internal")
            return ctx
        }).Feature()
    testenv.Test(t, feat)
}
```

### 4.8 Test D — reschedule / identity stability (claim 8, new k8s control)

```go
func TestIdentityStableAcrossReschedule(t *testing.T) {
    feat := features.New("reschedule-identity").
        Assess("delete leaf pod -> re-federates with SAME tailnet identity", func(ctx, t, c) context.Context {
            idBefore := tailnetNodeID(ctx, c, "os-peer", "federation")
            deletePod(ctx, c, "os-peer", "federation")
            waitPodReady(ctx, c, "app=os-peer", "federation")
            require.Eventually(t, func() bool { return leafnodeCount(ctx, c) == 1 }, 60*time.Second, 2*time.Second, "leaf did not re-federate")
            require.Equal(t, idBefore, tailnetNodeID(ctx, c, "os-peer", "federation"),
                "tailnet identity CHANGED across reschedule — audit broken; need persisted state")
            return ctx
        }).Feature()
    testenv.Test(t, feat)
}
```

The no-PVC variant should show `idBefore != idAfter`, proving the persistence requirement is real.

### 4.9 CI orchestration sketch

```bash
# scripts/k8s-federation-e2e.sh (artifact, not rawdogged inline — per CLAUDE.md §9)
set -euo pipefail
kind create cluster --name peer-a --config kind-cilium.yaml
kind create cluster --name peer-b --config kind-cilium.yaml
cilium install --context kind-peer-a; cilium install --context kind-peer-b
helm install headscale ./charts/headscale -n headscale --create-namespace --context kind-peer-b
TS_KEY=$(headscale-create-tagged-key tag:os-peer)
helm install nats nats/nats -f values-leaf.yaml --context kind-peer-a
helm install nats nats/nats -f values-hub.yaml  --context kind-peer-b
kubectl apply -f manifests/os-peer-pod.yaml --context kind-peer-a
kubectl apply -f manifests/networkpolicy/  --context kind-peer-a
go test ./e2e/...   # Tests A–D
kind delete cluster --name peer-a; kind delete cluster --name peer-b
```

---

## 5. Honest risks & open questions

1. **DERP relay needs TLS; hermetic HTTP Headscale can't relay.** [VERIFIED] Our Docker harness hit exactly this. **K8s implication:** CI must guarantee a **direct** peer path (kind node containers on one Docker network) so federation never needs relay. Real multi-cluster across NATs/firewalls **must** give Headscale TLS + a DERP node, or peers silently fail. *The single biggest "works in CI, breaks in prod" risk.* Mitigation: a separate non-hermetic tier with TLS Headscale + DERP, run less often.
2. **TLS for Headscale generally.** Production wants TLS; the HTTP shortcut is test-only — don't leak it into deploy docs (`feedback_deploy_existing_backlog_new`). Open question: real tailnet vs Headscale in prod (ACL model identical either way).
3. **Operator's tsnet breaks "app binds localhost."** [INFERRED, §1b] If HA later needs the operator, confirm with a spike — identity-collocation is load-bearing for the no-token claim. Can a ProxyGroup sidecar *into* the app Pod's netns? Current docs suggest no.
4. **MagicDNS in k8s.** Stable-name addressing depends on MagicDNS resolving inside the Pod; can collide with CoreDNS search/ndots [INFERRED]. Test: resolve `hub.tailnet` from inside the app container.
5. **Scale / resources.** One sidecar per federating Pod is cheap at two machines; flag only if federation fans out.
6. **CI runner constraints — `/dev/net/tun`, privileged, `NET_ADMIN`.** [INFERRED] Kernel mode needs both, nested through kind (host → node → Pod). Userspace mode avoids the device but **changes the very property under test** (breaks localhost-binding) — so a userspace CI fallback measures a *different* architecture. Open question: does hosted GitHub Actions permit tun+NET_ADMIN through nested kind? If not, kernel-mode tests need a self-hosted runner; hosted CI runs only the NetworkPolicy-layer tests. *Decide before building the full harness.*
7. **False-green on NetworkPolicy.** [VERIFIED risk] kindnet doesn't enforce; a deny silently passes. The Test A meta-control is mandatory.
8. **Two-cluster vs one-cluster fidelity.** One-cluster two-Pod lets the link short-circuit the Pod network, *invalidating* the CGNAT control (§2.5). The CGNAT assertion is the canary: a bridge IP means the two "machines" collapsed into one.

---

## Summary of recommendations

- **Topology:** Pod-level Tailscale sidecar sharing the netns (§1a) — the only faithful port. Backlog the Operator for HA/scale.
- **Trust boundary:** two co-equal layers. Tailscale tag-ACL stays the *tag-precise* federation boundary; NetworkPolicy is a *newly-required* default-deny + port-pin on the cluster edge. NetworkPolicy can only approximate the ACL with `ipBlock: 100.64.0.0/10` (weaker than `tag:os-peer`) — **don't drop the ACL.**
- **Two machines = two clusters,** so the only inter-peer path is the tailnet (preserves CGNAT-IP falsifiability).
- **NATS:** Helm leafnode remotes, **no credentials block**, distinct JetStream domains per island, durable stores on PVCs.
- **Testing:** `kind` + Cilium, Go + `sigs.k8s.io/e2e-framework`, in-cluster Headscale (HTTP, direct-path only). Positive + falsifiable negative control per claim, mirroring the Docker ablations, **plus** the new NetworkPolicy allow/deny control — guarded by a meta-control proving the CNI enforces deny.
- **Biggest risk:** DERP-needs-TLS (hermetic harness only works on a direct path) and `/dev/net/tun`/`NET_ADMIN` through nested kind (may force a self-hosted runner). Resolve the CI-capability question first.

---

### Sources (verified)

- Tailscale Kubernetes Operator / ProxyGroup / Services / ProxyGroupPolicy — tailscale.com/kb/1236, operator architecture, cluster egress, proxy-group policy, kb/1439 HA ingress
- kind vs k3d + Cilium NetworkPolicy enforcement — blogops.mixinet.net testing-cilium-with-k3d-and-kind; johal.in cilium-116-vs-calico-328
- NATS Helm leafnode remotes / JetStream domains — github nats-io/k8s values.yaml; docs.nats.io leafnodes/jetstream_leafnodes
- Headscale DERP requires TLS — headscale.net/stable/ref/derp; juanfont/headscale config-example.yaml
