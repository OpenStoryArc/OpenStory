# ADR 0004 — SOPS + age for secrets in the repo

**Status:** Accepted
**Date:** 2026-05-07

## Context

The lab needs secrets (`api_token`, `db_key`, NATS leaf token, Cloudflare API key, operator SSH keys). These cannot live in plaintext in the repo, but they also cannot live "somewhere else" in a way that breaks the deployable-artifact discipline — the repo IS the lab spec. If half the spec lives outside the repo, the lab is no longer reproducible from a clone.

The realistic options:
- **HashiCorp Vault** — full secret management, but adds a stateful service and a chicken-and-egg ("how does the host get the Vault unseal key?") problem.
- **Cloud KMS (AWS, GCP, Hetzner)** — vendor lock-in, contradicts sovereignty.
- **SOPS + age** — encrypted secrets in the repo; only specific people hold age keys. Decrypted at NixOS activation via `sops-nix`.

OpenStory's principles emphasize portable, user-owned data. SOPS+age fits: the encrypted file lives next to the code that needs it, in the same git history, reviewable in PRs (encrypted blobs are diffable line-by-line because SOPS does per-value encryption).

## Decision

Use **SOPS** with **age** keys. Encrypted secrets live at `docs/research/lab/secrets/{profile}.enc.yaml`. Age public keys are listed in a top-level `.sops.yaml` (one per operator). The host's age private key is provisioned out-of-band on first boot (via tofu user_data, or via scp by the bootstrap operator) and read by sops-nix from `/var/lib/sops-nix/key.txt`.

## Consequences

- **The lab repo IS the spec, secrets included.** A clone + the right age key reproduces the running lab.
- **Every secret rotation is a PR.** Encrypted before commit, decrypted at deploy. The history of secret changes is auditable.
- **Each operator's age public key is in `.sops.yaml`.** Adding/removing an operator is a PR; key rotation is `sops updatekeys`.
- **No external secret manager dependency.** No Vault to operate, no cloud KMS API to call.
- **First-boot bootstrap** is the one place the age key has to land manually. Documented in `README.md`. After that, every secret update is `sops -e -i` + `nixos-rebuild switch`.
- **NixOS activation reads the secrets**, never the workload pods directly. The chart's Secret resources are populated by sops-nix into hostPath, then mounted by Kubernetes as a Secret. Plaintext never touches Kubernetes etcd.

## Alternatives considered

- **HashiCorp Vault** — too much state for V0. Operationally heavy; defer until secret-volume justifies it.
- **age files committed plaintext via .gitignore** — reproducibility lost.
- **External secrets via env-only deploy (no secrets in repo)** — fails the "deployable artifact" property; the spec becomes "this repo + a long human readme of values you have to find elsewhere."
- **GitHub Actions secrets only** — works for CI but not for human-driven `tofu apply`.
