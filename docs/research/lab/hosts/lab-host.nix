{ config, pkgs, lib, ... }:

# Canonical V0 lab host: single Hetzner box, NixOS, runs k3s.
# Helm charts (openstory + nats) install onto k3s on first boot.
# Secrets are decrypted from sops-nix at activation time.

{
  imports = [
    ../modules/k3s.nix
    ../modules/sops.nix
    ../modules/openstory.nix
    ../modules/nats-hub.nix
  ];

  # ── system ────────────────────────────────────────────────────────────
  system.stateVersion = "24.11";

  networking = {
    hostName = "lab-host";
    firewall = {
      enable = true;
      # 80/443: public HTTPS via Caddy (or cert-manager via k3s LB).
      # 22:    SSH for ops (consider Tailscale-only).
      allowedTCPPorts = [ 22 80 443 ];
      # k8s, NATS internal traffic stays within the host (k3s loopback).
    };
  };

  # ── users ─────────────────────────────────────────────────────────────
  users.users.deploy = {
    isNormalUser = true;
    description = "OpenStory lab operator";
    extraGroups = [ "wheel" "docker" ];
    openssh.authorizedKeys.keys = [
      # Operator SSH keys are injected via tofu's user_data or sops.
      # Placeholder: replace before deploy.
      # "ssh-ed25519 AAAA... operator@lab"
    ];
  };

  security.sudo.wheelNeedsPassword = false;

  # ── services we always want ───────────────────────────────────────────
  services.openssh = {
    enable = true;
    settings = {
      PasswordAuthentication = false;
      PermitRootLogin = "no";
    };
  };

  # ── lab modules wired up ──────────────────────────────────────────────
  services.lab-k3s.enable = true;
  services.lab-openstory.enable = true;
  services.lab-nats-hub.enable = true;

  # ── secrets ───────────────────────────────────────────────────────────
  # Each entry below resolves to a file in /run/secrets/{name} after activation.
  # The Helm charts mount these by hostPath; the OpenStory pod reads them via env.
  sops.secrets."openstory/api_token".owner = "root";
  sops.secrets."openstory/db_key".owner = "root";
  sops.secrets."nats/leaf_token".owner = "root";

  # ── time ──────────────────────────────────────────────────────────────
  time.timeZone = "UTC";

  # ── pkgs in PATH on the host ──────────────────────────────────────────
  environment.systemPackages = with pkgs; [
    git
    htop
    jq
    kubectl
    kubernetes-helm
    sops
    age
    tmux
  ];
}
