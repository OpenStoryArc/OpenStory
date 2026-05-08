{ config, lib, pkgs, ... }:

# Host-level prep for the OpenStory server. The actual workload deploys
# via the Helm chart in ../charts/openstory; this module just handles
# what needs to be in place on the NixOS host before the chart starts:
#   - persistent data directory (/var/lib/openstory) with correct perms
#   - hostPath mount points the Helm chart references
#   - port 3002 reachable from the cluster (k3s on the same node, so
#     loopback is fine; ingress goes through Caddy/nginx on 443)

let
  cfg = config.services.lab-openstory;
in
{
  options.services.lab-openstory = {
    enable = lib.mkEnableOption "OpenStory host-level prep";

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/openstory";
      description = ''
        Host directory mounted into the OpenStory pod at /data.
        Persists SQLite DB, JSONL backups, and plans.
      '';
    };

    user = lib.mkOption {
      type = lib.types.int;
      default = 1000;
      description = "UID inside the OpenStory container; must own dataDir.";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.tmpfiles.rules = [
      "d ${cfg.dataDir} 0750 ${toString cfg.user} ${toString cfg.user} -"
    ];

    # The Helm chart's values.yaml references this path via hostPath.
    # We expose it through environment.etc so the chart can reference
    # a stable location for hostPath mounts.
    environment.etc."openstory-lab/data-dir".text = cfg.dataDir;
  };
}
