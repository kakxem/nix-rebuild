//! The update: the queue of steps `nix-rebuild update` runs.

mod state;

pub use state::{
    CredentialKeepAlive, Step, Steps, advance_step, cleanup_scope, needs_elevated_credential, plan,
    run_plain_steps, spawn_child,
};

#[cfg(test)]
mod tests;
