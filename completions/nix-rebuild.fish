complete -c nix-rebuild -f

complete -c nix-rebuild \
    -n 'not __fish_seen_subcommand_from system home update runs resume settings' \
    -a system -d 'Rebuild and activate NixOS'
complete -c nix-rebuild \
    -n 'not __fish_seen_subcommand_from system home update runs resume settings' \
    -a home -d 'Rebuild and activate Home Manager'
complete -c nix-rebuild \
    -n 'not __fish_seen_subcommand_from system home update runs resume settings' \
    -a update -d 'Refresh the lock file, then activate Home Manager and NixOS'
complete -c nix-rebuild \
    -n 'not __fish_seen_subcommand_from system home update runs resume settings' \
    -a runs -d 'Open a recorded run: choose one from the list, or name it'
complete -c nix-rebuild \
    -n 'not __fish_seen_subcommand_from system home update runs resume settings' \
    -a settings -d 'Edit the remembered settings without rebuilding'

complete -c nix-rebuild -l boot \
    -n '__fish_seen_subcommand_from system' \
    -d 'Register the system for next boot without switching live'
complete -c nix-rebuild -s y -l yes \
    -d 'Run cleanup without asking'
complete -c nix-rebuild -l no-clean \
    -d 'Skip generation pruning and garbage collection'
complete -c nix-rebuild -l flake -r \
    -d 'Rebuild this checkout instead of the configured one'

# The id and the two ways of reading a run belong to `runs`.
complete -c nix-rebuild -l list \
    -n '__fish_seen_subcommand_from runs resume' \
    -d 'Print the recorded runs, with their ids'
complete -c nix-rebuild -l logs \
    -n '__fish_seen_subcommand_from runs resume' \
    -d 'Print a recorded run to the log instead of drawing it'
complete -c nix-rebuild -l delete \
    -n '__fish_seen_subcommand_from runs resume' \
    -d 'Delete the recorded run named by the id'
complete -c nix-rebuild -l delete-all \
    -n '__fish_seen_subcommand_from runs resume' \
    -d 'Delete every recorded run'
complete -c nix-rebuild \
    -n '__fish_seen_subcommand_from runs resume' \
    -f -a 'last' -d 'The newest recorded run'
complete -c nix-rebuild \
    -n '__fish_seen_subcommand_from runs resume' \
    -f -a '(set -l state $XDG_STATE_HOME; test -n "$state"; or set state ~/.local/state; command ls $state/nix-rebuild/sessions 2>/dev/null | sort -r)'

complete -c nix-rebuild -s h -l help \
    -d 'Show help'
complete -c nix-rebuild -l version \
    -d 'Print version'
