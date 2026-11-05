[nix-rebuild](../README.md) / Settings

# Settings

## The file

`$XDG_CONFIG_HOME/nix-rebuild/config.toml` is written with an annotated default
the first time a run needs it. Every line is optional: a missing one keeps its
default, and a command-line flag wins over the file.

| key | what it holds |
|---|---|
| `cleanup` | `ask`, `always` or `never`: pruning and garbage collection after a successful rebuild |
| `keep_generations` | how many generations the prune keeps |
| `prune_logs` | whether the oldest recorded runs are dropped when a run finishes |
| `keep_logs` | how many recorded runs stay |
| `keep_locks` | how many saved lock files stay |
| `checkout` | the flake a run rebuilds when neither `--flake` nor the wrapper declares one |
| `leave` | `clean` or `frame`: what the shell is left with when the frame closes |
| `show_status`, `show_activity`, `show_logs` | which cards a run starts with |
| `show_downloads`, `show_builds`, `show_activity_history`, `show_cleanup_progress` | what the Activity card lists |
| `raw_logs` | whether the Logs card starts with raw lines |
| `log_filter_query`, `log_filter_kinds` | the Logs filter a start begins with |

## The dialog

`G` opens the Settings dialog over the run, and `Esc` or `G` closes it.
`nix-rebuild settings` opens the same dialog in a frame of its own, with no
rebuild: closing the dialog closes the frame, because the box was all there was
to look at. Without a terminal, `settings` prints the file instead.

The arrows, the wheel or the pointer move through the rows (hover picks a row, a
click changes it); `Enter` or `←`/`→` changes the selected row, and a change is
written back at once. The rows are grouped in sections (Panels, Activity, Runs,
Locks, Cleanup, Terminal). The list scrolls inside the box when the terminal is too
short for all of it, and its bar can be dragged like a card's.

A toggle reads green when it is on and red when it is off. A number is drawn with
`-` and `+` at the end of the row: click them, or use `←`/`→`.

The Runs section carries the recorded runs' own figures, how many runs the journal
holds and how many bytes they take, the pruning it does on finish, and a button
that cuts it: "Remove all previous runs" asks first, naming how many recorded runs
it would delete, and keeps the run being recorded now. The "Cleanup now" row opens
the same cleanup question a finished rebuild asks, and nothing runs until it is
answered.

The rows are the file's defaults, what the next start reads, rather than this
frame's state. The frame's own keys (`S`, `A`, `L`, `Ctrl+R`, `V`) are the
opposite: they change only what this frame shows, and nothing about them is
written to the file.
