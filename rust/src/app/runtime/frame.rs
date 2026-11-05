//! The frame loop: the terminal it owns, the events it reads and the child it advances.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn run_terminal(
    app: &mut App,
    mut child: Option<Child>,
    rx: &Receiver<RuntimeEvent>,
    tx: &Sender<RuntimeEvent>,
    tools: ToolPaths,
    session: &mut Option<Recording>,
    steps: &mut Steps,
    log_path: &Path,
) -> anyhow::Result<i32> {
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    terminal.clear()?;
    terminal.hide_cursor()?;

    let mut child_finished = child.is_none();
    let mut cleanup_started = false;
    let mut cleanup_control = cleanup::CleanupControl::default();
    let mut last_draw = Instant::now() - REDRAW_INTERVAL;
    let mut panels = Panels::default();
    let mut select = SelectGuard::default();
    let mut pending: Vec<Command> = Vec::new();
    let mut effects: Vec<Effect> = Vec::new();
    let mut recorded: Option<(Phase, Option<bool>)> = None;
    // The Settings dialog's figures are a measurement: they are taken again while it is up.
    let mut last_records = Instant::now() - RECORDS_REFRESH;

    loop {
        let now = Instant::now();
        if app.overlay == Overlay::Settings && last_records.elapsed() >= RECORDS_REFRESH {
            app.refresh_records();
            last_records = now;
        }
        if redraw_due(last_draw, now, false) {
            terminal.draw(|frame| panels = dashboard::render(frame, app))?;
            last_draw = now;
        }
        let wait = REDRAW_INTERVAL
            .saturating_sub(last_draw.elapsed())
            .max(Duration::from_millis(1));

        match rx.recv_timeout(wait) {
            Ok(RuntimeEvent::Key(key)) => {
                // F2 releases the mouse so text can be selected; handled here so no
                // open dialog (the help list included) can swallow it.
                if is_select_toggle(&key) {
                    select.toggle();
                } else {
                    dispatch_key(app, key, &mut pending);
                }
            }
            Ok(RuntimeEvent::Mouse(mouse)) => {
                if let Some(command) = handle_mouse_command(app, mouse, &panels) {
                    pending.push(command);
                }
            }
            Ok(RuntimeEvent::Signal(_))
                if matches!(
                    app.status.phase,
                    Phase::Complete | Phase::Failed | Phase::Cancelled
                ) =>
            {
                app.should_quit = true;
            }
            Ok(RuntimeEvent::Signal(_)) => pending.push(status::Command::ConfirmCancel.into()),
            Ok(RuntimeEvent::Output(line)) => {
                if let Some(session) = session.as_mut() {
                    session.record('O', &line);
                }
                apply(app, Action::Output(line), &mut effects);
            }
            // Stderr shows with the other output and is kept as the run's last word on why a step stopped.
            Ok(RuntimeEvent::Stderr(line)) => {
                if let Some(session) = session.as_mut() {
                    session.record('O', &line);
                }
                app.status.note_stderr(&line);
                apply(app, Action::Output(line), &mut effects);
            }
            Ok(RuntimeEvent::Json(line)) => {
                if let Some(session) = session.as_mut() {
                    session.record('J', &line);
                }
                apply(app, Action::NixJson(line), &mut effects);
            }
            Ok(RuntimeEvent::CleanupDone(success)) => {
                apply(app, Action::CleanupExited { success }, &mut effects);
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }

        for command in pending.drain(..) {
            apply(app, Action::Command(command), &mut effects);
        }
        for effect in effects.drain(..) {
            run_effect(
                app,
                effect,
                &mut cleanup_started,
                &mut cleanup_control,
                tx,
                &tools,
            );
        }

        if app.status.cancel_requested && !child_finished {
            if let Some(child) = child.as_mut() {
                let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGTERM);
                thread::sleep(Duration::from_millis(150));
                if child.try_wait()?.is_none() {
                    let _ = killpg(Pid::from_raw(child.id() as i32), Signal::SIGKILL);
                }
                let _ = child.wait();
            }
            child_finished = true;
        }
        // Settle on the confirmation, not on the kill: a second answer, or a child that was
        // already gone, otherwise leaves `cancelling` on screen with no key that leaves it.
        if app.status.cancel_requested && app.status.phase == Phase::Cancelling {
            if cleanup_started {
                cleanup_control.cancel();
            }
            app.settle_cancel(cleanup_started);
        }

        if !child_finished
            && let Some(active) = child.as_mut()
            && let Some(status) = active.try_wait()?
        {
            let failed = !status.success();
            // A clean step with another behind it continues the run; only the step and Run card change.
            let next = advance_step(steps, failed, &tools, log_path, tx)?;
            match next {
                Some((step, started)) => {
                    show_step(app, &step, steps.progress());
                    child = Some(started);
                    app.status.phase = Phase::Evaluating;
                }
                None => {
                    child_finished = true;
                    app.finish_after_exit(failed);
                    app.status.finish_timer();
                    if failed {
                        let code = status.code().unwrap_or(1);
                        app.status.phase = Phase::Failed;
                        app.status.message =
                            format!("{} exited with {code}", steps.running_label());
                        app.status.exit_code = code;
                        app.status.note_step_failure(code);
                    } else if app.cleanup.policy.is_disabled() {
                        app.status.phase = Phase::Complete;
                        app.status.message = app.completion_status();
                    } else if app.cleanup.policy.runs_unattended() {
                        app.begin_cleanup_phase();
                        cleanup_started = true;
                    } else {
                        app.request_cleanup_decision();
                    }
                }
            }
        }

        if app.status.phase == Phase::Cleanup && !cleanup_started {
            start_cleanup(app, tx, &tools, &cleanup_control);
            cleanup_started = true;
        }

        // Write the outcome as soon as it is known, so a terminal killed before the frame
        // closes still leaves a record; the cleanup result updates it if the answer changes.
        let outcome = cleanup_started.then_some(matches!(app.status.phase, Phase::Complete));
        let settled = matches!(
            app.status.phase,
            Phase::Complete | Phase::Failed | Phase::Cancelled
        );
        if settled && recorded != Some((app.status.phase, outcome)) {
            if let Some(session) = session.as_mut() {
                session.finish(session_outcome(app), outcome);
            }
            recorded = Some((app.status.phase, outcome));
        }

        if app.should_quit {
            // The frame goes with its screen; `leave = frame` redraws this one on the
            // shell's screen afterwards, closing line included.
            app.closing =
                records::leave_plan(app.config.leave, app.reopened) == records::Leave::Frame;
            break;
        }
    }

    let cursor_row = terminal.get_frame().area().bottom().saturating_sub(1);
    terminal.set_cursor_position((0, cursor_row))?;
    terminal.show_cursor()?;

    if let Some(session) = session.as_mut() {
        let cleanup_ok = cleanup_started.then_some(matches!(app.status.phase, Phase::Complete));
        session.finish(session_outcome(app), cleanup_ok);
    }
    // The journal keeps what the settings ask for; the record above is the newest, so it stays.
    if app.settings.config.prune_logs {
        records::prune(app.settings.config.keep_logs);
    }
    Ok(app.status.exit_code)
}
