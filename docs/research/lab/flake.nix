{
  description = "OpenStory lab — declarative deployable artifact (V0)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-24.11";

    sops-nix = {
      url = "github:Mic92/sops-nix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, sops-nix, ... }:
    let
      system = "x86_64-linux";
    in
    {
      # The canonical lab host — single-node, all-in-one.
      # Provisioned by tofu/main.tf onto a Hetzner box.
      nixosConfigurations.lab-host = nixpkgs.lib.nixosSystem {
        inherit system;
        modules = [
          sops-nix.nixosModules.sops
          ./hosts/lab-host.nix
        ];
      };

      # Convenience: build the host config without deploying.
      #   nix build .#lab-host
      packages.${system}.lab-host =
        self.nixosConfigurations.lab-host.config.system.build.toplevel;

      # Dev shell with the tools needed to operate the lab.
      #   nix develop
      devShells.${system}.default = nixpkgs.legacyPackages.${system}.mkShell {
        buildInputs = with nixpkgs.legacyPackages.${system}; [
          opentofu
          kubectl
          kubernetes-helm
          sops
          age
          conftest
          k3d # for spin_up_and_probe.sh local runs
          jq
          curl
        ];
      };
    };
}
