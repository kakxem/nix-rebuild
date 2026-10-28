[nix-rebuild](../README.md) / Development

# Development

## The devShell

`cargo`, `rustfmt`, `clippy` and `nixfmt` come from the flake's devShell, which is
the only place they are on `PATH`.

```
nix develop -c bash -c 'cd rust && cargo fmt --check && cargo test'
nix develop -c bash -c 'cd rust && cargo clippy --all-targets -- -D warnings'
nix flake check
```

`nix flake check` builds the package, which runs the test suite in the sandbox,
runs the formatting check over the Rust and Nix files, and evaluates both
installers. CI runs the same check plus clippy, so a warning fails the workflow.

## Tests

The tests are prose: they assert on the exact sentence a card or a footer shows,
with a message explaining why. A wording change is never one edit, so read what a
failing test claims about the screen before touching the code.

Tests use fake tools on an isolated `PATH`, a scratch checkout and stdin from
`/dev/null`, so none of them launches a real rebuild. A probe that wants to see a
card render can print it through the test backend instead of starting a run.

## The package

`default.nix` pins `nixos-rebuild` from this flake's nixpkgs. On nixpkgs unstable
that is `nixos-rebuild-ng`, a drop-in rewrite, and the argv this tool builds works
with both. The package's source is the repo root, with `cargoRoot` and
`buildAndTestSubdir` at `rust`, because a test reads the fish completion from
outside `rust/`.

## The screenshot

`assets/cli.png` is the frame captured at 120x32, with a scripted rebuild feeding
it Nix's own events, then rendered with the terminal's font and palette. Refresh
it when the frame's layout or colours move.
