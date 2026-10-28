# Installs nix-rebuild in the system profile, with the declaration from
# `module.nix`. Use this one when every user on the machine needs the tool.
{ config, lib, ... }:

{
  imports = [ ./module.nix ];

  config = lib.mkIf config.programs.nix-rebuild.enable {
    environment.systemPackages = [ config.programs.nix-rebuild.wrapped ];
  };
}
