# ADR 0003 — k3s on a single host for V0

**Status:** Accepted
**Date:** 2026-05-07

## Context

The lab needs a workload orchestrator. OpenStory and NATS are the two services; sharing controls, OIDC, Coder workspaces, microVMs are all in BACKLOG. V0 is intentionally scoped to one Hetzner box.

The realistic options for the workload layer:
- **k3s** (Rancher's lightweight Kubernetes — single binary, ~50MB)
- **Plain systemd units + Docker** (no orchestrator)
- **Nomad + Consul** (HashiCorp stack)
- **Full upstream Kubernetes (kubeadm)**

Single-host workload management does not *require* Kubernetes. Plain systemd would be simpler and smaller. The reason to add k3s is forward compatibility — the BACKLOG includes Coder workspaces, multi-host replication, federated topologies, Firecracker microVM substrate. All of those compose cleanly with Kubernetes primitives. A V0 that's "systemd everything" would need to be ripped out for V1.

## Decision

Use **k3s** in single-server mode on the lab host. Disable the bundled Traefik (we control ingress separately) and servicelb (single-host doesn't need a cluster LB; pods bind via hostPort or Caddy on the host). Workloads (OpenStory, NATS) install via Helm charts under `charts/`.

## Consequences

- **One declarative path** from V0 to V1+. Adding Coder, Keycloak, more replicas means new charts and modules, not a new orchestration story.
- **Helm becomes the unit of workload**. `charts/openstory/values.yaml` mirrors `rs/server/src/config.rs` Config fields; the helm-unittest suite enforces parity.
- **Some overhead** for V0 — k3s adds ~150MB RAM and a layer of complexity vs raw systemd.
- **Network policies** become available immediately (`charts/openstory/templates/networkpolicy.yaml` denies cross-namespace ingress to NATS).
- **OPA policies** (Conftest) work against rendered Helm output, giving the lab a falsifiable deployment-time invariant check before any workload starts.

## Alternatives considered

- **Plain systemd + Docker** — simpler V0, expensive V1 migration. Rejected on forward-compatibility grounds.
- **Nomad** — HashiCorp ecosystem licensing question (BSL); doesn't compose with the broader Kubernetes ecosystem we'll need for Coder/Keycloak. Rejected.
- **Full upstream Kubernetes (kubeadm)** — fine for clusters; overkill for a single host. k3s is the right rung.
