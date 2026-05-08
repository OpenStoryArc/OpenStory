{ config, lib, pkgs, ... }:

# SOPS integration via sops-nix. Each lab host has an age key at
# /var/lib/sops-nix/key.txt (provisioned by tofu via user_data, or
# manually for the first host). Encrypted secrets live in the repo at
# docs/research/lab/secrets/{profile}.enc.yaml; sops-nix decrypts them
# at activation time into /run/secrets/.
#
# Operators encrypt new secrets with:
#   sops -e -i docs/research/lab/secrets/single-host.enc.yaml
# using the age public keys in .sops.yaml at the repo root.

{
  sops = {
    # Default secrets file for this host. Override in hosts/lab-host.nix
    # with sops.defaultSopsFile = ../secrets/replicated.enc.yaml; to
    # switch profiles.
    defaultSopsFile = ../secrets/single-host.enc.yaml;
    defaultSopsFormat = "yaml";

    age = {
      # The age private key on the host. Provisioned via tofu user_data
      # or scp-ed by the operator on first boot.
      keyFile = "/var/lib/sops-nix/key.txt";
      generateKey = false;
    };
  };
}
