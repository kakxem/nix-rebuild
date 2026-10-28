//! Test harness shared by every slice: app, key, mouse, click and common fixtures.

use crossterm::event::{KeyEvent, MouseEvent};

use crate::app::model::App;
use crate::common::cleanup_policy::CleanupPolicy;
use crate::common::scope::Scope;

pub fn app() -> App {
    App::new(
        Scope::Home,
        "/repo#sample".into(),
        false,
        CleanupPolicy::Ask,
    )
}

/// Press a key through the real dispatch path (`handle_key`, then `reduce`), so
/// keyboard tests exercise it too.
pub fn press(app: &mut App, key: KeyEvent) -> Option<crate::common::actions::Command> {
    let command = crate::app::runtime::handle_key(app, key);
    if let Some(command) = command {
        crate::app::reduce::reduce(app, crate::common::actions::Action::Command(command));
    }
    command
}

/// Deliver a mouse event the way the runtime does.
pub fn mouse(app: &mut App, event: MouseEvent, panels: &crate::app::dashboard::Panels) {
    if let Some(command) = crate::app::runtime::handle_mouse_command(app, event, panels) {
        crate::app::reduce::reduce(app, crate::common::actions::Action::Command(command));
    }
}

pub fn click(app: &mut App, target: crate::common::hit_targets::Click) {
    crate::app::reduce::reduce(
        app,
        crate::common::actions::Action::Command(target.command()),
    );
}

/// A recorded run, as a list reads one. The checkout sits outside `$HOME` on purpose:
/// checkout-cell rendering must not depend on the environment tests run in.
pub fn run_meta(
    id: &str,
    scope: &str,
    exit_code: Option<i32>,
    status: &str,
) -> crate::common::records::Meta {
    run_meta_with_cleanup(id, scope, exit_code, None, status)
}

/// The same rows, with the cleanup the run ended on.
pub fn run_meta_with_cleanup(
    id: &str,
    scope: &str,
    exit_code: Option<i32>,
    cleanup_ok: Option<bool>,
    status: &str,
) -> crate::common::records::Meta {
    crate::common::records::Meta {
        id: id.into(),
        scope: scope.into(),
        flake: "/srv/checkouts/nixos-dotfiles#sample".into(),
        action: "switch".into(),
        cleanup: "ask".into(),
        started_at: 1_791_918_874,
        finished_at: Some(1_791_918_934),
        exit_code,
        cleanup_ok,
        status: status.into(),
    }
}

/// The derivation the failed run could not build.
pub const FAILED_DRV: &str = "/nix/store/pix54dha5hckxhbj3rc73f07kwg28b2i-nix-rebuild-0.1.0.drv";

use crate::common::phase::Phase;

pub fn app_with_phase(phase: Phase) -> App {
    let mut app = app();
    app.status.phase = phase;
    app
}

/// Drive the events a failed build produces in Nix's order: start, progress, stop,
/// then the error that names the derivation.
pub fn run_failed_build(app: &mut App) {
    app.ingest_json(&failed_build_start_json());
    app.ingest_json(r#"{"action":"result","fields":["compiling"],"id":1,"type":101}"#);
    app.ingest_json(r#"{"action":"stop","id":1}"#);
    app.ingest_json(&failed_build_json());
    app.finish_after_exit(true);
    app.status.phase = Phase::Failed;
}

/// Nix's failure event: one level-0 `msg` carrying the whole block, colours and newlines included.
pub fn failed_build_json() -> String {
    let lines = [
        format!("\\u001b[31;1merror:\\u001b[0m Cannot build '\\u001b[35;1m{FAILED_DRV}\\u001b[0m'."),
        "       Reason: \\u001b[31;1mbuilder failed with exit code 101\\u001b[0m.".to_owned(),
        "       Output paths:".to_owned(),
        "         \\u001b[35;1m/nix/store/av2i3dkhf1xzqi71wx3d7a48x8f48dgg-nix-rebuild-0.1.0\\u001b[0m"
            .to_owned(),
        "       Last 1 log lines:".to_owned(),
        "       > error[E0583]: file not found for module `config`".to_owned(),
        "       For full logs, run:".to_owned(),
        format!("         \\u001b[1mnix log {FAILED_DRV}\\u001b[0m"),
    ];
    format!(
        r#"{{"action":"msg","column":null,"file":null,"level":0,"line":null,"msg":"{}"}}"#,
        lines.join("\\n")
    )
}

/// The JSON line that opens the build the failure stopped.
pub fn failed_build_start_json() -> String {
    format!(
        r#"{{"action":"start","fields":["{FAILED_DRV}","",1,1],"id":1,"level":3,"parent":0,"text":"building '{FAILED_DRV}'","type":105}}"#
    )
}
