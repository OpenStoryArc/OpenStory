# Scientific Validation — does federation *really* ride the tailnet, and is the ACL *really* the boundary?

**Status:** ✅ 12/12 assertions PASS, deterministically, on a1 (native Linux,
x86_64, Docker 29.1.3). Run it:
`ssh a1 'cd ~/tailnet-harness && bash run.sh prove'` (or locally where Docker has
`/dev/net/tun` + `NET_ADMIN`). The experiment is the `prove` subcommand of
`harness/run.sh`.

## Why a "B received the session" test proves nothing

The naive test — publish on A, assert it appears on B — is **not** evidence the
Tailscale tailnet did anything. If A and B share any network, B might reach A's
NATS over the docker bridge, and the test passes *whether or not the tailnet is
load-bearing*. A real validation has to be **falsifiable**: each claim needs a
**negative control** that breaks federation when — and only when — the thing
under test is removed. That's the difference between a demo and an experiment.

## The two hypotheses, and how each is made falsifiable

**H1 — NATS federation rides the Tailscale tailnet, not a bridge.**
Three independent lines of evidence, the last one causal:

1. *Path by construction.* The NATS leaf dials `100.64.0.1` — a **CGNAT address
   (`100.64.0.0/10`, RFC 6598)** routable *only* via `tailscale0`. It is
   structurally not a docker-bridge IP, so the destination itself fixes the path.
2. *Direct observation, two vantage points.* The **hub** reports (`/leafz`) the
   leaf arriving from remote IP `100.64.0.2` — a tailnet IP, the hub's own
   testimony. The **leaf** side shows a live `ESTABLISHED` socket to
   `100.64.0.1:7422` in `/proc/net/tcp`.
3. *Causal ablation (the keystone).* In node-b, `iptables -I OUTPUT -d
   100.64.0.1 -j DROP` severs **only the route to the CGNAT hub IP** (the node and
   tailscaled stay up). The socket to `100.64.0.1:7422` **must** vanish and the
   hub drops the leaf. A bridge path would be unaffected by dropping a tailnet IP.
   Then delete the rule → federation **heals on its own** (the node never died).
   The route to the tailnet peer is a clean reversible on/off switch for
   federation — causation, not correlation.

**H2 — the tag-based ACL is the real permission boundary.**

1. *Port scope.* `:7422` connects; `:9999` over the tailnet is **refused**; the
   **compiled packet filter** (`tailscale debug netmap`) reads back as exactly
   `IPProto[6,17]` (TCP/UDP) to port `7422` and nothing else.
2. *Policy ablation.* Rebuild the **identical** stack changing **only** the ACL
   port `7422 → 9999` (`headscale/acl-deny.json`). The leaf then **never**
   connects. One variable changed — the policy — and federation vanished. That
   isolates the ACL as the cause, not an incidental firewall or default.

Each "must fail" control changes exactly one variable and predicts collapse. If
any of them had *passed* (federation surviving the ablation), the corresponding
hypothesis would be falsified. None did.

## The experiment (`run.sh prove`) and result

12 assertions across 4 experiments. Run on a1:

| Exp | Assertion | Type | Result |
|-----|-----------|------|--------|
| E1a | hub sees exactly 1 leaf connection | positive | PASS |
| E1b | hub reports leaf arriving from a `100.64/10` IP | path obs | PASS |
| E1c | live ESTAB socket to `100.64.0.1:7422` in node-b | path obs | PASS |
| E1d | a published event crosses leaf→hub | positive | PASS |
| E2a | `:7422` over the tailnet connects | positive | PASS |
| E2b | `:9999` over the tailnet **refused** | neg. control | PASS |
| E2c | compiled filter == TCP/UDP `:7422` only | direct read | PASS |
| E3a | drop route to `100.64.0.1` → hub drops the leaf to 0 | **causal ablation** | PASS |
| E3b | ESTAB socket to `100.64.0.1:7422` is gone | causal ablation | PASS |
| E3c | restore route → leaf reconnects (reversible switch) | causal ablation | PASS |
| E4a | deny-`:7422` ACL → leaf **never** connects | **policy ablation** | PASS |
| E4b | `:7422` over the tailnet now refused under deny policy | policy ablation | PASS |

**Conclusion: H1 and H2 supported.** Every positive control held, and every
negative control falsified federation *exactly* when the tailnet (E3) or the ACL
(E4) was removed — and E3c showed federation restored when the tailnet returned.

## Hard-won robustness lessons (encoded in the harness)

These are why the experiment is *deterministic* and not a flaky demo:

1. **Poll, never fixed-sleep.** The NATS leaf needs ~10–20s to establish on a1;
   a `sleep 6` then-read produced false negatives. Every state check polls with a
   timeout (`wait_leaf`). First runs "failed" purely from reading too early.
2. **Dead-leaf detection is ping-based, not instant.** `tailscale down` severs
   the wire with no TCP FIN, so the hub only notices via ping-timeout. We set
   `ping_interval 5s` / `ping_max 2` so detection is a bounded ~10–15s; the
   *immediate* causal signal is the socket disappearing (E3a), not the hub count.
3. **`tailscale down` kills the node container.** In the `tailscale/tailscale`
   image, `tailscale down` makes `containerboot` (PID 1) exit, which kills the
   node and any sidecar sharing its netns — so federation could never *heal*
   (there was nothing to heal to; the nats-leaf logged endless "network is
   unreachable" against a dead netns). The fix is a **surgical, reversible**
   ablation: `iptables` DROP on the hub's CGNAT IP cuts only the path, leaves the
   node alive. This is also *better science* — it ablates exactly "reaching the
   tailnet peer," not "the whole node."
4. **Bash functions are invisible inside `sh -c`.** Predicates are real functions
   called directly by the `chk` harness, never via `sh -c`.

## A real security finding the method surfaced (this is the point)

Porting the experiment to Rust on macOS, the E3 ablation *failed* in an
illuminating way: with the tailnet cut, a published event **still crossed**.
Decoding `/proc/net/tcp` showed why — the NATS leaf had **reconnected to the hub
over the docker-bridge IP `172.x:7422`**, not the tailnet. The hub had gossiped
its other interface addresses to the leaf via NATS `connect_urls`, handing it a
**non-tailnet fallback path**. Whenever any non-tailnet route exists between the
two machines, federation — and therefore the tailnet ACL boundary — could be
**silently bypassed**.

This is exactly what falsifiable testing is for: a "B received it" demo would
have shown green and hidden the leak. The negative control exposed it.

**Fix (a real hardening, now in `nats/hub.conf`):** pin the hub to advertise only
its tailnet address —
```
leafnodes { listen: 0.0.0.0:7422, advertise: "<hub-tailnet-ip>:7422" }
```
so the leaf never learns a non-tailnet fallback. Verified: after the fix, cutting
`tailscale0` severs federation with **no bridge reconnect and no event crossing**.
Production guidance: the hub must advertise its tailnet (MagicDNS/CGNAT) address,
never `0.0.0.0`-detected interfaces.

## A known limitation, stated honestly

We do **not** prove isolation "by construction" (disjoint docker networks so no
bridge path can exist). We tried: it forces all inter-node traffic onto Tailscale
**DERP relay**, and a hermetic **HTTP** Headscale cannot relay — **DERP requires
TLS** (independently confirmed by the Kubernetes research, see
[KUBERNETES.md](KUBERNETES.md) §5). So the disjoint-network variant times out.

Instead we use a shared underlay (direct WireGuard works) and prove the path by
**observation + causal ablation**, which is arguably stronger: it shows the
mechanism (the socket is on a CGNAT IP and dies with the tailnet), not merely the
absence of an alternative. Full isolation-by-construction is achievable with a
TLS-enabled DERP and is noted as future hardening.

## The runnable CI form — DONE

`rs/tests/test_tailnet_federation.rs` encodes the whole experiment as a Rust
integration test (the project's `Stack`+`Drop`+`Command` convention, `#[ignore]`
by default like the other Docker tests). It ports all 13 assertions into
first-class `assert!`s with the eventual-consistency polling baked in. **Validated
green on macOS Docker Desktop** (`test result: ok. 1 passed`), complementing the
bash harness's 12/12 on a1 (native Linux) — two independent environments.

```
cargo test -p open-story --test test_tailnet_federation -- --ignored --nocapture
```

It also improves on the bash E3 in two ways the macOS port forced: (1) the tailnet
partition blocks the whole `tailscale0` interface (routing-independent, not a
dest-IP match that flapped on Docker Desktop); (2) E3 asserts *functional*
severance (a published event must not cross, then must cross again on heal)
rather than NATS's lazy hub-side leaf-count bookkeeping.

The [K8S_TEST_PLAN.md](K8S_TEST_PLAN.md) extends the same method to Kubernetes on
the repo's existing `K3sCluster` testcontainer infra. The
[KUBERNETES.md](KUBERNETES.md) report extends the *same* falsifiable-control
method to k8s and adds a control we lack today: NetworkPolicy allow/deny
enforcement, guarded by a meta-control that proves the CNI actually enforces deny
(else a deny policy is a silent no-op and the test goes false-green).
