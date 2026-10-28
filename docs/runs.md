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

`nix-rebuild runs` opens the picker: one row per run with its date, id, scope,
result, duration and checkout. The arrows move, `Enter` opens the run in the
frame, `/` searches the columns, and `Esc` leaves.

`runs <ID>` opens that run directly, and `last` names the newest one.
`runs --list` prints the same table to stdout and `runs <ID> --logs` prints one
run's log, which is what a pipe or a bug report wants.

A run is recorded only through the frame, so a switch started from a script, a
systemd unit or CI leaves nothing behind.

## Without a terminal

The steps run plainly: cleanup happens with `--yes`, and an `ask` policy prints
why it was skipped. The commands that would draw a card say so instead: `runs`
asks for `--list` or an id, `runs --list` prints the table, `runs <ID> --logs`
prints the log and `settings` prints the file.
