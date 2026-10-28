//! The capabilities of nix-rebuild, grouped by what the user is doing: watching the rebuild
//! (`panels`), revisiting a run that already happened (`runs`), or running the work (`work`).
pub mod panels;
pub mod runs;
pub mod work;
