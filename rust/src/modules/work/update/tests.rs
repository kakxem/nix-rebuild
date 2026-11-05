//! Tests for the planned step sequence and each step's resolved command.

use crate::common::scope::Scope;
use crate::common::setup::HomeManager;
use crate::common::tool_paths::ToolPaths;

#[test]
fn an_update_cleans_both_profiles_when_it_activated_both() {
    assert_eq!(
        super::cleanup_scope(HomeManager::Standalone),
        Scope::Both,
        "the run switched both profiles, so its cleanup prunes both"
    );
    assert_eq!(
        super::cleanup_scope(HomeManager::Module),
        Scope::System,
        "a Home Manager inside the system configuration has no profile of its own to prune"
    );
}

#[test]
fn only_a_run_with_an_elevated_step_keeps_a_credential_alive() {
    let tools = ToolPaths::fake();
    let update = super::plan(
        &tools,
        "/repo",
        "/repo#sample",
        "/repo#system",
        "/repo#hb",
        "/repo#sb",
        false,
        HomeManager::Standalone,
    );
    assert!(
        super::needs_elevated_credential(&update),
        "the system step runs under sudo and needs its ticket kept alive"
    );

    let unprivileged = &update[..3];
    assert!(
        !super::needs_elevated_credential(unprivileged),
        "everything before the system step runs unprivileged"
    );
}

#[test]
fn a_plain_run_hands_an_elevated_step_its_whole_command() {
    use crate::common::tool_paths::FORWARDED_TOOLS;

    // `fake()` pins the tool names, and an ambient NIX_CONFIG is cleared because
    // a plain run inherits it verbatim into the argv.
    temp_env::with_var("NIX_CONFIG", None::<&str>, || {
        let tools = ToolPaths::fake();
        let steps = super::plan(
            &tools,
            "/repo",
            "/repo#h",
            "/repo#system",
            "/repo#hb",
            "/repo#sb",
            false,
            HomeManager::Standalone,
        );

        // A plain run has no frame or event log. It must never spawn the bare
        // `sudo -n env` it is built around: that exits clean and activates nothing.
        let command = steps[3].resolved_command(&tools, None);
        assert_eq!(
            command[..3].to_vec(),
            vec!["sudo".to_owned(), "-n".into(), "env".into()]
        );
        for variable in FORWARDED_TOOLS {
            let expected = tools.path(variable).expect("a forwarded name");
            assert!(
                command
                    .iter()
                    .any(|arg| arg == &format!("{variable}={expected}")),
                "the elevated step needs {variable} or it looks for the tool on root's PATH: {command:?}"
            );
        }
        let tail: Vec<String> = vec![
            tools.nixos_rebuild.clone(),
            "switch".into(),
            "--flake".into(),
            "/repo#system".into(),
        ];
        assert_eq!(
            command[command.len() - tail.len()..].to_vec(),
            tail,
            "the rebuild has to be there, after the assignments"
        );
        assert!(
            !command.iter().any(|arg| arg.contains("json-log-path")),
            "a plain run has no panels to feed, so it names no log: {command:?}"
        );
    });
}

#[test]
fn a_kept_credential_is_refreshed_on_a_schedule_and_stops_with_the_run() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    // sudo's ticket lasts five minutes, less than an update's earlier steps.
    let refreshes = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&refreshes);
    let binary = std::env::current_exe().expect("the test binary");
    let keep_alive = super::CredentialKeepAlive::start(
        &[binary.to_str().expect("utf-8 path"), "--list"],
        Duration::from_millis(20),
        move || {
            counted.fetch_add(1, Ordering::Relaxed);
        },
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    while refreshes.load(Ordering::Relaxed) < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        refreshes.load(Ordering::Relaxed) >= 2,
        "the refresher keeps firing while the run works, got {}",
        refreshes.load(Ordering::Relaxed)
    );

    drop(keep_alive);
    let stopped = refreshes.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(120));
    assert_eq!(
        refreshes.load(Ordering::Relaxed),
        stopped,
        "a finished run leaves no refresher behind"
    );
}

#[test]
fn an_update_builds_both_generations_then_activates_them() {
    // The same tool names on every machine: nothing here reads the caller's shell.
    let tools = ToolPaths::fake();
    let home_build = "/repo#homeConfigurations.\"sample\".activationPackage";
    let system_build = "/repo#nixosConfigurations.system.config.system.build.toplevel";
    let steps = super::plan(
        &tools,
        "/repo",
        "/repo#sample",
        "/repo#system",
        home_build,
        system_build,
        false,
        HomeManager::Standalone,
    );

    let labels: Vec<&str> = steps.iter().map(|step| step.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "nix flake update",
            "nix build home and system",
            "home-manager switch",
            "nixos-rebuild switch"
        ]
    );
    assert_eq!(
        steps[0].dir.as_deref(),
        Some(std::path::Path::new("/repo")),
        "the lock file is refreshed inside the checkout"
    );
    assert_eq!(steps[0].action, "update");

    // One build over both installables: shared paths are fetched and built once.
    assert_eq!(steps[1].scope, Scope::Both);
    assert_eq!(steps[1].action, "build");
    assert_eq!(
        steps[1].command,
        vec![
            tools.nix(),
            "build".to_owned(),
            "--no-link".into(),
            home_build.to_owned(),
            system_build.to_owned(),
        ],
        "the build names the attributes the two activations will look for"
    );

    assert_eq!(
        steps[2].command[0], tools.home_manager,
        "home activates its own output"
    );
    assert!(
        steps[2].command.contains(&"/repo#sample".to_owned()),
        "{:?}",
        steps[2].command
    );
    assert!(
        steps[0].elevated.is_none() && steps[1].elevated.is_none() && steps[2].elevated.is_none(),
        "only the system step needs root"
    );
    assert_eq!(steps[3].scope, Scope::System);

    // Order contract: credential, then tool paths and log, then the rebuild.
    let log = std::path::Path::new("/run/user/1000/nix-rebuild-1.jsonl");
    let resolved = steps[3].resolved_command(&tools, Some(log));
    assert_eq!(
        resolved[..3].to_vec(),
        vec!["sudo".to_owned(), "-n".into(), "env".into()]
    );
    assert!(
        resolved
            .iter()
            .any(|arg| arg == &format!("NIXOS_REBUILD_PATH={}", tools.nixos_rebuild)),
        "{resolved:?}"
    );
    assert!(
        resolved
            .iter()
            .any(|arg| arg.starts_with("NIX_CONFIG=") && arg.contains("nix-rebuild-1.jsonl")),
        "the elevated step writes to the same log, or it would be invisible"
    );
    assert_eq!(
        resolved[resolved.len() - 4..].to_vec(),
        vec![
            tools.nixos_rebuild.clone(),
            "switch".to_owned(),
            "--flake".into(),
            "/repo#system".into()
        ]
    );

    assert_eq!(
        steps[2].resolved_command(&tools, Some(log)),
        steps[2].command,
        "no environment is reset, so nothing has to be re-declared"
    );
    assert_eq!(
        steps[1].resolved_command(&tools, Some(log)),
        steps[1].command,
        "the build runs as the user, too"
    );

    // A boot update registers the generation instead of switching to it.
    let booting = super::plan(
        &tools,
        "/repo",
        "/repo#h",
        "/repo#system",
        "/repo#hb",
        "/repo#sb",
        true,
        HomeManager::Standalone,
    );
    assert_eq!(booting[3].label, "nixos-rebuild boot");
    assert!(
        booting[3]
            .elevated
            .as_ref()
            .expect("elevated")
            .contains(&"boot".to_owned())
    );
}

#[test]
fn a_machine_whose_home_manager_is_a_nixos_profile_updates_the_system_alone() {
    // Only labels are asserted, so no ambient NIX_* path belongs in the fixture.
    let tools = ToolPaths::fake();
    let system_build = "/repo#nixosConfigurations.system.config.system.build.toplevel";
    // Empty home arguments on purpose: a NixOS-module Home Manager names no home.
    let steps = super::plan(
        &tools,
        "/repo",
        "",
        "/repo#system",
        "",
        system_build,
        false,
        HomeManager::Module,
    );

    let labels: Vec<&str> = steps.iter().map(|step| step.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "nix flake update",
            "nix build system",
            "nixos-rebuild switch"
        ],
        "one generation is built and one activation runs"
    );
    assert_eq!(steps[1].scope, Scope::System);
    assert_eq!(
        steps[1].command,
        vec![
            tools.nix(),
            "build".to_owned(),
            "--no-link".into(),
            system_build.to_owned(),
        ],
        "the home generation is not named at all"
    );
    assert_eq!(
        steps[2].scope,
        Scope::System,
        "and only the system switches"
    );
}

#[test]
fn an_impure_update_passes_the_flag_to_every_command_that_reads_the_checkout() {
    let tools = ToolPaths {
        impure: true,
        ..ToolPaths::fake()
    };
    let steps = super::plan(
        &tools,
        "/repo",
        "/repo#sample",
        "/repo#system",
        "/repo#hb",
        "/repo#sb",
        false,
        HomeManager::Standalone,
    );

    // All four commands evaluate the flake. The system activation carries the
    // flag inside the argv sudo runs, the only place `nixos-rebuild` sees it.
    let elevated = steps[3]
        .elevated
        .as_ref()
        .expect("the system step elevates");
    for command in [
        &steps[0].command,
        &steps[1].command,
        &steps[2].command,
        elevated,
    ] {
        assert!(
            command.iter().any(|arg| arg == "--impure"),
            "every command that reads the checkout asks for an impure evaluation: {command:?}"
        );
    }

    // The flag must survive sudo's env reset, at the end of the rebuilt argv.
    let resolved = steps[3].resolved_command(&tools, None);
    assert_eq!(
        resolved.last().map(String::as_str),
        Some("--impure"),
        "the elevated rebuild keeps the flag after the assignments: {resolved:?}"
    );
}

#[test]
fn a_machine_without_home_manager_updates_the_same_way() {
    // Only labels are asserted, so no ambient NIX_* path belongs in the fixture.
    let tools = ToolPaths::fake();
    let steps = super::plan(
        &tools,
        "/repo",
        "",
        "/repo#s",
        "",
        "/repo#sb",
        true,
        HomeManager::None,
    );

    let labels: Vec<&str> = steps.iter().map(|step| step.label.as_str()).collect();
    assert_eq!(
        labels,
        ["nix flake update", "nix build system", "nixos-rebuild boot"],
        "no Home Manager means no home step, boot included"
    );
}

#[test]
fn a_rollback_plans_the_build_and_the_activations_without_refreshing_the_lock() {
    let tools = ToolPaths::fake();
    let steps = super::plan_pinned(
        &tools,
        "/repo#sample",
        "/repo#system",
        "/repo#hb",
        "/repo#sb",
        false,
        HomeManager::Standalone,
    );

    let labels: Vec<&str> = steps.iter().map(|step| step.label.as_str()).collect();
    assert_eq!(
        labels,
        [
            "nix build home and system",
            "home-manager switch",
            "nixos-rebuild switch"
        ],
        "a kept lock is already pinned, so the queue starts at the build"
    );
    assert_eq!(
        steps[0].scope,
        Scope::Both,
        "one build feeds both activations"
    );
    assert!(
        steps[2].elevated.is_some() && steps[1].elevated.is_none(),
        "only the system activation runs elevated"
    );
}
