# Tailnet Federation — prototype results

**Date:** 2026-06-13 · **Status:** prototype GREEN, hermetic, reproducible.

Run it: `cd harness && ./run.sh up && ./run.sh test` (`./run.sh down` to tear down).
Requires only Docker. No real Tailscale account, no secrets, no internet
dependency — Headscale is the self-hosted control server.

## What was proven

| Rung | Claim | Result |
|------|-------|--------|
| P0 | Self-hosted control plane (Headscale + embedded DERP) stands up | ✅ |
| P1 | Two nodes join the tailnet programmatically via a minted, **tagged** preauth key, get kernel-mode routing | ✅ `100.64.0.1` / `100.64.0.2`, both `tag:os-peer` |
| P2 | A 4-line tag-based ACL gates traffic to one service port | ✅ `:7422` connects, `:9999` **blocked** |
| P3 | NATS federates **bidirectionally** over the tailnet with **no shared token** — the tailnet is the auth layer | ✅ leaf↔hub both directions |
| P4 | Two real **OpenStory** instances federate a session over the tailnet (watcher→translate→publish→leaf→ingest→API) | ✅ session crossed in ~1s, provenance `host:os-node-a` preserved |

Final test output:

```
== ACL enforcement (clubhouse policy = os-peer -> os-peer:7422 only) ==
  :7422 allowed -> CONNECTED
  :9999 denied  -> blocked
  ICMP (expected reachable, not a service path) -> reachable
== NATS federation over the tailnet (bidirectional) ==
  hub sees leaf_count=1
  leaf->hub propagated -> OK
  hub->leaf propagated -> OK
ALL GREEN
```

## The topology (and how it maps to production)

```
        docker network os-tailnet
  ┌────────────┐
  │ hs-control │  Headscale control server + embedded DERP (hermetic)
  └────────────┘
  os-node-a (tag:os-peer, 100.64.0.1)     os-node-b (tag:os-peer, 100.64.0.2)
   └─ nats-hub  (shares netns)             └─ nats-leaf (shares netns)
        listen :7422  ◄───── tailnet, ACL-gated, tokenless ─────  dials 100.64.0.1:7422
```

Each `os-node-X` is a **Tailscale sidecar** giving a machine its tailnet
identity; NATS (and, in P4, OpenStory itself) ride in that network namespace
via `--network container:os-node-X`. This is the key production insight: the
Rust server **never embeds Go `tsnet`** — a sidecar container provides the
tailnet, the Rust process just binds `0.0.0.0` and is reachable on the tailnet
IP. Mirrors the peer project's tailnet-sidecar pattern exactly.

## Why this beats bridging personal tailnets

Every pain point from the earlier hub-to-hub-over-personal-tailnets sketch
dissolves:

- **"Whose ACL gates port 7422?"** → one purpose-built tailnet, one shared
  4-line ACL. No cross-personal-tailnet node-sharing.
- **"His port comes back filtered and only he can fix it."** → tags + minted
  keys; membership is symmetric and self-served.
- **"Token visible in `nats://TOKEN@host` in `ps`."** → gone. No NATS token at
  all; the tailnet identity is the auth.
- **Fragile manual node sharing** → nodes *join* programmatically with an
  ephemeral, tagged key and clean themselves up.

OpenStory's data architecture is untouched — NATS leaf/hub, durable JetStream,
bidirectional propagation. We swapped only the messy network layer for a clean
one. This is "tsnet *under* NATS", the low-disruption / high-fit option.

## Learnings (the non-obvious bits, encoded in the harness)

1. **Headscale v0.28 config gotchas:** `dns.override_local_dns: false` +
   `dns.nameservers.global: []` are required or boot fails; policy-v2 tag owners
   must be a user-with-`@` (`club@`), not a bare username.
2. **Tag on the key, not the node.** Stamping `--tags tag:os-peer` on the
   preauth key makes nodes register *pre-tagged*, inheriting the restrictive
   filter from birth. Tagging a live node + reloading policy does **not** push
   to already-connected nodes (their netmap goes stale).
3. **Don't poke containerboot nodes.** Running `tailscale up --reset` inside a
   `tailscale/tailscale` container kills its PID-1 `containerboot`, taking down
   any sidecar sharing its netns. Recreate the node instead.
4. **Kernel mode needs `--cap-add NET_ADMIN --device /dev/net/tun`.** Available
   locally and on standard CI runners; fall back to `TS_USERSPACE=true` where
   `/dev/net/tun` is absent (loses native routing; needs the SOCKS proxy).
5. **ICMP is always allowed to a granted host.** The packet filter is
   `IPProto[6,17]` (TCP/UDP) scoped to `:7422`; ICMP echo still flows. It's a
   reachability affordance, not a service path — the port scope is the real
   boundary.
6. **Sidecar wireguard warmup:** first ICMP/TCP after join can drop while the
   WireGuard handshake completes (~1s); retry once.

## P4 evidence

```
== session present at origin (openstory-a)? ==     A has tailnet-demo-001 -> OK
== session FEDERATED to openstory-b over the tailnet? ==
  B received tailnet-demo-001 after 1s -> OK
P4 GREEN: a real OpenStory session crossed the tailnet
```

`openstory-a` read a synthetic-but-real Claude transcript (`gen_session.py`,
mirroring `rs/tests/helpers/synth.rs`) on boot-scan, translated + published to
its sidecar NATS hub; the event propagated over the ACL-gated tokenless `:7422`
to `openstory-b`'s sidecar leaf, was ingested, and surfaced in B's
`/api/sessions` with `host:os-node-a` — provenance preserved across the tailnet.

**Boot ordering matters** (encoded in `run.sh p4`): bring up B *before* A so B's
consumer is subscribed and the leaf stream is synced before A publishes — else
A's events land only in the hub stream and B (a late subscriber) misses the live
propagation. This is the one real sequencing constraint of leaf+JetStream.

## Next steps toward the goal (testcontainers CI test)

The shell harness is the **spec**; the goal is the same flow as a Rust
`testcontainers` test in `rs/tests/`, next to `test_multi_leaf.rs`.

- **Path A (fastest, recommended first):** author `rs/tests/docker-compose.tailnet.yml`
  expressing this topology — `network_mode: "service:os-node-a"` for the sidecars,
  `cap_add: [NET_ADMIN]` + `devices: [/dev/net/tun]` on the tailscale services,
  Headscale bootstrap via an init step. A Rust test brings it up (mirroring
  `start_multi_leaf_stack`), drops a fixture, polls `/api/sessions`. CI runners
  expose `/dev/net/tun`; fall back to `TS_USERSPACE` where absent.
- **Path B:** a thin `testcontainers`-rs harness that shells the container wiring
  directly. More code; only worth it if compose proves limiting.
- **Headscale bootstrap in CI:** the user/preauth-key minting (currently
  imperative `docker exec headscale ...`) becomes a one-shot init container so the
  stack is declarative.
- **Real-world variant (off-harness):** point a node at the actual peer tailnet
  with a tagged auth key (real Tailscale, not Headscale) to confirm the same ACL
  shape holds outside the hermetic harness.
