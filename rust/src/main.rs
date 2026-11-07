use std::env;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::Context;
use clap::Parser;
use nix::unistd::geteuid;

use nix_rebuild::app::model::{App, Overlay};
use nix_rebuild::app::runtime;
use nix_rebuild::common::cleanup_policy::CleanupPolicy;
use nix_rebuild::common::cli::{Cli, Target};
use nix_rebuild::common::config::Settings;
use nix_rebuild::common::records;
use nix_rebuild::common::scope::Scope;
use nix_rebuild::common::setup::{self, HomeManager};
use nix_rebuild::common::terminal;
use nix_rebuild::common::tool_paths::{FORWARDED_TOOLS, ToolPaths};
use nix_rebuild::modules::locks;
use nix_rebuild::modules::panels::logs;
use nix_rebuild::modules::runs::picker;
use nix_rebuild::modules::runs::records as store;
use nix_rebuild::modules::work::update::{self, Step};

/// What the elevated frame still needs from the user's session: the clipboard tools
/// talk to the compositor, and sudo resets the environment.
const FORWARDED_SESSION: &[&str] = &[
    "WAYLAND_DISPLAY",
    "DISPLAY",
    "XAUTHORITY",
    "XDG_RUNTIME_DIR",
];

/// The tools a run spawns. `impure` is applied here so `ToolPaths::from_env` stays environment-independent.
fn tools(cli: &Cli) -> ToolPaths {
    ToolPaths {
        impure: setup::impure(),
        trust_flake_config: cli.trust_flake_config(),
        ..ToolPaths::from_env()
    }
}

fn main() -> ExitCode {
    // The frame owns the screen: a panic has to give it back before printing, or the
    // message would be drawn on the alternate screen and wiped with it.
    nix_rebuild::common::terminal::restore_on_panic();
    match real_main() {
        Ok(code) => terminal::exit_code(code),
        Err(error) => {
            eprintln!("nix-rebuild: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn real_main() -> anyhow::Result<i32> {
    let cli = Cli::parse();
    cli.check_run_arguments().map_err(anyhow::Error::msg)?;

    // The run selector can hand back a rollback: that one reads the checkout and the
    // declarations a rebuild needs, so it falls through to them instead of returning.
    let mut restore_lock: Option<String> = None;
    if cli.opens_a_run() {
        restore_lock = runs_command(&cli)?;
        if restore_lock.is_none() {
            return Ok(0);
        }
    }

    if cli.target.is_some_and(Target::edits_settings) {
        return settings_frame(&cli);
    }

    // A listing is a reading, not a rebuild: it needs no checkout, no declaration, no terminal.
    if cli.opens_locks() && cli.list {
        terminal::write_stdout(&locks::listing())?;
        return Ok(0);
    }

    let Some(target) = cli.target else {
        anyhow::bail!(
            "a target is required: `nix-rebuild system`, `nix-rebuild home`, \
             `nix-rebuild update`, `nix-rebuild locks` or `nix-rebuild runs`"
        );
    };
    if target == Target::Home && cli.boot {
        anyhow::bail!("--boot is only valid with the system scope");
    }
    // Read after the arguments are known: a bare invocation must not write a settings file.
    let settings = Settings::load();

    // A rollback from the run selector names its own checkout: the lock was kept for it.
    let restore = match &restore_lock {
        Some(id) => Some(locks::meta(id).ok_or_else(|| {
            anyhow::anyhow!(
                "the lock that run kept is no longer stored; `nix-rebuild locks --list` \
                 lists the ones that are"
            )
        })?),
        None => None,
    };
    let named = restore
        .as_ref()
        .map(|lock| lock.checkout.as_str())
        .or(settings.checkout.as_deref());
    let Some(checkout) = cli.checkout(named) else {
        anyhow::bail!(
            "no checkout is configured: declare `checkout` in the nix-rebuild module, set \
             FLAKE_PATH, or pass --flake PATH"
        );
    };
    // Each declaration is read only by the runs that use it.
    let system_config = (target != Target::Home)
        .then(setup::system_config)
        .transpose()?;
    let home_manager = match target {
        Target::System => None,
        Target::Home => {
            let kind = setup::home_manager()?;
            // A Home Manager that is a NixOS profile has nothing for `home` to activate, and this run says so.
            if kind != HomeManager::Standalone {
                anyhow::bail!("{}", kind.why_home_rebuilds_nothing());
            }
            Some(kind)
        }
        Target::Update => Some(setup::home_manager()?),
        // A rollback builds and activates exactly what an update does, so it reads the same kind.
        Target::Locks => Some(setup::home_manager()?),
        // Only a rollback from the selector gets this far; a plain `runs` left above.
        Target::Runs => Some(setup::home_manager()?),
        Target::Settings => unreachable!("settings names no configuration"),
    };

    let system_flake = system_config
        .as_ref()
        .map(|name| format!("{checkout}#{name}"))
        .unwrap_or_default();
    let system_build = system_config
        .as_ref()
        .map(|name| format!("{checkout}#nixosConfigurations.{name}.config.system.build.toplevel"))
        .unwrap_or_default();

    // The home half is named only when something activates it, so the build lands on what the activations expect.
    let home_config = match home_manager {
        Some(HomeManager::Standalone) => Some(setup::home_config()?),
        _ => None,
    };
    let home_flake = home_config
        .as_ref()
        .map(|name| format!("{checkout}#{name}"))
        .unwrap_or_default();
    let home_build = home_config
        .as_ref()
        .map(|name| format!("{checkout}#homeConfigurations.\"{name}\".activationPackage"))
        .unwrap_or_default();
    let flake = if target == Target::Home {
        home_flake.clone()
    } else {
        system_flake.clone()
    };

    // One shape for the three verbs that build and activate both configurations.
    let plan_with = |home_manager: HomeManager| Deployment {
        checkout: &checkout,
        home_flake: &home_flake,
        system_flake: &system_flake,
        home_build: &home_build,
        system_build: &system_build,
        home_manager,
    };

    if target == Target::Update {
        let plan = plan_with(
            home_manager
                .context("an update plans with the kind of Home Manager this machine runs")?,
        );
        return update(&cli, &settings, &plan);
    }

    if target == Target::Locks {
        let plan = plan_with(
            home_manager
                .context("a rollback plans with the kind of Home Manager this machine runs")?,
        );
        return locks_command(&cli, &settings, &plan);
    }

    if let Some(lock) = &restore {
        let plan = plan_with(
            home_manager
                .context("a rollback plans with the kind of Home Manager this machine runs")?,
        );
        return restore_and_rebuild(&cli, &settings, &plan, lock);
    }

    let Some(scope) = target.scope() else {
        unreachable!("every target that reaches here is one scope");
    };

    if scope == Scope::Home && geteuid().is_root() {
        anyhow::bail!("home must run as your regular user, not root");
    }

    if scope == Scope::System && !geteuid().is_root() {
        return reexec_as_root(scope, &cli, &checkout);
    }

    let tools = tools(&cli);
    let command = tools.rebuild_command(scope, &flake, cli.boot);
    let policy = cli.cleanup_policy(settings.cleanup);
    let mut app = App::new(scope, flake, cli.boot, policy);
    // Settings fill the view; explicit flags already won.
    app.apply_settings(&settings);
    runtime::run(&mut app, &command, tools)
}

/// What the verbs that build and activate both configurations need to name: the checkout,
/// both flake references, both build attributes, and how Home Manager is installed.
#[derive(Clone, Copy)]
struct Deployment<'a> {
    checkout: &'a str,
    home_flake: &'a str,
    system_flake: &'a str,
    home_build: &'a str,
    system_build: &'a str,
    home_manager: HomeManager,
}

/// Refresh the lock file and activate both configurations under one frame.
/// Keeps the lock it replaces first, and takes the sudo credential before the frame
/// because the frame cannot ask for a password.
fn update(cli: &Cli, settings: &Settings, deployment: &Deployment<'_>) -> anyhow::Result<i32> {
    let Deployment {
        checkout,
        home_flake,
        system_flake,
        home_build,
        system_build,
        home_manager,
    } = *deployment;
    if geteuid().is_root() {
        anyhow::bail!("update must run as your regular user, not root");
    }
    // The lock file is refreshed from inside the checkout, so it has to be a directory.
    if !Path::new(checkout).is_dir() {
        anyhow::bail!(
            "update refreshes the lock file inside the checkout, so {checkout} must be a \
             directory; pass --flake PATH"
        );
    }
    // Keep what this update is about to replace, before `nix flake update` touches it.
    let kept = locks::save(checkout, locks::Reason::Update);
    if kept.is_some() {
        locks::prune(settings.keep_locks);
    }

    let tools = tools(cli);
    let steps = update::plan(
        &tools,
        checkout,
        home_flake,
        system_flake,
        home_build,
        system_build,
        cli.boot,
        home_manager,
    );
    activate(
        cli,
        settings,
        deployment,
        steps,
        tools,
        "update",
        kept.map(|kept| kept.id),
    )
}

/// The `locks` verb: put a kept lock back on the checkout and rebuild with it.
fn locks_command(
    cli: &Cli,
    settings: &Settings,
    deployment: &Deployment<'_>,
) -> anyhow::Result<i32> {
    let id = match cli.id.as_deref() {
        Some(id) => id.to_owned(),
        None => {
            if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
                anyhow::bail!(
                    "the lock selector needs a terminal; `nix-rebuild locks --list` prints the \
                     saved locks and `nix-rebuild locks <ID>` restores one"
                );
            }
            let saved = locks::list();
            // Nothing kept yet: the listing says what puts one there, which a frame cannot.
            if saved.is_empty() {
                terminal::write_stdout(&locks::listing())?;
                return Ok(0);
            }
            // The selector marks the lock the checkout holds now, so it reads that digest first.
            let current = locks::current(deployment.checkout);
            match locks::pick_lock(saved, current)? {
                Some(id) => id,
                // Leaving the selector without choosing is not a failed run.
                None => return Ok(0),
            }
        }
    };

    let lock = locks::meta(&id).ok_or_else(|| {
        anyhow::anyhow!("no saved lock named `{id}`; `nix-rebuild locks --list` lists them")
    })?;
    restore_and_rebuild(cli, settings, deployment, &lock)
}

/// Put a kept lock back on the checkout and rebuild with the queue it pins. The `locks` verb
/// and `R` in the run selector share this, so a restored lock always rebuilds the same way.
fn restore_and_rebuild(
    cli: &Cli,
    settings: &Settings,
    deployment: &Deployment<'_>,
    lock: &locks::SavedLock,
) -> anyhow::Result<i32> {
    if geteuid().is_root() {
        anyhow::bail!("a rollback must run as your regular user, not root");
    }
    // A rollback writes the lock file inside the checkout, so it has to be a directory.
    let checkout = deployment.checkout;
    if !Path::new(checkout).is_dir() {
        anyhow::bail!(
            "a rollback writes the saved lock into the checkout, so {checkout} must be a \
             directory; pass --flake PATH"
        );
    }

    // The restore keeps the lock it replaces, so this rollback can itself be undone.
    let restored = locks::restore(&lock.id, checkout)?;
    locks::prune(settings.keep_locks);

    let tools = tools(cli);
    let steps = update::plan_pinned(
        &tools,
        deployment.home_flake,
        deployment.system_flake,
        deployment.home_build,
        deployment.system_build,
        cli.boot,
        deployment.home_manager,
    );
    activate(
        cli,
        settings,
        deployment,
        steps,
        tools,
        "rollback",
        restored.kept,
    )
}

/// Take the sudo ticket, then run the steps under one frame. An update and a rollback plan
/// different queues and share everything after the plan.
fn activate(
    cli: &Cli,
    settings: &Settings,
    deployment: &Deployment<'_>,
    steps: Vec<Step>,
    tools: ToolPaths,
    verb: &str,
    kept_lock: Option<String>,
) -> anyhow::Result<i32> {
    let credential = Command::new("sudo")
        .arg("-v")
        .status()
        .context("ask sudo for a credential")?;
    if !credential.success() {
        anyhow::bail!("{verb} needs sudo; the credential was not granted");
    }

    let policy = cli.cleanup_policy(settings.cleanup);
    let first = steps.first().context("a run has steps")?;
    let mut app = App::new(first.scope, deployment.checkout.into(), cli.boot, policy);
    // The journal names the verb that started this queue and the lock it kept on the way.
    app.flow = verb.into();
    app.kept_lock = kept_lock;
    // One run activates both profiles, so its cleanup prunes both; the system half asks for
    // the sudo ticket taken above, because the frame stays unprivileged.
    app.cleanup.scope = update::cleanup_scope(deployment.home_manager);
    // Settings fill the view; explicit flags already won.
    app.apply_settings(settings);
    runtime::run_steps(&mut app, steps, tools)
}

/// The `settings` verb: the same dialog the frame opens, in a frame with nothing to run.
fn settings_frame(cli: &Cli) -> anyhow::Result<i32> {
    let mut app = App::new(Scope::Home, String::new(), false, CleanupPolicy::Ask);
    app.apply_settings(&Settings::load());
    // The dialog is the point of this frame, so it opens at once; no card claims work here, or
    // the Run card would read `running` with a clock over a verb that rebuilds nothing.
    app.overlay = Overlay::Settings;
    app.config.show_status = false;
    app.config.show_activity = false;
    app.config.show_logs = false;
    runtime::run_settings(&mut app, tools(cli))
}

/// The `runs` verb. A rollback asked for in the selector comes back as the lock to restore:
/// it needs the checkout and the declarations, so the caller finishes it.
fn runs_command(cli: &Cli) -> anyhow::Result<Option<String>> {
    if cli.delete || cli.delete_all {
        delete_runs(cli)?;
        return Ok(None);
    }
    if cli.list {
        terminal::write_stdout(&store::listing())?;
        return Ok(None);
    }
    if cli.logs {
        // `--logs` never takes the screen; no id means the newest run.
        print_run_log(cli.id.as_deref().unwrap_or("last"))?;
        return Ok(None);
    }
    if let Some(id) = cli.id.as_deref() {
        open_run(id)?;
        return Ok(None);
    }
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!(
            "the run selector needs a terminal; `nix-rebuild runs --list` prints the runs \
             and `nix-rebuild runs <ID> --logs` prints one"
        );
    }
    let runs = store::list();
    // Nothing recorded: show the listing instead of a frame.
    if runs.is_empty() {
        terminal::write_stdout(&store::listing())?;
        return Ok(None);
    }
    match picker::pick_run(runs)? {
        Some(picker::Pick::Open(id)) => {
            open_run(&id)?;
            Ok(None)
        }
        // The reader confirmed it wants the lock that run replaced put back.
        Some(picker::Pick::Rollback(lock)) => Ok(Some(lock)),
        // Leaving the selector without choosing is not a failed run.
        None => Ok(None),
    }
}

/// Delete recorded runs: one named run, or all of them. A terminal answers first.
fn delete_runs(cli: &Cli) -> anyhow::Result<i32> {
    if cli.delete_all {
        let runs = store::list();
        if runs.is_empty() {
            terminal::write_stdout("no recorded runs to delete\n")?;
            return Ok(0);
        }
        let noun = if runs.len() == 1 { "run" } else { "runs" };
        terminal::write_stdout(&format!(
            "delete {} {noun} from {}\n",
            runs.len(),
            store::runs_dir().display()
        ))?;
        if !confirm(cli.yes)? {
            terminal::write_stdout("nothing deleted\n")?;
            return Ok(0);
        }
        let deleted = store::remove_all();
        let noun = if deleted == 1 { "run" } else { "runs" };
        terminal::write_stdout(&format!("deleted {deleted} {noun}\n"))?;
        return Ok(0);
    }

    let id = cli.id.as_deref().context("a deletion names a run")?;
    let Some(meta) = store::meta(id) else {
        anyhow::bail!("no recorded run named `{id}`; `nix-rebuild runs --list` lists them");
    };
    terminal::write_stdout(&format!(
        "delete {} · {} · {} · {}\n",
        meta.id,
        meta.scope,
        meta.action,
        meta.outcome()
    ))?;
    if !confirm(cli.yes)? {
        terminal::write_stdout("nothing deleted\n")?;
        return Ok(0);
    }
    if !store::remove(&meta.id) {
        anyhow::bail!("could not delete {}", meta.id);
    }
    terminal::write_stdout(&format!("deleted {}\n", meta.id))?;
    Ok(0)
}

/// Ask before deleting. `--yes` answers in advance, and a pipe cannot answer at all.
fn confirm(assume_yes: bool) -> anyhow::Result<bool> {
    if assume_yes {
        return Ok(true);
    }
    if !io::stdin().is_terminal() {
        anyhow::bail!("deleting a recorded run asks first; pass --yes to delete without asking");
    }
    terminal::write_stdout("Delete? [y/N] ")?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

/// A recorded run's log with nothing drawn around it.
fn print_run_log(id: &str) -> anyhow::Result<i32> {
    let (meta, records) = load_run(id)?;
    let app = App::reopen(&meta, &records);
    terminal::write_stdout(&format!("{}{}", meta.heading(), logs::logs_text(&app.logs)))?;
    Ok(0)
}

fn open_run(id: &str) -> anyhow::Result<i32> {
    let (meta, records) = load_run(id)?;
    let mut app = App::reopen(&meta, &records);
    // The settings file reaches a replay too: starting panels and what the terminal is left with.
    app.apply_settings(&Settings::load());
    runtime::run_replay(&mut app)
}

fn load_run(id: &str) -> anyhow::Result<(records::Meta, Vec<records::Record>)> {
    store::load(id).ok_or_else(|| {
        anyhow::anyhow!("no recorded run named `{id}`; `nix-rebuild runs --list` lists them")
    })
}

/// The user's clipboard tool, resolved while the frame still has the user's PATH.
fn clipboard_tool() -> Option<PathBuf> {
    let path = env::var_os("PATH")?;
    for name in ["wl-copy", "xclip"] {
        for dir in env::split_paths(&path) {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn reexec_as_root(scope: Scope, cli: &Cli, checkout: &str) -> anyhow::Result<i32> {
    let executable = env::current_exe().context("resolve nix-rebuild executable")?;
    let mut command = Command::new("sudo");
    command.arg("env").arg(format!("FLAKE_PATH={checkout}"));
    // Name the settings file explicitly: sudo's environment need not carry XDG_CONFIG_HOME.
    command.arg(format!("NIX_REBUILD_CONFIG={}", Settings::path().display()));
    // Root may not see the user's profile on PATH: name the clipboard tool too.
    if let Some(tool) = clipboard_tool() {
        command.arg(format!("NIX_REBUILD_CLIPBOARD={}", tool.display()));
    }
    for variable in FORWARDED_TOOLS
        .iter()
        .chain(setup::DECLARATIONS)
        .chain(FORWARDED_SESSION)
    {
        if let Ok(value) = env::var(variable) {
            command.arg(format!("{variable}={value}"));
        }
    }
    command.arg(executable).arg(scope.to_string());
    // The child resolves the same checkout the parent did, whatever its settings file says.
    command.arg("--flake").arg(checkout);
    if cli.boot {
        command.arg("--boot");
    }
    if cli.accept_flake_config {
        command.arg("--accept-flake-config");
    }
    if cli.yes {
        command.arg("--yes");
    }
    if cli.no_clean {
        command.arg("--no-clean");
    }
    let status = command.status().context("run sudo")?;
    Ok(status.code().unwrap_or(1))
}
