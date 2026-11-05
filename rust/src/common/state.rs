//! Where nix-rebuild keeps its own files: `$XDG_STATE_HOME`, else `~/.local/state`.

use std::path::PathBuf;

/// The state directory the journal and the kept locks live under.
pub fn state_base() -> PathBuf {
    std::env::var("XDG_STATE_HOME")
        .map(PathBuf::from)
        .or_else(|_| std::env::var("HOME").map(|home| PathBuf::from(home).join(".local/state")))
        .unwrap_or_else(|_| PathBuf::from(".local/state"))
}
