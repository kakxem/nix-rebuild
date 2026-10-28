# Interactive rebuild interface built with Rust and Ratatui: the binary owns
# the whole run, from Nix event decoding to activation and cleanup.
{
  lib,
  rustPlatform,
  makeWrapper,
  nixos-rebuild,
  home-manager,
  nix,
  coreutils-full,
}:

rustPlatform.buildRustPackage {
  pname = "nix-rebuild";
  version = (lib.importTOML ./rust/Cargo.toml).package.version;

  src = lib.cleanSource ./.;

  # The crate sits in `rust/`; the completion its tests read stays at the root.
  cargoRoot = "rust";
  buildAndTestSubdir = "rust";

  cargoLock.lockFile = ./rust/Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  postInstall = ''
    mkdir -p $out/share/fish/vendor_completions.d
    install -m 0644 ${./completions/nix-rebuild.fish} \
      $out/share/fish/vendor_completions.d/nix-rebuild.fish

    wrapProgram $out/bin/nix-rebuild \
      --prefix PATH : ${
        lib.makeBinPath [
          nixos-rebuild
          home-manager
          nix
          coreutils-full
        ]
      } \
      --set NIXOS_REBUILD_PATH ${nixos-rebuild}/bin/nixos-rebuild \
      --set HOME_MANAGER_PATH ${home-manager}/bin/home-manager \
      --set NIX_ENV_PATH ${nix}/bin/nix-env \
      --set NIX_COLLECT_GARBAGE_PATH ${nix}/bin/nix-collect-garbage

    test -x $out/bin/nix-rebuild
    test -f $out/share/fish/vendor_completions.d/nix-rebuild.fish
  '';

  meta = {
    description = "Interactive NixOS and Home Manager rebuild interface";
    license = lib.licenses.mit;
    platforms = lib.platforms.linux;
    mainProgram = "nix-rebuild";
  };
}
