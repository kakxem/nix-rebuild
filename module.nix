# What this machine declares to nix-rebuild: each command-line default is an
# option here. Declares only; `home.nix` and `nixos.nix` install the binary.
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.nix-rebuild;

  # null drops the variable; an empty string would still be carried.
  declared = lib.filterAttrs (_: value: value != null) {
    FLAKE_PATH = cfg.checkout;
    NIX_REBUILD_SYSTEM_CONFIG = cfg.system.config;
    NIX_REBUILD_HOME_CONFIG = cfg.home.config;
    NIX_REBUILD_HOME_MANAGER = cfg.home.manager;
    # Off by default; pure evaluation is what Nix does anyway.
    NIX_REBUILD_IMPURE = if cfg.impure then "1" else null;
  };

  # --set-default, not --set: the caller's own environment wins over this.
  defaults = lib.concatStringsSep " " (
    lib.mapAttrsToList (name: value: "--set-default ${name} ${lib.escapeShellArg value}") declared
  );
in
{
  options.programs.nix-rebuild = {
    enable = lib.mkEnableOption "nix-rebuild, the interactive rebuild interface";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./default.nix { };
      defaultText = lib.literalExpression "pkgs.callPackage ./default.nix { }";
      description = ''
        The nix-rebuild package to install, before this machine's declaration is
        wrapped around it. The default builds the one beside this module.
      '';
    };

    checkout = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/home/you/.config/nixos-dotfiles";
      description = ''
        The flake checkout a run rebuilds when no `--flake` is given. It has to
        be a path rather than a flake reference, because an update refreshes the
        lock file from inside it. Left out, the tool refuses to start instead of
        guessing a directory.
      '';
    };

    impure = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Whether the flake is evaluated impurely: `--impure` on every command
        that reads the checkout. A flake that reaches outside its own tree, at a
        path in the environment or under a home directory, is refused by a pure
        evaluation, and only this flag lets it through.

        The default is what Nix already does, so a machine that does not need it
        passes no flag at all.
      '';
    };

    system = {
      config = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "myhost";
        description = ''
          The `nixosConfigurations` attribute the system scope activates. Left
          out, a run that needs it stops and says so: the hostname and the
          `system` convention are guesses, and this tool does not guess.
        '';
      };
    };

    home = {
      config = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "you";
        description = ''
          The `homeConfigurations` attribute the home scope activates. Left out,
          a run that needs it stops and says so, rather than assuming the name
          of the user running the tool. A system-wide install bakes one name
          into the machine, so a shared machine installs the tool from each
          user's Home Manager instead.
        '';
      };

      manager = lib.mkOption {
        type = lib.types.enum [
          "standalone"
          "module"
          "none"
        ];
        description = ''
          How Home Manager is installed here, which is what `nix-rebuild home`
          can do. `standalone` activates a `homeConfigurations` output of its
          own, `module` is a profile inside the NixOS configuration that only
          the system activation reaches, and `none` means the machine does not
          use it. With the last two, `home` says so rather than failing at Home
          Manager, and an update activates the system configuration alone.

          This one has no default: which kind this machine runs is what the
          machine knows, not something to assume.
        '';
      };
    };

    wrapped = lib.mkOption {
      type = lib.types.package;
      readOnly = true;
      internal = true;
      description = "The package to install: `package` with this machine's declaration in its environment.";
      default = pkgs.symlinkJoin {
        name = "nix-rebuild";
        paths = [ cfg.package ];
        nativeBuildInputs = [ pkgs.makeWrapper ];
        postBuild = lib.optionalString (defaults != "") ''
          wrapProgram $out/bin/nix-rebuild ${defaults}
        '';
      };
    };
  };
}
