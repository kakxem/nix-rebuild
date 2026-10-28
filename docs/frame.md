[nix-rebuild](../README.md) / The frame

# The frame

## The alternate screen

The frame fills the terminal's alternate screen, so nothing of it lands in the
shell's scrollback. A panic is printed on the shell's screen, not inside a frame
that is about to disappear.

When the frame closes, `leave` decides what the shell is left with. `clean` prints
the run's closing line under the prompt, and nothing at all for a reopened run.
`frame` draws the last frame again on the shell's screen.

## The Status card

Three rows and a badge. The left column is the checkout being built, shortened as
the card narrows, with `· dirty` when the tree has uncommitted changes. The middle
is the step on the card's centre: the scope and the action. The right is the
clock. The badge in the title is the phase: `preparing`, `evaluating`, `building`,
`activating`, `cleaning`, `complete`, `failed`, `cancelling` or `cancelled`.

The card keeps its three rows, which is why the border under it is not a drag
handle.

## The Activity card

One row per piece of work, built from Nix's own events.

| column | what it holds |
|---|---|
| package | the store name of the path, or the cache being asked |
| state | `queued`, `downloading`, `copying`, `querying`, the build phase while it runs (`compiling`, `configuring`, `installing`), then `done`, `failed` or `not built` |
| time | how long the row has been open |
| transfer | the bytes moved for a download, the distinct paths for a fetch or a probe |

A download shows `84.0 MiB/132.0 MiB` while it moves, and the total alone when it
finishes. A fetch of a flake input or a probe of a cache carries no bytes, so it
has no progress and no ETA: the count column is what moves. Both kinds close at
the first build, which is the fetching being over rather than a failure.

The widths are solved from the card rather than taken as percentages of it: the
glyph is one cell so the name starts one space after it, the state, clock and
transfer columns are as wide as their longest content, and the name column takes
the slack. It keeps two columns of air from the border on each side, like the
Logs records. The transfer column is flush with the card's edge and its figures are
right-aligned, so a resize never leaves a strip of empty cells after the last
column.

`V` swaps the card for the cleanup steps, and `T` filters it by kind.

## The Logs card

Every line Nix prints, tagged with the kind of work behind it: `eval`, `fetch`,
`build`, `activate`, `clean`, `info`, `warning` and `error`. The tool relabels
what Nix writes, so `copying path ... from <url>` reads as `downloading <name>`,
and a probe shows as the cache host rather than as the path asked about. The
records sit one row and two columns inside the border, the padding every card
but the Run card keeps, so neither the first nor the last of them touches a
rule. Two columns rather than one because a Nerd Font glyph is drawn wider than
the cell it occupies, which eats a single column of air.

`F` opens the filter: a query, a raw-lines toggle and one row per kind. `Ctrl+R`
toggles the raw lines without opening the dialog, and `C` copies the whole log.

## Resizing

A card's border is a drag handle: grab the row where two cards meet and the pair
trades rows, so the total stays the same. The Status card is the exception. The
split belongs to the window and not to the file, and it goes back to the default
when the terminal changes size.

The Settings dialog is not part of the stack: it covers the cards, and its own
list scrolls inside the box. See [settings](settings.md).
