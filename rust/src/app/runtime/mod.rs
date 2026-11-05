//! The frame's entry points and the terminal plumbing they share.
//! The loop itself is in `frame`, the effects it carries out in `effects`, the key and mouse
//! mapping in `input`.

use std::fs::{self, File};
use std::io::{self, IsTerminal};
use std::path::Path;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Context;
use crossbeam_channel::{Receiver, Sender, unbounded};
use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton,
    MouseEvent, MouseEventKind,
};
use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Terminal, TerminalOptions, Viewport};

use crate::app::dashboard;
use crate::app::dashboard::{PanelId, Panels};
use crate::app::model::{App, Overlay};
use crate::app::reduce::reduce;
use crate::common::actions::{Action, Command, Effect};
use crate::common::hit_targets::{
    Click, Hover, ScrollbarDrag, ScrollbarGeometry, ScrollbarTarget, area_contains,
};
use crate::common::phase::Phase;
use crate::common::progress::StepProgress;
use crate::common::tool_paths::ToolPaths;
use crate::modules::panels::activity;
use crate::modules::panels::help;
use crate::modules::panels::logs;
use crate::modules::panels::settings;
use crate::modules::panels::status;
use crate::modules::runs::records::{self, Recording};
use crate::modules::work::cleanup;
use crate::modules::work::update::{
    CredentialKeepAlive, Step, Steps, advance_step, run_plain_steps, spawn_child,
};

use crate::common::terminal::*;

mod effects;
mod frame;
mod input;

use effects::apply;
pub(crate) use effects::{ask_remove_runs, remove_runs, run_effect, start_cleanup};
use frame::run_terminal;
use input::dispatch_key;
pub use input::{handle_key, handle_mouse_command};

pub fn run(app: &mut App, command: &[String], tools: ToolPaths) -> anyhow::Result<i32> {
    // A single-scope rebuild is one step; the update target builds its own sequence and calls `run_steps`.
    let step = Step {
        label: app.status.action.clone(),
        scope: app.status.scope,
        action: app.status.action.clone(),
        command: command.to_vec(),
        dir: None,
        elevated: None,
    };
    run_steps(app, vec![step], tools)
}

/// The loop owns the child so the next step starts when the current one exits
/// cleanly; the first failure stops the run.
pub fn run_steps(app: &mut App, steps: Vec<Step>, tools: ToolPaths) -> anyhow::Result<i32> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        let status = run_plain_steps(&steps, &tools)?;
        if status != 0 || app.cleanup.policy.is_disabled() {
            return Ok(status);
        }
        // A redirect gets the cleanup it asked for; `ask` has no one to ask.
        if app.cleanup.policy.runs_unattended() {
            let _ = write_stdout("cleanup: pruning generations and collecting garbage\n");
            cleanup::run_plain(app.cleanup.scope, app.config.keep_generations, &tools)?;
        } else {
            let _ = write_stdout(
                "cleanup skipped: no terminal to ask; pass --yes to prune and collect\n",
            );
        }
        return Ok(0);
    }

    // Keep the elevated step's credential valid: sudo's ticket lasts five minutes and earlier steps can outlast it.
    let _keep_alive = CredentialKeepAlive::for_steps(&steps);
    let log_path = event_log_path();
    File::create(&log_path).context("create Nix event log")?;
    let (tx, rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    spawn_input(tx.clone(), Arc::clone(&stop));
    spawn_signals(tx.clone());
    spawn_log_tail(log_path.clone(), tx.clone(), Arc::clone(&stop));

    // Recorded as it happens so the run can be reopened; the frame needs the id as soon as it exists.
    let mut session = Recording::start(
        app.status.scope,
        &app.status.flake,
        &app.flow,
        app.cleanup.policy,
        app.kept_lock.as_deref(),
    );
    app.run_id = session.as_ref().map(|session| session.id().to_owned());

    let mut queue = Steps::new(steps);
    let first = queue.start_next().context("a run needs a step")?;
    show_step(app, &first, queue.progress());
    let mut child = Some(spawn_child(&first, &tools, &log_path, &tx)?);
    app.status.phase = Phase::Evaluating;
    app.status.message = "evaluating the flake".into();

    let screen = ScreenGuard::enter()?;
    let result = run_terminal(
        app,
        child.take(),
        &rx,
        &tx,
        tools,
        &mut session,
        &mut queue,
        &log_path,
    );
    stop.store(true, Ordering::Relaxed);
    let _ = fs::remove_file(log_path);
    // Hand the screen back to the shell before anything is written to it, then leave on
    // it what the leave setting asks for.
    drop(screen);
    let left = leave_frame(app);
    // Compress after the frame is gone, so a big log never freezes the last screen.
    if let Some(session) = session.take() {
        session.seal();
    }
    let code = result?;
    left?;
    Ok(code)
}

/// What the shell's own screen keeps once the frame's screen is gone: the closing line,
/// the whole frame, or nothing.
fn leave_frame(app: &mut App) -> anyhow::Result<()> {
    match records::leave_plan(app.config.leave, app.reopened) {
        records::Leave::Frame => replay_frame(app),
        records::Leave::CleanMessage => match app.run_id.as_deref() {
            Some(id) => Ok(records::print_closing_line(id)?),
            None => Ok(()),
        },
        records::Leave::CleanSilent => Ok(()),
    }
}

/// Draw the last frame again on the shell's screen, one row short so its prompt has room
/// under it: the frame itself lived on the alternate screen and went with it.
fn replay_frame(app: &mut App) -> anyhow::Result<()> {
    let (width, rows) = crossterm::terminal::size().unwrap_or((100, 30));
    let area = replay_area(width, rows);
    let height = area.height;
    let options = TerminalOptions {
        viewport: Viewport::Fixed(area),
    };
    let mut terminal = Terminal::with_options(CrosstermBackend::new(io::stdout()), options)
        .context("create terminal")?;
    // Blank the area first: a frame paints only the cells it draws, so the shell's own
    // text would show through every blank cell of the cards.
    terminal.clear()?;
    app.closing = true;
    terminal.draw(|frame| {
        dashboard::render(frame, app);
    })?;
    // Leave the cursor on the frame's last row: the shell ends the line itself, and that
    // newline lands on the row below instead of scrolling the frame's top row away.
    terminal.set_cursor_position((0, height - 1))?;
    terminal.show_cursor()?;
    Ok(())
}

/// The `settings` verb: the Settings dialog in a frame of its own, with nothing to run.
pub fn run_settings(app: &mut App, tools: ToolPaths) -> anyhow::Result<i32> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        // No frame to drive, so print the file the cards read.
        write_stdout(&app.config.render())?;
        return Ok(0);
    }

    let screen = ScreenGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    let (tx, rx) = unbounded();
    let mut control = cleanup::CleanupControl::default();
    // Set while a pass runs: the frame closes once it is done, never over its own work.
    let mut leave_when_done = false;
    // Set when the frame's only reason to stay (the dialog) has been dismissed.
    let mut leave = false;
    let mut effects = Vec::new();
    let mut panels = Panels::default();
    // The card's read-only figures are a measurement, so they are taken again while it is up.
    let mut last_records = Instant::now();
    app.refresh_records();
    'frame: loop {
        if last_records.elapsed() >= RECORDS_REFRESH {
            app.refresh_records();
            last_records = Instant::now();
        }
        terminal.draw(|frame| panels = dashboard::render(frame, app))?;
        // A pass started from the card reports here; the phase is the only record that it runs.
        while let Ok(event) = rx.try_recv() {
            match event {
                RuntimeEvent::Output(line) => effects.extend(reduce(app, Action::Output(line))),
                RuntimeEvent::CleanupDone(_) => {
                    // The card started it, so the frame is not a run: it goes back to waiting.
                    app.status.phase = Phase::Preparing;
                    if leave_when_done {
                        break 'frame;
                    }
                }
                _ => {}
            }
        }
        if !event::poll(REDRAW_INTERVAL)? {
            for effect in effects.drain(..) {
                settings_effect(app, effect, &tx, &tools, &mut control);
            }
            continue;
        }
        match event::read()? {
            CrosstermEvent::Key(key) => {
                // Ctrl+C stops the pass this frame started, then closes the frame.
                if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                    if app.status.phase == Phase::Cleanup {
                        control.cancel();
                        leave_when_done = true;
                    } else {
                        leave = true;
                    }
                } else {
                    leave = settings_key(app, key, &mut effects);
                }
            }
            CrosstermEvent::Mouse(mouse) => {
                let covered = app.overlay != Overlay::None;
                if let Some(command) = handle_mouse_command(app, mouse, &panels) {
                    effects.extend(reduce(app, Action::Command(command)));
                }
                // Clicking the dialog shut is the same goodbye as the escape key.
                leave = covered && app.overlay == Overlay::None;
            }
            _ => {}
        }
        for effect in effects.drain(..) {
            settings_effect(app, effect, &tx, &tools, &mut control);
        }
        if leave {
            leave = false;
            // A pass started from the dialog still owns the frame until it is done.
            if app.status.phase != Phase::Cleanup {
                break 'frame;
            }
            leave_when_done = true;
        }
    }

    drop(screen);
    leave_frame(app)?;
    Ok(0)
}

/// The effects this frame carries out. A card's button acts here: the file is written, the
/// journal is cut, and a cleanup runs on its own thread because the frame has nothing to run.
fn settings_effect(
    app: &mut App,
    effect: Effect,
    tx: &Sender<RuntimeEvent>,
    tools: &ToolPaths,
    control: &mut cleanup::CleanupControl,
) {
    match effect {
        Effect::SaveSettings => app.settings.config.save(),
        Effect::RemoveRuns => remove_runs(app),
        Effect::AskRemoveRuns => ask_remove_runs(app),
        Effect::StartCleanup => start_cleanup(app, tx, tools, control),
        _ => {}
    }
}

/// Esc, q and the card's own key leave this frame; every other key belongs to the card.
/// A dialog owns the keys while it is up, Esc included, so it closes instead of leaving.
fn settings_key(app: &mut App, key: KeyEvent, effects: &mut Vec<Effect>) -> bool {
    if app.overlay != Overlay::None {
        if let Some(command) = handle_key(app, key) {
            effects.extend(reduce(app, Action::Command(command)));
        }
        // The dialog is this frame's whole reason to exist, so dismissing it is the way out.
        return app.overlay == Overlay::None;
    }
    if matches!(
        key.code,
        KeyCode::Esc | KeyCode::Char('q' | 'Q' | 's' | 'S')
    ) {
        return true;
    }
    if let Some(command) = handle_key(app, key) {
        effects.extend(reduce(app, Action::Command(command)));
    }
    false
}

/// Open a finished run for reading: the live frame with no child, so nothing is executed.
pub fn run_replay(app: &mut App) -> anyhow::Result<i32> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        anyhow::bail!(
            "a recorded run needs a terminal to be shown; `nix-rebuild runs <ID> --logs` \
             prints one instead"
        );
    }
    let (tx, rx) = unbounded();
    let stop = Arc::new(AtomicBool::new(false));
    spawn_input(tx.clone(), Arc::clone(&stop));
    spawn_signals(tx.clone());

    // A recording being read back: this decides whether the closing line is worth leaving behind.
    app.reopened = true;
    let screen = ScreenGuard::enter()?;
    let mut session = None;
    let log_path = event_log_path();
    let mut queue = Steps::new(Vec::new());
    let result = run_terminal(
        app,
        None,
        &rx,
        &tx,
        ToolPaths::from_env(),
        &mut session,
        &mut queue,
        &log_path,
    );
    stop.store(true, Ordering::Relaxed);
    // Hand the screen back to the shell before anything is written to it.
    drop(screen);
    let left = leave_frame(app);
    let code = result?;
    left?;
    Ok(code)
}

/// The area a replayed frame gets: the top of the screen, one row short so the prompt
/// that follows has a line of its own.
fn replay_area(width: u16, rows: u16) -> Rect {
    Rect::new(0, 0, width, rows.saturating_sub(1).max(1))
}

/// What the journal writes about this run; the recorder never reads the run's own state.
fn session_outcome(app: &App) -> records::Outcome {
    records::Outcome {
        scope: app.status.scope,
        action: app.flow.clone(),
        exit_code: app.status.exit_code,
        status: app.status.message.clone(),
    }
}

/// Point the Run card at the step that just started; the queue owns the progress it reports.
fn show_step(app: &mut App, step: &Step, progress: Option<StepProgress>) {
    app.status.scope = step.scope;
    app.status.action = step.action.clone();
    app.status.step = progress;
}

/// How often the Settings dialog's journal figures are measured again while it is up.
const RECORDS_REFRESH: Duration = Duration::from_secs(2);

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::app::model::Overlay;
    use crate::app::testing::app;

    #[test]
    fn closing_the_settings_dialog_is_the_way_out() {
        // Keep the file out of the way: a change writes it, and a test must not touch the real one.
        temp_env::with_var(
            "NIX_REBUILD_CONFIG",
            Some("/nonexistent/nix-rebuild/config.toml"),
            || {
                let mut app = app();
                app.overlay = Overlay::Settings;
                let mut effects = Vec::new();

                assert!(
                    !super::settings_key(
                        &mut app,
                        KeyEvent::new(KeyCode::Down, KeyModifiers::NONE),
                        &mut effects
                    ),
                    "an arrow belongs to the dialog"
                );
                assert_eq!(app.settings.row, 1, "the arrow moved the selection");
                assert!(
                    super::settings_key(
                        &mut app,
                        KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                        &mut effects
                    ),
                    "escape closes the box, and the box was the whole frame"
                );
                assert_eq!(app.overlay, Overlay::None, "the dialog took that escape");
            },
        );
    }

    #[test]
    fn a_question_asked_by_the_dialog_keeps_the_frame_open() {
        // A filter is answered and dismissed, and the settings it was opened from come back.
        let mut app = app();
        app.overlay = Overlay::Settings;
        // The dialog opens the filter the way its row does, so the filter knows where to return.
        app.open_overlay(Overlay::LogFilter);
        let mut effects = Vec::new();

        assert!(
            !super::settings_key(
                &mut app,
                KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE),
                &mut effects
            ),
            "the frame waits for the dialog underneath"
        );
        assert_eq!(
            app.overlay,
            Overlay::Settings,
            "the filter gave the box back"
        );
    }

    #[test]
    fn a_replayed_frame_keeps_the_bottom_row_for_the_prompt() {
        let area = super::replay_area(100, 28);
        assert_eq!(
            area,
            ratatui::layout::Rect::new(0, 0, 100, 27),
            "the frame stops one row short of the pane",
        );
        // The cursor is left on the frame's last row and the shell's own newline takes
        // the bottom one; a frame that ended on the bottom row would scroll its top away.
        assert_eq!(area.bottom(), 27, "one row is left under the frame");
    }

    #[test]
    fn the_confirmed_answer_cuts_the_journal_and_keeps_the_live_run() {
        let dir = std::path::PathBuf::from(
            std::env::var("CARGO_TARGET_TMPDIR").unwrap_or_else(|_| "/tmp".into()),
        )
        .join(format!("remove-runs-runtime-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        temp_env::with_var("XDG_STATE_HOME", Some(&dir), || {
            let journal = dir.join("nix-rebuild/runs");
            for name in ["old-a", "old-b", "live"] {
                std::fs::create_dir_all(journal.join(name)).expect("a recorded run");
            }
            let mut app = app();
            app.run_id = Some("live".into());
            app.overlay = Overlay::RemoveRunsPrompt;

            super::remove_runs(&mut app);

            assert_eq!(
                crate::modules::runs::records::count(),
                1,
                "the run being recorded is kept; every other one is gone"
            );
            assert!(
                journal.join("live").is_dir(),
                "and it is the one the frame is writing"
            );
            assert_eq!(app.overlay, Overlay::None, "and the question is answered");
            assert_eq!(
                (app.settings.recorded_runs, app.settings.recorded_bytes),
                (1, 0),
                "and the figures under the button are measured again"
            );
            let _ = std::fs::remove_dir_all(&dir);
        });
    }
}
