[nix-rebuild](../README.md) / Install and options

# Install and options

The tool ships as its own flake and is consumed as a flake input.

```nix
# flake.nix
inputs.nix-rebuild.url = "github:you/nix-rebuild";
# or path:/home/you/Documents/Gits/nix-rebuild while you change it

# in the modules list of a homeManagerConfiguration:
programs.nix-rebuild = {
  enable = true;
  checkout = "/home/you/.config/nixos-dotfiles";
  system.config = "host";
  home = { config = "you"; manager = "standalone"; };
};
```

`checkout` has to be a path rather than a flake reference: `update` refreshes the
lock file from inside it.

## The two modules

`nix-rebuild.homeManagerModules.default` is the usual one. It installs the
package into `home.packages` and wraps it with that user's declaration.

`nix-rebuild.nixosModules.default` installs the same wrapped binary in
`environment.systemPackages`. Its wrapper carries one declaration for the whole
machine, so `home.*` names a single user. It is meant for a machine where every
user needs the tool; on a shared machine, install it from each user's Home
Manager instead.

The declaration reaches the binary as environment defaults set with
`--set-default`, so a value already in the caller's environment wins over the
module's.

## Options

| option | type | what it means |
|---|---|---|
| `enable` | bool, off | install the wrapped binary |
| `package` | package | what to wrap, defaulting to the package beside the module |
| `checkout` | string, unset | the flake a run rebuilds |
| `impure` | bool, off | pass `--impure` to every command that reads the checkout |
| `trustFlakeConfig` | bool, off | pass `--accept-flake-config` to every command that reads the checkout, so the `nixConfig` the flake sets (its substituters and keys) is not ignored; a nix without a terminal cannot answer Nix's own question about it |
| `system.config` | string, unset | the `nixosConfigurations` attribute the system scope activates |
| `home.config` | string, unset | the `homeConfigurations` attribute the home scope activates |
| `home.manager` | `standalone`, `module`, `none` | what `nix-rebuild home` can do here |

Nothing is guessed: an undeclared value stops the run with the name of the option
that declares it. `home.manager` has no default, because which kind this machine
runs is what the machine knows.

`standalone` activates a `homeConfigurations` output of its own. `module` means
the home profile is part of the NixOS configuration, so `nix-rebuild home` stops
and points at `nix-rebuild system`. `none` says the machine does not use Home
Manager, and an update activates the system alone.

## Flags

| flag | effect |
|---|---|
| `--flake PATH` | rebuild this checkout instead of the declared one |
| `--boot` | register the system generation for the next boot; refused with `home` |
| `-y`, `--yes` | run cleanup without asking; cannot be combined with `--no-clean` |
| `--no-clean` | skip generation pruning and garbage collection |
| `--accept-flake-config` | turn `trustFlakeConfig` on for this run, on `system`, `home`, `update` and `locks` |
| `runs <ID>` | open one recorded run; `last` is the newest |
| `runs --list` | print the recorded runs with their ids |
| `runs <ID> --logs` | print one run's log, for a pipe; with no id, the newest run |
| `runs <ID> --delete` | delete one recorded run after asking |
| `runs --delete-all` | delete every recorded run, after the same question |
| `locks <ID>` | put a saved lock back on the checkout and rebuild with it; `last` is the newest |
| `locks --list` | print the saved locks with their ids |
| `settings` | the Settings dialog alone; no run, nothing rebuilt |

## How the checkout is chosen

1. `--flake`, when it is given.
2. The machine's declaration: `FLAKE_PATH` in the wrapper's environment.
3. The `checkout` in the settings file.

The `sudo` re-exec passes `--flake` along, so the elevated frame resolves the
same tree as the parent.
