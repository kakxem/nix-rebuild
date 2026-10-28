//! Keys and the pointer, translated into the commands the reducer applies.

use super::*;

pub(crate) fn dispatch_key(app: &mut App, key: KeyEvent, pending: &mut Vec<Command>) {
    // `handle_key` may update its own presentation state (help search, filter query) before naming a command.
    if let Some(command) = handle_key(app, key) {
        pending.push(command);
    }
}

pub(crate) fn scrollbar_command(target: ScrollbarTarget, offset: usize) -> Command {
    match target {
        ScrollbarTarget::Logs => logs::Command::SetLogScroll(offset).into(),
        ScrollbarTarget::Activity => activity::Command::SetActivityScroll(offset).into(),
        ScrollbarTarget::Settings => settings::Command::SetScroll(offset).into(),
    }
}

/// Where a bar's pointer sits in its own list: the logs and the activity read from the
/// newest line up, the settings card from its first line down.
pub(crate) fn scrollbar_offset(
    target: ScrollbarTarget,
    geometry: ScrollbarGeometry,
    row: u16,
    grab_offset: u16,
) -> usize {
    match target {
        ScrollbarTarget::Settings => geometry.top_offset_at(row, grab_offset),
        ScrollbarTarget::Logs | ScrollbarTarget::Activity => {
            geometry.scroll_offset_at(row, grab_offset)
        }
    }
}

pub(crate) fn scrollbar_under_pointer(
    app: &App,
    column: u16,
    row: u16,
) -> Option<(ScrollbarTarget, ScrollbarGeometry)> {
    [
        (ScrollbarTarget::Logs, app.logs.scrollbar),
        (ScrollbarTarget::Activity, app.activity.scrollbar),
        (ScrollbarTarget::Settings, app.settings.scrollbar),
    ]
    .into_iter()
    .find_map(|(target, geometry)| {
        geometry
            .filter(|geometry| geometry.contains(column, row))
            .map(|geometry| (target, geometry))
    })
}

/// Route a mouse event. A dialog in front owns the pointer: a press inside it goes to the dialog,
/// a press on the dimmed backdrop dismisses it, and nothing behind it answers while it is up. With
/// no dialog, the wheel scrolls whichever panel is under the pointer; `layout` is the geometry of
/// the last rendered frame.
pub fn handle_mouse_command(app: &mut App, mouse: MouseEvent, panels: &Panels) -> Option<Command> {
    if matches!(mouse.kind, MouseEventKind::Up(MouseButton::Left)) {
        app.scrollbar_drag = None;
        app.resizing = None;
        return None;
    }

    // A bar the pointer grabbed follows it, a dialog's bar included: this is the only path a
    // drag takes, so it sits above the gate that hands everything else to the dialog.
    if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left))
        && let Some(drag) = app.scrollbar_drag
    {
        let geometry = match drag.target {
            ScrollbarTarget::Logs => app.logs.scrollbar,
            ScrollbarTarget::Activity => app.activity.scrollbar,
            ScrollbarTarget::Settings => app.settings.scrollbar,
        };
        if let Some(geometry) = geometry {
            return Some(scrollbar_command(
                drag.target,
                scrollbar_offset(drag.target, geometry, mouse.row, drag.grab_offset),
            ));
        }
    }

    // A dialog in front owns the pointer, whatever the last frame measured. The box it drew only
    // says which cells are the backdrop; before a frame of its own, all of them are.
    if app.overlay != Overlay::None {
        let dialog_box = app
            .clicks
            .dialog()
            .map(|dialog| (dialog.area, dialog.dismiss));
        let inside = match dialog_box {
            Some((area, _)) => area_contains(area, mouse.column, mouse.row),
            None => true,
        };
        // Hover reads only inside the dialog, so nothing behind it lights up under the pointer.
        if inside {
            app.set_hover(Hover {
                column: mouse.column,
                row: mouse.row,
            });
            return dialog_mouse(app, mouse);
        }
        app.clear_hover();
        return match (mouse.kind, dialog_box) {
            (MouseEventKind::Down(MouseButton::Left), Some((_, dismiss))) => Some(dismiss),
            // The wheel still moves the dialog's own list from anywhere on the backdrop.
            (MouseEventKind::ScrollUp | MouseEventKind::ScrollDown, _) => dialog_mouse(app, mouse),
            _ => None,
        };
    }

    app.set_hover(Hover {
        column: mouse.column,
        row: mouse.row,
    });

    if matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left)) {
        // A border drag resizes the two cards it separates; nothing else is written down.
        if let Some(resize) = app.resizing {
            resize.apply(&mut app.heights, mouse.row);
            return None;
        }
        return None;
    }

    // A left click acts like pressing the key hinted by the thing clicked.
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
        if let Some((target, geometry)) = scrollbar_under_pointer(app, mouse.column, mouse.row) {
            let grab_offset = if geometry.thumb_contains(mouse.row) {
                mouse.row.saturating_sub(geometry.thumb_y)
            } else {
                geometry.thumb_height / 2
            };
            app.scrollbar_drag = Some(ScrollbarDrag {
                target,
                grab_offset,
            });
            return Some(scrollbar_command(
                target,
                scrollbar_offset(target, geometry, mouse.row, grab_offset),
            ));
        }
        // A press on a card border grabs it, but a control drawn on that same row (the
        // Filter and copy buttons live on the top border) keeps the click.
        if app.clicks.under(mouse.column, mouse.row).is_none()
            && let Some(resize) = panels.boundary_at(mouse.column, mouse.row, app.heights)
        {
            app.resizing = Some(resize);
            return None;
        }
        // Clicking a panel body does nothing; only the wheel scrolls, so a stray click cannot move
        // the view.
        return app
            .clicks
            .under(mouse.column, mouse.row)
            .map(Click::command);
    }

    let lines = match mouse.kind {
        MouseEventKind::ScrollUp => 3,
        MouseEventKind::ScrollDown => -3,
        _ => return None,
    };

    if app.config.show_activity
        && rect_contains(panels.rect(PanelId::Activity), mouse.column, mouse.row)
    {
        Some(activity::Command::ScrollActivity(lines).into())
    } else if app.config.show_logs
        && rect_contains(panels.rect(PanelId::Logs), mouse.column, mouse.row)
    {
        Some(logs::Command::ScrollLogs(lines).into())
    } else {
        None
    }
}

/// A pointer event the dialog in front owns: the controls it drew, then the rows its own slice
/// keeps (a filter list, the help list). A cell of its box that neither knows about does nothing.
pub(crate) fn dialog_mouse(app: &mut App, mouse: MouseEvent) -> Option<Command> {
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && let Some(click) = app.clicks.under(mouse.column, mouse.row)
    {
        return Some(click.command());
    }
    // The settings dialog carries the only bar a dialog owns: a press on it grabs the bar,
    // and the drag that follows is routed before the modal gate.
    if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left))
        && app.overlay == Overlay::Settings
        && let Some(geometry) = app
            .settings
            .scrollbar
            .filter(|geometry| geometry.contains(mouse.column, mouse.row))
    {
        let grab_offset = if geometry.thumb_contains(mouse.row) {
            mouse.row.saturating_sub(geometry.thumb_y)
        } else {
            geometry.thumb_height / 2
        };
        app.scrollbar_drag = Some(ScrollbarDrag {
            target: ScrollbarTarget::Settings,
            grab_offset,
        });
        return Some(scrollbar_command(
            ScrollbarTarget::Settings,
            scrollbar_offset(ScrollbarTarget::Settings, geometry, mouse.row, grab_offset),
        ));
    }
    match app.overlay {
        Overlay::Help => help::mouse_command(&mut app.help, mouse.kind, mouse.column, mouse.row),
        Overlay::LogFilter => {
            logs::mouse_command(&mut app.logs, mouse.kind, mouse.column, mouse.row)
        }
        Overlay::ActivityFilter => {
            activity::mouse_command(&mut app.activity, mouse.kind, mouse.column, mouse.row)
        }
        Overlay::Settings => {
            settings::mouse_command(&mut app.settings, mouse.kind, mouse.column, mouse.row)
        }
        // Details, the cancel question, the journal question and the cleanup question answer
        // through the controls they drew, never through a row lookup.
        _ => None,
    }
}

pub(crate) fn rect_contains(area: Rect, column: u16, row: u16) -> bool {
    column >= area.x
        && column < area.x.saturating_add(area.width)
        && row >= area.y
        && row < area.y.saturating_add(area.height)
}

/// Translate one keystroke into the command it means. An open dialog owns every key, the cleanup
/// question included; otherwise run, logs, activity and help answer their own keys in that order.
pub fn handle_key(app: &mut App, key: KeyEvent) -> Option<Command> {
    if key.kind != KeyEventKind::Press {
        return None;
    }
    // A dialog never traps the terminal: Ctrl+C leaves from all of them.
    if app.overlay != Overlay::None && leaves_on_ctrl_c(key) {
        return Some(Command::ForceQuit);
    }
    match app.overlay {
        Overlay::ConfirmCancel => status::cancel_key(&app.status, key),
        Overlay::CleanupPrompt => cleanup::key_command(key, true),
        Overlay::Help => help::key_command(&mut app.help, key, true),
        Overlay::Details => status::key_command(&app.status, key, true),
        Overlay::LogFilter => logs::key_command(&mut app.logs, key, true),
        Overlay::RemoveRunsPrompt => records::confirm_key(key),
        Overlay::ActivityFilter => activity::key_command(&mut app.activity, key, true),
        Overlay::Settings => settings::key_command(key, true),
        Overlay::None => panel_key(app, key),
    }
}

/// The escape hatch every dialog shares; the run's own keys decide it while no dialog is up.
pub(crate) fn leaves_on_ctrl_c(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL)
}

/// What the frame's own cards answer, in the order they are asked: Settings, run, logs,
/// activity, help. The first slice to claim a key owns it.
pub(crate) fn panel_key(app: &mut App, key: KeyEvent) -> Option<Command> {
    // `G` opens the dialog from anywhere on the frame; its rows answer once it is up.
    if let Some(command) = settings::key_command(key, false) {
        return Some(command);
    }
    if let Some(command) = status::key_command(&app.status, key, false) {
        return Some(command);
    }
    if let Some(command) = logs::key_command(&mut app.logs, key, false) {
        return Some(command);
    }
    if let Some(command) = activity::key_command(&mut app.activity, key, false) {
        return Some(command);
    }
    help::key_command(&mut app.help, key, false)
}
