[nix-rebuild](../README.md) / Recorded runs

# Recorded runs

## Where they live

A run is recorded under `$XDG_STATE_HOME/nix-rebuild/runs/<id>`, one directory per
run with its `meta.conf` and its `run.log`. A run recorded before the directory
was renamed sits under `sessions/<id>` and is still read from there.

`meta.conf` carries the scope, the flake, the action, the cleanup policy, the
start time and the exit code once the run ends. The id is a timestamp and a short
suffix, in the order the runs were started.

## The log

The frame records the run as it happens. When the run finishes and its frame
closes, the log is gzipped, roughly 20 to 30 times smaller, and the plain file is
removed. A run killed mid-flight keeps its plain log, and both forms are readable,
so an interrupted run is never lost.

## Pruning

The recorder keeps the newest 20 runs under a 256 MiB ceiling, pruned when a run
finishes. Both limits are the settings file's (`prune_logs`, `keep_logs`), and the
Settings dialog edits them.

`runs <ID> --delete` and `runs --delete-all` remove runs on demand. Both ask
first, naming how many runs would go; `--yes` skips the question, and without a
terminal it is required.

## Reading them

`nix-rebuild runs` opens the picker: one row per run with its date, id, action,
scope, lock, result, duration and checkout. The action is the verb that started the
run (`switch`, `boot`, `update` or `rollback`), so an update is not read as the
`switch` it ends with; the scope is the step the run ended on. The arrows move,
`Enter` opens the run in the frame, `/` searches the columns, and `Esc` leaves.

The lock column is the digest of the `flake.lock` the run built with, read when the
run finished: an update has already moved the file by then, so the value is of the
inputs the build used, not the ones it replaced. Two runs with the same inputs show
the same eight characters, which is how a `home` or `system` switch can be matched to
the update that pinned it; `locks --list` carries the same value for a kept lock, and
`runs <ID> --logs` prints the whole digest. It is an FNV-1a digest of the file, not a
checksum to compare with `sha256`. A flake reference that is not a local path has no
file to read, so its column stays empty.

`runs <ID>` opens that run directly, and `last` names the newest one.
`runs --list` prints the same table to stdout and `runs <ID> --logs` prints one
run's log, which is what a pipe or a bug report wants.

## Putting back what a run replaced

An `update` records the lock file it kept, and a `rollback` records the one it
replaced, so a rollback is itself reversible. On a row that carries one, the title
shows `[R  Revert]` beside the other keys (on a terminal too narrow for five chips
the hint under the list says it instead, and it is a click too). `R` asks first,
because the checkout's `flake.lock` is overwritten; the question is the same
`Rollback` box `nix-rebuild locks` draws. `[Y  Revert and rebuild]` (`y`) writes the
kept lock back and rebuilds both configurations; `[N  Keep the lock]` (`n` or `Esc`)
keeps the current one. What runs is what `nix-rebuild locks <ID>` runs: one
`nix build` and the activations, with no `nix flake update`.

The `Now` card above the list carries the digest of the `flake.lock` the checkout
holds right now and the run that built with it; that row carries a `•` in place of the
mark. A digest no recorded run has seen leaves every row unmarked.

`R` is pressed on the run you want to undo, which is the one you just ran: `R` on the
third of three updates lands on the inputs of the second.

A rollback keeps the lock it replaces, so its own row carries one and `R` on it goes
forward again, back to the inputs the rollback overwrote. A run whose lock has been
pruned says so instead of rebuilding, and only `nix-rebuild update` runs
`nix flake update`.

A run is recorded only through the frame, so a switch started from a script, a
systemd unit or CI leaves nothing behind.

## Without a terminal

The steps run plainly: cleanup happens with `--yes`, and an `ask` policy prints
why it was skipped. The commands that would draw a card say so instead: `runs`
asks for `--list` or an id, `runs --list` prints the table, `runs <ID> --logs`
prints the log and `settings` prints the file.
