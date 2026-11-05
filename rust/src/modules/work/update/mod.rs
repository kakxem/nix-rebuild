//! The update: the queue of steps `nix-rebuild update` runs.

mod state;

pub use state::{
    CredentialKeepAlive, Step, Steps, advance_step, cleanup_scope, lock_step,
    needs_elevated_credential, plan, plan_pinned, run_plain_steps, spawn_child,
};

#[cfg(test)]
mod tests;
