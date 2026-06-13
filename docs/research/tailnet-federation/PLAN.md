# Tailnet Federation — prototype plan (spike)

**Status:** P0–P4 PROVEN + scientifically VALIDATED (2026-06-13). Hermetic harness
green end-to-end: `harness/run.sh up && run.sh test && run.sh p4 && run.sh p4test`.
A real OpenStory session federates between two instances over the tailnet, and
`run.sh prove` is a **12/12 controlled experiment** (path observation + falsifiable
negative controls) validated on a1 (native Linux). The Rust `testcontainers`
encoding **`rs/tests/test_tailnet_federation.rs` is GREEN on macOS** too. The
experiment caught (and fixed) a real ACL-bypass — the NATS leaf falling back to
the docker bridge — hardened via `leafnodes { advertise: ... }`. See
[VALIDATION.md](VALIDATION.md), [RESULTS.md](RESULTS.md), [KUBERNETES.md](KUBERNETES.md),
[K8S_TEST_PLAN.md](K8S_TEST_PLAN.md). Remaining: implement the k8s tests (Phase 1
first). Still incubating in `docs/research/` until then.

Proves a cleaner way for two OpenStory instances to share events over a network,
modelled on a pattern a peer already solved (a `tsnet` + tagged-tailnet +
minimal-ACL design from an inference project).

## The idea

Stop bridging two *personal* tailnets (whose-ACL-gates-which-port pain). Instead
stand up a small **purpose-built tailnet** that services *join programmatically*
as **tagged, ephemeral** nodes, governed by a tiny tag-based ACL:

```jsonc
{ "action": "accept", "src": ["tag:os-peer"], "dst": ["tag:os-peer:7422"] }
```

Keep OpenStory's data plane (NATS leaf/hub, durable JetStream, bidirectional
propagation) — just run it over this clean network instead of glued personal
tailnets. This is "tsnet *under* NATS", the low-disruption, high-fit option.

## Why Headscale for the prototype

Real Tailscale needs `login.tailscale.com` + secret auth keys — not hermetic,
not CI-safe. **Headscale** is the open-source Tailscale control server. Run it as
a container; both nodes join *it* via `--login-server`. Fully self-contained, no
external dependency, no secrets. (The peer's agent register-response even carries
a `tailscale_login_server` field — that's exactly this seam.)

## Environment preflight (verified 2026-06-13)

- Docker Desktop 28.3.3, linux/aarch64
- `/dev/net/tun` present with `--cap-add NET_ADMIN` → kernel-mode tailscale, native routing
- `headscale/headscale` + `tailscale/tailscale` images pull cleanly

## Build ladder (verify each rung by running it)

- **P0 — control plane.** Headscale up; create a user + preauth keys.
- **P1 — two nodes join.** Two `tailscale` containers register against headscale,
  get tailnet IPs, ping each other. (Proves the clubhouse tailnet, hermetic.)
- **P2 — ACL enforcement.** Apply the tag-based ACL; prove the allowed port is
  reachable and a disallowed port is blocked.
- **P3 — NATS over the tailnet.** NATS hub on node A, leaf on node B dialing A's
  tailnet IP:7422; publish on one, observe on the other. (Data plane rides the
  clean network = the whole point.)
- **P4 — OpenStory over the tailnet (stretch).** Swap plain nodes for
  `open-story:test` containers; propagate a real session event between two
  instances.

## Deliverable

A hermetic harness (driver script first for fast iteration, then encoded as a
Rust `testcontainers` test under `rs/tests/`) that brings up headscale + two
nodes and asserts an event published on one instance appears on the other —
purely over the headscale-managed tailnet. Living reference doc for the topology.

## Open questions to resolve in-flight

1. Same-docker-network nodes: do they get direct connections, or need DERP? (Plan:
   enable headscale's embedded DERP region to stay hermetic.)
2. Headscale CLI surface drifts across versions — pin a version once it works.
3. Kernel vs userspace tailscale in CI — kernel works locally; confirm CI runners
   expose `/dev/net/tun` or fall back to `TS_USERSPACE`.
