[nix-rebuild](../README.md) / Using it

# Using it

## The commands

| command | what it runs |
|---|---|
| `system` | `nixos-rebuild switch --flake <checkout>#<system.config>`, or `boot` with `--boot` |
| `home` | `home-manager switch --flake <checkout>#<home.config> -b backup-<stamp>-<pid>` |
| `update` | `nix flake update`, one `nix build`, then the activations of the declared scopes |
| `locks [<ID>]` | a saved lock file: put it back on the checkout and rebuild with it, or list them |
| `runs [<ID>]` | a recorded run: open it, list them, print one log, or delete one |
| `settings` | the Settings dialog alone, with no rebuild |

`resume` parses as an alias of `runs`, so old notes keep working without the help
teaching two names.

`system` runs the frame elevated and `home` refuses to run as root. `update`
elevates only the system activation, so the frame stays the user's, and `locks`
rebuilds the same way.

## Keys

| key | what it does |
|---|---|
| `S`, `A`, `L` | show and hide the Status, Activity and Logs cards |
| `G` | open the Settings dialog; `Esc` or `G` closes it |
| `H` | the full list; `/` searches it and `Enter` runs the selected row |
| `D` | the details of this run, and the id it is recorded under |
| `F` | filter the logs, and save the filter as a default |
| `T` | filter the activity by kind |
| `V` | swap the Activity card between Nix's work and the cleanup steps |
| `Ctrl+R` | raw log lines instead of the parsed ones |
| `C` | copy every log line |
| `↑` `↓` | scroll the logs a line; `PgUp` `PgDn` a page; `End` returns to the newest |
| `Esc` | ask to cancel a running rebuild, or close a finished one |
| `Ctrl+C` | cancel a running rebuild; a second press leaves |
| `Enter` | close a finished run, or act on the selected row in a dialog |
| `F2` | release the mouse, so text can be selected |

In the run picker the arrows move, `Enter` opens a run, `R` reverts the lock that
run replaced (it asks first, because the checkout's `flake.lock` is overwritten, and
it lands on the state the run before it left), `/` searches the columns and `Esc`
(or `q`) leaves. In the lock picker the arrows move, `Enter` (or a click
on the row) puts that lock back and rebuilds, and `Esc` (or `q`) leaves; there is
no search, because the list holds at most twenty rows. In the Settings dialog `↑`
`↓` (or `j` `k`) move, `Home` and `End` jump to the ends, and `←` `→` (`Enter`,
`Space`) change the selected row. `nix-rebuild settings` is that dialog in a frame
of its own, so closing it closes the frame: there is nothing behind it.

`D` opens one dialog with the run's own facts: the id it is recorded under (or
"not recorded"), the flake, whether the tree was dirty, the action, the cleanup
policy, the scope and the elapsed time, plus what the run counted.

## Dialogs

A dialog owns the keyboard and the pointer while it is up: no key and no click
reaches the cards behind it, and hover lights nothing there. A press on the dimmed
backdrop closes it, which for a question is the answer `Esc` gives. The cleanup
question follows the same rule: only `y`, `n`, `Esc` and `Enter` settle it, and
`Ctrl+C` still quits.

Both filter dialogs end with a "Save as default" row, which `S` also triggers: the
Logs one keeps its query and its kinds, the Activity one its three choosers, and
the next start opens filtered that way.

## Copying the logs

`C` copies every line through `wl-copy` or `xclip`, trying both. Under a system
run the frame is elevated, so the display variables travel with it, and a copy
that cannot reach the session is reported in the Logs card instead of failing in
silence.
