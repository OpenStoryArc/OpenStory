# ADR 0001 — OpenTofu, not Terraform

**Status:** Accepted
**Date:** 2026-05-07

## Context

The lab's deployable artifact uses an HCL-shaped tool to provision Hetzner hosts and Cloudflare DNS. The choice is between Terraform and OpenTofu (the MPL-2.0 fork created after HashiCorp's 2023 license change to BSL).

OpenStory's soul is personal sovereignty. The principles include "open standards, user-owned data" — an HCL tool that ships with a vendor-restrictive license sits awkwardly with that. Both tools share the same configuration language; the choice is mostly about license posture, governance, and ecosystem direction.

## Decision

Use **OpenTofu**. It is MPL-2.0, governed by the Linux Foundation, and shipping the same provider ecosystem (the Hetzner and Cloudflare providers run identically against both). The lab's IaC files are `.tf` and `.tfvars` — recognizable to any Terraform user — so the cognitive cost is zero.

## Consequences

- The lab is reproducible by anyone who installs OpenTofu (free, MPL). No registration, no commercial license check.
- Provider ecosystem parity means we can pull `hetznercloud/hcloud` and `cloudflare/cloudflare` without modification.
- If OpenTofu and Terraform diverge significantly, we may need a future migration. Until then, files written for one work for the other.
- The dev shell in `flake.nix` pins `opentofu` from nixpkgs. Operators don't need to install anything separately.

## Alternatives considered

- **Terraform** — same language, same providers, vendor-restrictive license. Rejected on sovereignty grounds.
- **Pulumi (TypeScript/Go)** — code-as-infrastructure rather than declarative HCL. Stronger typing, larger surface area, less tooling parity with the existing community ecosystem. Defer until a multi-language need emerges.
- **Crossplane** — Kubernetes-native infra. Powerful, but circular at V0: we'd need a cluster to provision the cluster.
