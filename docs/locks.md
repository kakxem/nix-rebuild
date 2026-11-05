[nix-rebuild](../README.md) / Saved locks

# Saved locks

## What keeps one

`nix-rebuild update` copies the checkout's `flake.lock` before `nix flake update`
runs, so the inputs an update is about to replace survive it. A rollback keeps the
lock it is about to replace in the same way, which makes a rollback reversible.

They live under `$XDG_STATE_HOME/nix-rebuild/locks/<id>`, one directory per lock
holding the `flake.lock` itself and a `meta.conf` that names the checkout, the
reason (`update` or `rollback`) and when it was kept. The id has the same timestamp
shape the recorded runs use.

## Putting one back

`nix-rebuild locks` opens the selector: one row per kept lock with its date, id,
reason, lock and checkout. The lock column is the digest of the kept file, the same
value `runs --list` shows for the run that built with those inputs, so a lock and a
rebuild can be matched without opening either.

A card above the table carries the digest of the `flake.lock` the checkout holds
right now, read from the file itself. The row holding that digest carries a `•` in
place of the `❯`, so the state you are in is a row of the table; when no kept lock has
it, the card says so.

The arrows move and `Esc` leaves. `Enter`, or a click on a row, asks first, because
this writes over the checkout's lock and rebuilds: the `Rollback` box `nix-rebuild
runs` draws too, naming the digest leaving and the one arriving
(`9382a635 → 64c10c1d`), and saying so when the chosen lock is already what the
checkout holds. `[N  Keep the lock]` (`n`, `Esc`, or the corner) leaves without
writing anything; `[Y  Revert and rebuild]` (`y` or `Enter`) goes ahead.

`nix-rebuild locks <ID>` does the same without the selector, and `last` names the
newest one; `locks --list` prints the table. The rebuild is the second half of an
update: one `nix build` and the activations, with no `nix flake update`, because the
lock is already pinned. `--boot` registers the system generation for the next boot
instead of switching, `--no-clean` skips the cleanup question, and `--flake` picks
the checkout.

A lock belongs to the flake it pins, so restoring one kept for another checkout is
refused and names the checkout it came from. `R` in `nix-rebuild runs` reads the
checkout off the lock's own `meta.conf`, so it needs no `--flake`.

## Pruning

The newest 20 are kept. The Settings dialog's `Locks kept` row, or `keep_locks` in
`config.toml`, moves that count, and the oldest go when a new one is kept. A lock is
a few kilobytes, so there is no byte ceiling beside the count.

## Without a terminal

`locks` without an id asks for `--list` or an id. `locks --list` prints the table
and needs neither a checkout nor a declaration; `locks <ID>` restores the lock and
runs the steps plainly, exactly as `update` does under a redirect.
