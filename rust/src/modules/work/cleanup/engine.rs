//! The cleaner: generation pruning and garbage collection.

use std::fs;
use std::io::{self, BufRead};
use std::os::unix::process::CommandExt;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use crossbeam_channel::Sender;
use nix::sys::signal::{Signal, killpg};
use nix::unistd::{Pid, geteuid};

use crate::common::scope::Scope;
use crate::common::terminal::RuntimeEvent;
use crate::common::tool_paths::{FORWARDED_TOOLS, ToolPaths};

#[derive(Debug, Clone, Default)]
pub struct CleanupControl {
    current: Arc<Mutex<Option<i32>>>,
    cancelled: Arc<AtomicBool>,
}

impl CleanupControl {
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Relaxed)
    }

    /// Clear a previous pass's cancellation: a stopped cleaner must not poison the next attempt.
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::Relaxed);
        if let Ok(mut current) = self.current.lock() {
            *current = None;
        }
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let Some(pid) = self.current.lock().ok().and_then(|guard| *guard) else {
            return;
        };
        let _ = killpg(Pid::from_raw(pid), Signal::SIGINT);
        let control = self.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(700));
            if control.current_pid() == Some(pid) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM);
            }
            thread::sleep(Duration::from_millis(700));
            if control.current_pid() == Some(pid) {
                let _ = killpg(Pid::from_raw(pid), Signal::SIGKILL);
            }
        });
    }

    fn current_pid(&self) -> Option<i32> {
        self.current.lock().ok().and_then(|guard| *guard)
    }

    fn track(&self, pid: u32) {
        if let Ok(mut current) = self.current.lock() {
            *current = Some(pid as i32);
        }
    }

    fn clear(&self, pid: u32) {
        if let Ok(mut current) = self.current.lock()
            && *current == Some(pid as i32)
        {
            *current = None;
        }
    }
}

/// Start the pass on its own thread so the frame keeps running.
pub fn start(
    scope: Scope,
    keep: usize,
    tx: Sender<RuntimeEvent>,
    tools: ToolPaths,
    control: CleanupControl,
) {
    thread::spawn(move || run(scope, tools, tx, control, keep));
}

/// Where a cleanup line goes: the frame's log, or stdout when there is no frame.
type Report = Arc<dyn Fn(&str) + Send + Sync>;

pub fn run(
    scope: Scope,
    tools: ToolPaths,
    tx: Sender<RuntimeEvent>,
    control: CleanupControl,
    keep: usize,
) {
    let sender = tx.clone();
    let report: Report = Arc::new(move |line: &str| {
        let _ = sender.send(RuntimeEvent::Output(line.to_owned()));
    });
    let result = execute(scope, &tools, &report, &control, keep);

    if let Err(error) = &result
        && !control.is_cancelled()
    {
        report(&format!("error: cleanup failed: {error}"));
    }
    let _ = tx.send(RuntimeEvent::CleanupDone(result.is_ok()));
}

/// The same pass with no frame: a redirected run still gets the cleanup `--yes` asked for.
pub fn run_plain(scope: Scope, keep: usize, tools: &ToolPaths) -> anyhow::Result<()> {
    let report: Report = Arc::new(|line: &str| {
        let _ = crate::common::terminal::write_stdout(&format!("{line}\n"));
    });
    execute(scope, tools, &report, &CleanupControl::default(), keep)
}

fn execute(
    scope: Scope,
    tools: &ToolPaths,
    report: &Report,
    control: &CleanupControl,
    keep: usize,
) -> anyhow::Result<()> {
    match scope {
        Scope::Home => prune_home(tools, report, control, keep),
        Scope::System => prune_system(tools, report, control, keep),
        // An update activates both profiles, so its cleanup prunes both.
        Scope::Both => prune_home(tools, report, control, keep)
            .and_then(|_| prune_system(tools, report, control, keep)),
    }
    .and_then(|_| collect_garbage(tools, report, control))
}

fn prune_home(
    tools: &ToolPaths,
    report: &Report,
    control: &CleanupControl,
    keep: usize,
) -> anyhow::Result<()> {
    let mut list = Command::new(&tools.home_manager);
    list.arg("generations");
    let output = run_output(&mut list, control)?;
    if !output.status.success() {
        anyhow::bail!("could not list Home Manager generations{}", why(&output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ids: Vec<String> = stdout
        .lines()
        .filter_map(|line| {
            let words: Vec<_> = line.split_whitespace().collect();
            words
                .windows(2)
                .find(|pair| pair[0] == "id")
                .map(|pair| pair[1].to_owned())
        })
        .collect();
    let doomed: Vec<_> = ids.into_iter().skip(keep).collect();
    if doomed.is_empty() {
        report("cleanup: no Home Manager generations to prune");
        return Ok(());
    }
    report(&format!(
        "cleanup: removing generations {}",
        doomed.join(", ")
    ));
    let mut remove = Command::new(&tools.home_manager);
    remove.arg("remove-generations").args(&doomed);
    let output = run_streaming(&mut remove, control, report)?;
    if !output.status.success() {
        anyhow::bail!("Home Manager generation pruning failed{}", why(&output));
    }
    Ok(())
}

fn prune_system(
    tools: &ToolPaths,
    report: &Report,
    control: &CleanupControl,
    keep: usize,
) -> anyhow::Result<()> {
    let root = geteuid().is_root();
    let mut list = command_for(&profile_argv(tools, root));
    list.args([
        "--list-generations",
        "--profile",
        "/nix/var/nix/profiles/system",
    ]);
    let output = run_output(&mut list, control)?;
    if !output.status.success() {
        anyhow::bail!("could not list NixOS generations{}", why(&output));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let ids: Vec<u64> = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().next()?.parse().ok())
        .collect();
    let current = fs::canonicalize("/run/current-system").ok();
    let mut doomed = Vec::new();
    for id in ids.iter().take(ids.len().saturating_sub(keep)) {
        let generation = format!("/nix/var/nix/profiles/system-{id}-link");
        if fs::canonicalize(&generation).ok() != current {
            doomed.push(id.to_string());
        }
    }
    if doomed.is_empty() {
        report("cleanup: no NixOS generations to prune");
        return Ok(());
    }
    report(&format!(
        "cleanup: removing generations {}",
        doomed.join(", ")
    ));
    let mut remove = command_for(&profile_argv(tools, root));
    remove
        .args([
            "--delete-generations",
            "--profile",
            "/nix/var/nix/profiles/system",
        ])
        .args(&doomed);
    let output = run_streaming(&mut remove, control, report)?;
    if !output.status.success() {
        anyhow::bail!("NixOS generation pruning failed{}", why(&output));
    }
    Ok(())
}

fn collect_garbage(
    tools: &ToolPaths,
    report: &Report,
    control: &CleanupControl,
) -> anyhow::Result<()> {
    report("cleanup: collecting unreachable store paths");
    let mut collect = Command::new(&tools.nix_collect_garbage);
    let output = run_streaming(&mut collect, control, report)?;
    if !output.status.success() {
        anyhow::bail!("garbage collection failed{}", why(&output));
    }
    Ok(())
}

/// The system profile belongs to root, and an update keeps its frame as the user:
/// those commands go through the sudo ticket the run already holds. `-n` matters,
/// because the frame cannot answer a password prompt.
pub(crate) fn profile_argv(tools: &ToolPaths, root: bool) -> Vec<String> {
    if root {
        return vec![tools.nix_env.clone()];
    }
    let mut argv = vec!["sudo".to_owned(), "-n".to_owned(), "env".to_owned()];
    for variable in FORWARDED_TOOLS {
        if let Some(value) = tools.path(variable) {
            argv.push(format!("{variable}={value}"));
        }
    }
    argv.push(tools.nix_env.clone());
    argv
}

/// A command from its argv, so the elevation is built where a test can read it.
fn command_for(argv: &[String]) -> Command {
    let mut command = Command::new(&argv[0]);
    command.args(&argv[1..]);
    command
}

/// The first line of a helper's stderr, so a permission denial is not reported as a mystery.
fn why(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .next()
        .map(|line| format!(": {line}"))
        .unwrap_or_default()
}

fn run_output(command: &mut Command, control: &CleanupControl) -> anyhow::Result<Output> {
    spawn_streaming(command, control, None)
}

/// Run a helper and forward output live: buffering froze the Logs for minutes
/// and dumped the transcript out of order. Bytes are still kept whole.
fn run_streaming(
    command: &mut Command,
    control: &CleanupControl,
    report: &Report,
) -> anyhow::Result<Output> {
    spawn_streaming(command, control, Some(report))
}

fn spawn_streaming(
    command: &mut Command,
    control: &CleanupControl,
    report: Option<&Report>,
) -> anyhow::Result<Output> {
    if control.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    let mut child = command.spawn()?;
    let pid = child.id();
    control.track(pid);

    let readers = [
        child
            .stdout
            .take()
            .map(|stream| forward(stream, report.cloned())),
        child
            .stderr
            .take()
            .map(|stream| forward(stream, report.cloned())),
    ];
    let status = child.wait();
    control.clear(pid);

    let mut collected = readers.into_iter().map(join_reader);
    let (stdout, stderr) = (
        collected.next().unwrap_or_default(),
        collected.next().unwrap_or_default(),
    );

    if control.is_cancelled() {
        anyhow::bail!("cancelled");
    }
    Ok(Output {
        status: status?,
        stdout,
        stderr,
    })
}

/// Forward a child stream line by line, keeping every byte.
fn forward<R: io::Read + Send + 'static>(
    stream: R,
    report: Option<Report>,
) -> thread::JoinHandle<Vec<u8>> {
    thread::spawn(move || {
        let mut collected = Vec::new();
        let mut reader = io::BufReader::new(stream);
        let mut line = Vec::new();
        while let Ok(read) = reader.read_until(b'\n', &mut line) {
            if read == 0 {
                break;
            }
            if let Some(report) = &report {
                let text = String::from_utf8_lossy(&line);
                let text = text.trim_end_matches(['\n', '\r']);
                if !text.is_empty() {
                    report(text);
                }
            }
            collected.extend_from_slice(&line);
            line.clear();
        }
        collected
    })
}

fn join_reader(reader: Option<thread::JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader
        .map(|handle| handle.join().unwrap_or_default())
        .unwrap_or_default()
}
