{ config, lib, pkgs, ... }:

# k3s on a single node. We disable traefik because we want our own
# ingress story (cert-manager + nginx, or Caddy on the host). We disable
# servicelb because the lab is single-host; pods bind to the host network
# via NodePort or hostPort.

let
  cfg = config.services.lab-k3s;
in
{
  options.services.lab-k3s = {
    enable = lib.mkEnableOption "lab k3s single-node";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.k3s;
      description = "k3s package to install.";
    };

    dataDir = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/rancher/k3s";
      description = "Where k3s persists state.";
    };
  };

  config = lib.mkIf cfg.enable {
    services.k3s = {
      enable = true;
      package = cfg.package;
      role = "server";
      extraFlags = toString [
        "--disable=traefik"
        "--disable=servicelb"
        "--write-kubeconfig-mode=0644"
        "--data-dir=${cfg.dataDir}"
      ];
    };

    # Helm charts install via a one-shot systemd unit on first boot.
    # The unit reads from /etc/openstory-lab/charts (mounted from the
    # repo's docs/research/lab/charts during deploy) and applies them.
    systemd.services.lab-helm-bootstrap = {
      description = "Apply OpenStory + NATS Helm charts on first boot";
      after = [ "k3s.service" ];
      wants = [ "k3s.service" ];
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      path = [ pkgs.kubernetes-helm pkgs.kubectl pkgs.bash ];
      script = ''
        set -euo pipefail
        export KUBECONFIG=${cfg.dataDir}/kubeconfig.yaml
        # Wait for k3s API to come up.
        for i in $(seq 1 30); do
          if kubectl get nodes >/dev/null 2>&1; then break; fi
          sleep 2
        done
        # Apply charts. Idempotent — helm upgrade --install.
        if [ -d /etc/openstory-lab/charts/openstory ]; then
          helm upgrade --install openstory /etc/openstory-lab/charts/openstory \
            --namespace openstory --create-namespace \
            --values /etc/openstory-lab/charts/openstory/values.yaml \
            --wait --timeout 5m
        fi
        if [ -d /etc/openstory-lab/charts/nats ]; then
          helm upgrade --install nats /etc/openstory-lab/charts/nats \
            --namespace openstory \
            --values /etc/openstory-lab/charts/nats/values.yaml \
            --wait --timeout 5m
        fi
      '';
    };
  };
}
