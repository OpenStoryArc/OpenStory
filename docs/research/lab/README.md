# OpenStory Lab — deployable artifact (V0)

> *Nullius in verba* — take nobody's word for it. Run `tofu plan` against this directory and read the truth.

This is the lab's deployable artifact: a declarative description of an OpenStory deployment that anyone can read, fork, and apply. The repo IS the spec.

V0 deploys exactly what already runs end-to-end today (Rust binary, NATS bus, single-Person config), expressed in NixOS + OpenTofu + k3s + Helm + SOPS. New product features (sharing controls, multi-Person, OIDC, Cloud IDE workspaces, microVM hardening) are tracked in [`docs/BACKLOG.md`](../../BACKLOG.md).

## Why this lives in `docs/research/`

This is exploratory infrastructure work paired with the ongoing identity research ([personhood-and-principals](../personhood-and-principals.md), [nats-permissions-spike](../nats-permissions-spike.md)). It graduates out of `docs/research/` to a top-level `lab/` directory once it has driven a real deploy and stayed stable across at least one round of changes.

## Layout

```
docs/research/lab/
├── flake.nix                    # NixOS inputs (nixpkgs 24.11, sops-nix)
├── hosts/
│   └── lab-host.nix             # canonical V0 host
├── modules/                     # NixOS module composition
│   ├── k3s.nix                  # k3s server + helm-bootstrap unit
│   ├── openstory.nix            # host prep (data dir, perms)
│   ├── nats-hub.nix             # NATS hub config (composes deploy/nats-hub.conf)
│   └── sops.nix                 # SOPS integration via sops-nix
├── charts/                      # Helm charts (workload layer)
│   ├── openstory/               # OpenStory server (mirrors rs/server/src/config.rs)
│   └── nats/                    # NATS hub (composes deploy/nats-hub.conf)
├── tofu/                        # OpenTofu (Hetzner host + Cloudflare DNS)
│   ├── main.tf
│   ├── variables.tf
│   ├── outputs.tf
│   └── profiles/
│       └── single-host.tfvars   # the V0 profile
├── policy/                      # OPA / Conftest invariants
│   ├── encrypted-storage-required.rego
│   ├── api-token-required.rego
│   ├── no-token-in-url.rego
│   ├── nats-not-public.rego
│   └── policy_test.rego
├── conformance/
│   ├── spin_up_and_probe.sh     # CI: kind cluster → deploy → probe → teardown
│   └── smoke.sh                 # Post-deploy smoke against real URL
├── secrets/                     # SOPS-encrypted, only operators hold age keys
│   └── single-host.enc.yaml     # placeholder until `sops -e -i` produces real content
└── adr/
    ├── 0001-opentofu-not-terraform.md
    ├── 0002-nixos-flakes-for-substrate.md
    ├── 0003-k3s-on-single-host-v0.md
    └── 0004-sops-for-secrets.md
```

## Algebraic property

Modules compose freely:

- **`hosts/{role}.nix`** = "what runs on this machine"
- **`modules/{capability}.nix`** = a single capability (k3s, nats hub, openstory prep, sops)
- **`tofu/profiles/{topology}.tfvars`** = "where it runs and at what scale"

Future capabilities (Coder workspaces, Keycloak, Firecracker microVMs) and topologies (replicated, federated) drop in as new modules and new tfvars files without restructuring V0.

## How to use

### Dev shell (provides every tool)

```bash
cd docs/research/lab
nix develop
# → drops you into a shell with opentofu, kubectl, helm, sops, age, conftest, k3d, jq, curl
```

### Bring up the lab on Hetzner

```bash
# 1. Set Hetzner + Cloudflare credentials.
export HCLOUD_TOKEN=...
export CLOUDFLARE_API_TOKEN=...

# 2. Edit profiles/single-host.tfvars: real cloudflare_zone_id and operator_ssh_keys.
$EDITOR tofu/profiles/single-host.tfvars

# 3. Provision the box.
cd tofu
tofu init
tofu plan -var-file=profiles/single-host.tfvars
tofu apply -var-file=profiles/single-host.tfvars

# 4. Install NixOS over the bootstrap image.
nixos-anywhere --flake ..#lab-host root@$(tofu output -raw lab_ipv4)

# 5. Push secrets via sops (age public keys must already be in .sops.yaml).
sops -e -i ../secrets/single-host.enc.yaml

# 6. Activate.
nixos-rebuild switch --flake ..#lab-host --target-host root@$(tofu output -raw lab_ipv4)

# 7. Smoke test.
LAB_URL=https://lab.example.com LAB_TOKEN=... ../conformance/smoke.sh
```

### Run the integration test locally (no Hetzner needed)

```bash
# Requires Docker + the open-story:test image:
cd ../../  # back to repo root
cd rs && docker build -t open-story:test .
cd ../docs/research/lab
./conformance/spin_up_and_probe.sh
```

This spins up a k3d cluster, installs the Helm charts, posts a synthetic hook event, and probes `/api/sessions` and `/api/sessions/{id}/records`. Tears down on exit. Set `KEEP_CLUSTER=1` to leave the cluster up for inspection.

### Validate without deploying

```bash
# 1. Render the Helm charts and check OPA policies.
helm template openstory charts/openstory > /tmp/rendered.yaml
helm template nats charts/nats >> /tmp/rendered.yaml
conftest test /tmp/rendered.yaml --policy policy/

# 2. Run helm chart unit tests.
helm unittest charts/openstory
helm unittest charts/nats

# 3. Run OPA policy unit tests.
opa test policy/

# 4. tofu plan.
cd tofu && tofu plan -var-file=profiles/single-host.tfvars
```

## How we test it

Five layers, ordered cheap → expensive:

1. **`tofu plan` + Conftest** — every PR that touches `docs/research/lab/`. Cheapest test; catches "encrypted storage disabled," "secret committed in plaintext," "NATS exposed publicly." Runs in CI via `.github/workflows/lab-plan.yml`.
2. **Helm chart unit tests (`helm-unittest`)** — verifies chart structure (env vars from secretKeyRef not inline, NetworkPolicy denies external NATS, ConfigMap mirrors `rs/server/src/config.rs`).
3. **`spin_up_and_probe.sh`** — k3d → deploy → probe → teardown. Green CI = the deploy artifact actually works against the real binary in real Kubernetes. The "headline" integration test, modeled on `rs/tests/helpers/container.rs`.
4. **Existing `cargo test --workspace`** — ensures the binary the chart deploys is the binary CI proved correct. The Helm chart's image tag pins a specific Dockerfile.prod build.
5. **`smoke.sh`** — manual post-deploy probe against the real URL.

## Graduation criterion

This directory promotes to a top-level `lab/` when:

- It has driven at least one real Hetzner deploy (`tofu apply` + `nixos-anywhere`)
- The integration test (`spin_up_and_probe.sh`) has been green in CI for at least 2 weeks
- The deploy spec has accepted at least one substantive change without restructuring (e.g., a config field added in `rs/server/src/config.rs` flowing through to `values.yaml` cleanly)

Until then, the lab evolves alongside the personhood research in `docs/research/`. Premature promotion is a worse failure than late promotion.

## Related research

- [`../personhood-and-principals.md`](../personhood-and-principals.md) — the identity model that the lab will eventually extend from one Person to many
- [`../nats-permissions-spike.md`](../nats-permissions-spike.md) — the bus-level findings (single-account JetStream consumer leak) that constrain V2 hardening
- [`../../BACKLOG.md`](../../BACKLOG.md) — what comes next (sharing controls, anonymization, OIDC, Cloud IDE, microVM hardening, federated topology)
