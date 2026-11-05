//! The capabilities of nix-rebuild, grouped by what the user is doing: watching the rebuild
//! (`panels`), revisiting a run that already happened (`runs`), putting a kept lock back
//! (`locks`), or running the work (`work`).
pub mod locks;
pub mod panels;
pub mod runs;
pub mod work;
