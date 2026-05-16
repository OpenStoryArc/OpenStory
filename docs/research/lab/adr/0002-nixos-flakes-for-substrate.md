# ADR 0002 — NixOS flakes as the host substrate

**Status:** Accepted
**Date:** 2026-05-07

## Context

The lab host runs OpenStory's binary, NATS, and k3s. We need a way to declare "what the host is" — packages, services, users, secrets, kernel — that survives reboots, can be reproduced from source, and stays auditable across operators.

The realistic options:
- **Ubuntu/Debian + Ansible**: familiar, large operator pool, mutable hosts (drift over time).
- **NixOS + flakes**: every byte of the running system is provably built from a public flake. Sovereignty becomes a property of the binary, not a claim.
- **Container-only (no host substrate)**: only k8s primitives. Minimal host = harder operator ergonomics for SSH-level work; brittle when something needs to live outside Kubernetes.

OpenStory's soul is sovereignty + minimal honest code. The principles include "open standards, user-owned data" and "minimal, honest code." A host whose entire definition lives in a 200-line flake — readable, diffable, reproducible — fits the soul more cleanly than a stateful Ubuntu box maintained by Ansible drift.

## Decision

Use **NixOS with flakes** as the substrate. The lab host is defined by `flake.nix` + `hosts/lab-host.nix` + the modules under `modules/`. `nixos-anywhere` installs the system over a vanilla bootstrap image; `nixos-rebuild switch --target-host` applies updates declaratively.

## Consequences

- **Sovereignty story is operational, not aspirational.** "What runs on the lab host?" is answered by reading the flake. There is no drift between docs and reality.
- **Reproducible.** Anyone who clones the repo and runs `nix build .#lab-host` gets the exact system image we deploy.
- **Steeper learning curve.** Contributors unfamiliar with Nix face a real ramp. Documented in `README.md` and ADR 0003.
- **Smaller operator pool.** We trade community size for sovereignty alignment. Acceptable in V0 because the lab is invite-only.
- **k3s and Helm carry the workload layer.** Nix declares the host; Kubernetes declares the apps. Right rung of declarativeness for each layer.
- **Secret handling pairs with sops-nix** (see ADR 0004) — secrets stay encrypted at rest in the repo, decrypted at activation.

## Alternatives considered

- **Ubuntu + Ansible** — familiar, mature, but mutable. Rejected on sovereignty grounds.
- **NixOS without flakes** — works, but flakes are the modern idiom and pin everything (nixpkgs commit, sops-nix commit, etc.) explicitly. We use flakes.
- **Talos Linux** — minimal Kubernetes-only OS. Nice posture but constrains us to k8s primitives at the host level. We want some host-level work (firewall, sops decryption, persistent volumes outside k8s) so a fuller OS substrate fits better.
