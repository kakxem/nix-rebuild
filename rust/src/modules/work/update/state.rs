//! The update's step queue: the steps `nix-rebuild update` runs, the plan
//! that spells them, and the runner that advances past a clean exit.

use std::collections::VecDeque;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command as ProcessCommand, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use anyhow::Context;
use crossbeam_channel::Sender;

use crate::common::progress::StepProgress;
use crate::common::scope::Scope;
use crate::common::setup::HomeManager;
use crate::common::terminal::{RuntimeEvent, spawn_reader};
use crate::common::tool_paths::{FORWARDED_TOOLS, ToolPaths, nix_config_for};

/// One command in a run: one step for a rebuild, four for an update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub label: String,
    pub scope: Scope,
    /// Exact Run card text: `update` for the lock file, `switch` for activations.
    pub action: String,
    pub command: Vec<String>,
    pub dir: Option<PathBuf>,
    /// Argv after `sudo -n env`, which resets the environment.
    pub elevated: Option<Vec<String>>,
}

impl Step {
    /// Argv to spawn. After `env`, tool paths are re-declared (sudo resets the
    /// environment) and a framed run passes `NIX_CONFIG` with the event log.
    pub fn resolved_command(&self, tools: &ToolPaths, log: Option<&Path>) -> Vec<String> {
        match &self.elevated {
            None => self.command.clone(),
            Some(elevated) => {
                let mut command = self.command.clone();
                for variable in FORWARDED_TOOLS {
                    if let Some(value) = tools.path(variable) {
                        command.push(format!("{variable}={value}"));
                    }
                }
                let config = match log {
                    Some(log) => nix_config_for(log),
                    None => std::env::var("NIX_CONFIG").unwrap_or_default(),
                };
                if !config.is_empty() {
                    command.push(format!("NIX_CONFIG={config}"));
                }
                command.extend(elevated.clone());
                command
            }
        }
    }
}

/// The lock file's own step, run from inside the checkout so `nix flake update` finds it.
pub fn lock_step(tools: &ToolPaths, checkout: &str) -> Step {
    let mut command = vec![tools.nix(), "flake".into(), "update".into()];
    tools.push_trust_flake_config(&mut command);
    tools.push_impure(&mut command);
    Step {
        label: "nix flake update".into(),
        scope: Scope::Home,
        action: "update".into(),
        command,
        dir: Some(PathBuf::from(checkout)),
        elevated: None,
    }
}

/// Build both generations, then activate: one `nix build` for the paths both share, and a
/// NixOS-profile Home Manager activates once. A rollback runs these steps after putting a
/// kept lock back, so its queue starts at the build and refreshes nothing.
#[allow(clippy::too_many_arguments)]
pub fn plan_pinned(
    tools: &ToolPaths,
    home_flake: &str,
    system_flake: &str,
    home_build: &str,
    system_build: &str,
    boot: bool,
    home_manager: HomeManager,
) -> Vec<Step> {
    let standalone = home_manager == HomeManager::Standalone;

    // Activations find their generation in the store; `--no-link` avoids
    // leaving a `result` symlink behind.
    let mut steps = vec![Step {
        label: if standalone {
            "nix build home and system"
        } else {
            "nix build system"
        }
        .into(),
        scope: if standalone {
            Scope::Both
        } else {
            Scope::System
        },
        action: "build".into(),
        command: {
            let mut command = vec![tools.nix(), "build".into(), "--no-link".into()];
            if standalone {
                command.push(home_build.into());
            }
            command.push(system_build.into());
            tools.push_trust_flake_config(&mut command);
            tools.push_impure(&mut command);
            command
        },
        dir: None,
        elevated: None,
    }];

    if standalone {
        steps.push(Step {
            label: "home-manager switch".into(),
            scope: Scope::Home,
            action: "switch".into(),
            command: tools.rebuild_command(Scope::Home, home_flake, false),
            dir: None,
            elevated: None,
        });
    }

    steps.push(Step {
        label: if boot {
            "nixos-rebuild boot"
        } else {
            "nixos-rebuild switch"
        }
        .into(),
        scope: Scope::System,
        action: "switch".into(),
        command: vec!["sudo".into(), "-n".into(), "env".into()],
        dir: None,
        elevated: Some(tools.rebuild_command(Scope::System, system_flake, boot)),
    });

    steps
}

/// The whole update: refresh the lock from inside the checkout, then build and activate.
#[allow(clippy::too_many_arguments)]
pub fn plan(
    tools: &ToolPaths,
    checkout: &str,
    home_flake: &str,
    system_flake: &str,
    home_build: &str,
    system_build: &str,
    boot: bool,
    home_manager: HomeManager,
) -> Vec<Step> {
    let mut steps = vec![lock_step(tools, checkout)];
    steps.extend(plan_pinned(
        tools,
        home_flake,
        system_flake,
        home_build,
        system_build,
        boot,
        home_manager,
    ));
    steps
}

/// What an update's cleanup prunes: both profiles when Home Manager is standalone,
/// because the run activates both, and the system one alone otherwise.
pub fn cleanup_scope(home_manager: HomeManager) -> Scope {
    if home_manager == HomeManager::Standalone {
        Scope::Both
    } else {
        Scope::System
    }
}

pub struct Steps {
    queue: VecDeque<Step>,
    running: Option<Step>,
    total: usize,
    index: usize,
    /// What the Run card shows: set when a step starts, `None` for a run of one.
    progress: Option<StepProgress>,
}

impl Steps {
    pub fn new(steps: Vec<Step>) -> Self {
        let total = steps.len();
        Self {
            queue: VecDeque::from(steps),
            running: None,
            total,
            index: 0,
            progress: None,
        }
    }

    pub fn start_next(&mut self) -> Option<Step> {
        let step = self.queue.pop_front()?;
        self.index += 1;
        // A run of one is not a sequence, so the card shows no step progress.
        self.progress = (self.total > 1).then(|| StepProgress {
            index: self.index,
            total: self.total,
            label: step.label.clone(),
        });
        self.running = Some(step.clone());
        Some(step)
    }

    /// The card's step line, for the frame that has just started a step.
    pub fn progress(&self) -> Option<StepProgress> {
        self.progress.clone()
    }

    /// Label a failure belongs to; `rebuild` for a single-step run.
    pub fn running_label(&self) -> &str {
        if self.total <= 1 {
            return "rebuild";
        }
        self.running
            .as_ref()
            .map(|step| step.label.as_str())
            .unwrap_or("rebuild")
    }
}

/// Spawn the next step after a clean exit; the first failure stops the run.
pub fn advance_step(
    steps: &mut Steps,
    failed: bool,
    tools: &ToolPaths,
    log_path: &Path,
    tx: &Sender<RuntimeEvent>,
) -> anyhow::Result<Option<(Step, Child)>> {
    if failed {
        return Ok(None);
    }
    let Some(next) = steps.start_next() else {
        return Ok(None);
    };
    let child = spawn_child(&next, tools, log_path, tx)?;
    Ok(Some((next, child)))
}

/// Spawn one step's command, streaming its output into events.
pub fn spawn_child(
    step: &Step,
    tools: &ToolPaths,
    log_path: &Path,
    tx: &Sender<RuntimeEvent>,
) -> anyhow::Result<Child> {
    let command = step.resolved_command(tools, Some(log_path));
    let (program, args) = command.split_first().context("empty rebuild command")?;
    let mut process = ProcessCommand::new(program);
    process
        .args(args)
        .env("NIX_CONFIG", nix_config_for(log_path))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(dir) = &step.dir {
        process.current_dir(dir);
    }
    let mut child = process
        .spawn()
        .with_context(|| format!("start {program}"))?;
    if let Some(stdout) = child.stdout.take() {
        spawn_reader(stdout, tx.clone(), RuntimeEvent::Output);
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader(stderr, tx.clone(), RuntimeEvent::Stderr);
    }
    Ok(child)
}

/// Run the steps without a frame, for redirects and CI. An elevated step must
/// carry its tool paths and rebuild: bare `sudo -n env` exits clean and does nothing.
pub fn run_plain_steps(steps: &[Step], tools: &ToolPaths) -> anyhow::Result<i32> {
    let _keep_alive = CredentialKeepAlive::for_steps(steps);
    for step in steps {
        let command = step.resolved_command(tools, None);
        let (program, args) = command.split_first().context("empty rebuild command")?;
        let mut process = ProcessCommand::new(program);
        process.args(args);
        if let Some(dir) = &step.dir {
            process.current_dir(dir);
        }
        let status = process
            .status()
            .with_context(|| format!("start {program}"))?;
        if !status.success() {
            return Ok(status.code().unwrap_or(1));
        }
    }
    Ok(0)
}

/// Refresh interval: sudo's ticket lasts five minutes (`sudoers(5)`) and the
/// earlier steps can outlive it, so `sudo -n` must not meet a dead ticket.
const CREDENTIAL_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// Sleep slice: small, so stopping the refresher costs one tick.
const CREDENTIAL_REFRESH_TICK: Duration = Duration::from_millis(50);

/// Refresh command. `-n` matters: the credential predates the frame taking the
/// screen, so a password prompt cannot be answered.
const CREDENTIAL_REFRESH_COMMAND: [&str; 3] = ["sudo", "-n", "-v"];

pub fn needs_elevated_credential(steps: &[Step]) -> bool {
    steps.iter().any(|step| step.elevated.is_some())
}

/// Keeps the sudo ticket alive so a late elevated step still finds it.
pub struct CredentialKeepAlive {
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

impl CredentialKeepAlive {
    pub fn for_steps(steps: &[Step]) -> Self {
        if !needs_elevated_credential(steps) {
            return Self {
                stop: Arc::new(AtomicBool::new(false)),
                thread: None,
            };
        }
        Self::start(
            &CREDENTIAL_REFRESH_COMMAND,
            CREDENTIAL_REFRESH_INTERVAL,
            || {},
        )
    }

    /// Refresh `command` every `interval` until dropped. `on_refresh` runs after
    /// each attempt; it is how tests observe a refresh that leaves no trace.
    pub fn start(
        command: &[&str],
        interval: Duration,
        mut on_refresh: impl FnMut() + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = Arc::clone(&stop);
            let command: Vec<String> = command.iter().map(|part| (*part).to_owned()).collect();
            thread::spawn(move || {
                let mut waited = Duration::ZERO;
                while !stop.load(Ordering::Relaxed) {
                    thread::sleep(CREDENTIAL_REFRESH_TICK);
                    waited += CREDENTIAL_REFRESH_TICK;
                    if waited < interval {
                        continue;
                    }
                    waited = Duration::ZERO;
                    on_refresh();
                    if let Some((program, args)) = command.split_first() {
                        // The ticket belongs to this terminal: keep its stdin.
                        let _ = ProcessCommand::new(program)
                            .args(args)
                            .stdin(Stdio::inherit())
                            .stdout(Stdio::null())
                            .stderr(Stdio::null())
                            .status();
                    }
                }
            })
        };
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for CredentialKeepAlive {
    /// Stop the refresher and join it, so a finished run never leaves one behind.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
