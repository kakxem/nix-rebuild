# Installs nix-rebuild in the home profile, with the declaration from `module.nix`.
{ config, lib, ... }:

{
  imports = [ ./module.nix ];

  config = lib.mkIf config.programs.nix-rebuild.enable {
    home.packages = [ config.programs.nix-rebuild.wrapped ];
  };
}
