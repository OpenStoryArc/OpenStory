{ config, lib, pkgs, ... }:

# Host-level prep for the NATS hub. The hub configuration is composed
# from the existing deploy/nats-hub.conf (so we don't duplicate truth);
# the Helm chart in ../charts/nats applies it to the cluster.

let
  cfg = config.services.lab-nats-hub;

  # We import the existing hub config as the single source of truth.
  # The Helm chart converts it into a ConfigMap.
  natsHubConf = builtins.path {
    name = "nats-hub.conf";
    path = ../../../../deploy/nats-hub.conf;
  };
in
{
  options.services.lab-nats-hub = {
    enable = lib.mkEnableOption "NATS hub host-level prep";

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/nats";
      description = "Host directory for NATS JetStream persistent storage.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0700 root root -"
    ];

    # Surface the existing hub config to the Helm chart's bootstrap unit.
    environment.etc."openstory-lab/nats-hub.conf".source = natsHubConf;
    environment.etc."openstory-lab/nats-data-dir".text = cfg.dataDir;
  };
}
