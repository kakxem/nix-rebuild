{
  description = "nix-rebuild: the interactive NixOS and Home Manager rebuild interface";

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      forAllSystems = function: nixpkgs.lib.genAttrs systems function;

      perSystem =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          packages = {
            default = pkgs.callPackage ./default.nix { };
            nix-rebuild = pkgs.callPackage ./default.nix { };
          };

          checks =
            let
              package = pkgs.callPackage ./default.nix { };
            in
            {
              # Building the package runs `cargo test` in its check phase.
              inherit package;

              formatting =
                pkgs.runCommand "nix-rebuild-formatting"
                  {
                    nativeBuildInputs = [
                      pkgs.cargo
                      pkgs.rustfmt
                      pkgs.nixfmt
                    ];
                    src = pkgs.lib.cleanSource ./.;
                  }
                  ''
                    cp -r $src/. .
                    chmod -R u+w .
                    cd rust
                    cargo fmt --check
                    cd ..
                    nixfmt --check flake.nix default.nix module.nix home.nix nixos.nix
                    touch $out
                  '';

              # Both installers must evaluate with a declaration and hand over the wrapper.
              module =
                let
                  lib = nixpkgs.lib;
                  declaration = {
                    programs.nix-rebuild = {
                      enable = true;
                      checkout = "/var/empty/nixos-dotfiles";
                      system.config = "host";
                      home = {
                        config = "user";
                        manager = "standalone";
                      };
                    };
                  };
                  nixos = lib.nixosSystem {
                    inherit system;
                    modules = [
                      self.nixosModules.default
                      declaration
                      { system.stateVersion = "25.11"; }
                    ];
                  };
                  home = lib.evalModules {
                    specialArgs = { inherit pkgs; };
                    modules = [
                      {
                        options.home.packages = lib.mkOption {
                          type = lib.types.listOf lib.types.package;
                          default = [ ];
                        };
                      }
                      self.homeManagerModules.default
                      declaration
                    ];
                  };
                in
                pkgs.runCommand "nix-rebuild-module" { } ''
                  test -n "${nixos.config.programs.nix-rebuild.wrapped.drvPath}"
                  test -n "${home.config.programs.nix-rebuild.wrapped.drvPath}"
                  touch $out
                '';
            };

          devShells.default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.nixfmt
            ];
          };

          formatter = pkgs.nixfmt;
        };
    in
    {
      packages = forAllSystems (system: (perSystem system).packages);

      checks = forAllSystems (system: (perSystem system).checks);

      devShells = forAllSystems (system: (perSystem system).devShells);

      formatter = forAllSystems (system: (perSystem system).formatter);

      overlays.default = final: _previous: {
        nix-rebuild = final.callPackage ./default.nix { };
      };

      # Each installer is exposed under both `default` and `nix-rebuild`.
      homeManagerModules.default = import ./home.nix;
      homeManagerModules.nix-rebuild = import ./home.nix;
      nixosModules.default = import ./nixos.nix;
      nixosModules.nix-rebuild = import ./nixos.nix;
    };
}
