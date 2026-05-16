terraform {
  required_version = ">= 1.7.0"

  required_providers {
    hcloud = {
      source  = "hetznercloud/hcloud"
      version = "~> 1.48"
    }
    cloudflare = {
      source  = "cloudflare/cloudflare"
      version = "~> 4.40"
    }
  }
}

# ── Hetzner provider ────────────────────────────────────────────────────
# HCLOUD_TOKEN env var carries the API token; never store it in a tfvars file.
provider "hcloud" {}

# ── Cloudflare provider ────────────────────────────────────────────────
# CLOUDFLARE_API_TOKEN env var.
provider "cloudflare" {}

# ── SSH keys allowed to reach the host ─────────────────────────────────
# Public keys live in tfvars (they're not secrets); the operator's age
# key for SOPS is provisioned out-of-band on first boot.
resource "hcloud_ssh_key" "operators" {
  for_each   = { for k in var.operator_ssh_keys : k.name => k }
  name       = each.value.name
  public_key = each.value.public_key
}

# ── The lab host ───────────────────────────────────────────────────────
resource "hcloud_server" "lab" {
  name        = var.host_name
  server_type = var.server_type
  location    = var.location
  image       = var.bootstrap_image # e.g. "ubuntu-24.04"; we install NixOS via nixos-anywhere post-create

  ssh_keys = [for k in hcloud_ssh_key.operators : k.id]

  labels = {
    project = "openstory"
    role    = "lab"
    profile = var.profile_name
  }

  # Lifecycle protection — ignore image changes (we install NixOS over it).
  lifecycle {
    ignore_changes = [image, ssh_keys]
  }
}

# ── DNS ────────────────────────────────────────────────────────────────
resource "cloudflare_record" "lab_a" {
  zone_id = var.cloudflare_zone_id
  name    = var.host_name
  type    = "A"
  value   = hcloud_server.lab.ipv4_address
  ttl     = 300
  proxied = false
}

# ── Bootstrap notes ────────────────────────────────────────────────────
# After `tofu apply`, finish bootstrap with:
#
#   nixos-anywhere --flake ../#lab-host root@$(tofu output -raw lab_ipv4)
#
# (Run from docs/research/lab/. The flake.nix output `lab-host` defines
# the NixOS system; nixos-anywhere installs it over the bootstrap image.)
#
# Subsequent updates:
#   nixos-rebuild switch --flake ../#lab-host --target-host root@$(tofu output -raw lab_ipv4)
