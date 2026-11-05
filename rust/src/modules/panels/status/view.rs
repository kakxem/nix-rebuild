//! The Run card, the failure banner that takes the Activity card over, and the cancel dialog.
//! View only: every fact comes in as parameters.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::common::actions::Command;
use crate::common::dialog::{self, Answers, Dialog};
use crate::common::hit_targets::{Click, ClickMap, Hover};
use crate::common::layout::centered_rect;
use crate::common::phase::Phase;
use crate::common::text::{clip_line, format_clock};
use crate::common::theme::{CYAN, GREEN, MUTED, PURPLE, RED, YELLOW};
use crate::common::ui::{ControlSpec, line_width, panel_block, panel_label};
use crate::modules::panels::status;

pub fn footer_control(show_status: bool) -> ControlSpec {
    ControlSpec {
        command: super::Command::ToggleStatus.into(),
        prefix: "",
        key: "S",
        icon: Some('\u{f04b}'),
        label: "tatus",
        active: show_status,
        enabled: true,
    }
}

/// The banner's rows: how the run ended, the step's last word, and the two ways out.
pub const FAILURE_BANNER_ROWS: u16 = 3;

/// The gap between the card's three columns: what it builds, what it does, how long it took.
const COLUMN_GAP: u16 = 3;
/// Cells the flake keeps before the step may take the centre: a cut name reads worse than a cut label.
const FLAKE_MIN: u16 = 12;
/// How the tree is announced beside the flake, as a suffix of the left column.
const DIRTY_SUFFIX: &str = " · dirty";

pub fn render_summary(
    frame: &mut Frame<'_>,
    run: &status::State,
    cleanup_freed: Option<&(u64, String)>,
    cleanup_succeeded: Option<bool>,
    area: Rect,
) {
    let block = panel_block(
        panel_label("Status"),
        MUTED,
        Some(status_badges(run, cleanup_freed, cleanup_succeeded).right_aligned()),
    );
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let (clock, clock_width) = clock_column(run, inner.width);
    let clock_x = inner.width.saturating_sub(clock_width + 1);
    // Three zones: the checkout flush left, the step on the card's centre, the clock flush
    // right. The step is what changes, so it is the one that keeps the centre.
    let room = clock_x.saturating_sub(COLUMN_GAP);
    let middle = middle_line(run);
    let natural = line_width(&middle);
    let centred = room.saturating_mul(2).saturating_sub(inner.width);
    let beside_flake = inner.width.saturating_sub(2 * (FLAKE_MIN + COLUMN_GAP + 1));
    // A centred middle spends the same cells on both sides, so it either fits beside the
    // flake or takes the whole middle: the flake gives up its cells before the step does.
    let limit = if natural <= beside_flake {
        beside_flake
    } else {
        centred
    };
    let middle_width = natural.min(limit);
    let middle_x = inner.width.saturating_sub(middle_width) / 2;
    let left_width = middle_x.saturating_sub(COLUMN_GAP + 1);

    frame.render_widget(
        Paragraph::new(clip_line(left_line(run, left_width), left_width)),
        Rect::new(inner.x.saturating_add(1), inner.y, left_width, 1),
    );
    frame.render_widget(
        Paragraph::new(clip_line(middle, room.saturating_sub(middle_x))),
        Rect::new(
            inner.x.saturating_add(middle_x),
            inner.y,
            room.saturating_sub(middle_x),
            1,
        ),
    );
    frame.render_widget(
        Paragraph::new(clock),
        Rect::new(inner.x.saturating_add(clock_x), inner.y, clock_width, 1),
    );
}

/// The clock keeps its reading at any width; only its glyph and the air around it go.
fn clock_column(run: &status::State, width: u16) -> (Line<'static>, u16) {
    let value = format_clock(run.elapsed());
    let reading = UnicodeWidthStr::width(value.as_str()) as u16;
    if reading + 3 <= width {
        return (
            Line::from(vec![
                Span::styled("\u{f017} ", Style::default().fg(MUTED)),
                Span::raw(value),
            ]),
            reading + 2,
        );
    }
    (Line::from(Span::raw(value)), reading)
}

/// What the run does: the scope, the action, and the step of a sequence.
fn middle_line(run: &status::State) -> Line<'static> {
    let mut spans = vec![
        Span::styled("\u{f04b} ", Style::default().fg(MUTED)),
        Span::styled(run.scope.label(), Style::default().fg(CYAN)),
        Span::styled(" · ", Style::default().fg(MUTED)),
        Span::raw(run.action.clone()),
    ];
    if let Some(step) = &run.step {
        spans.push(Span::styled(" · ", Style::default().fg(MUTED)));
        spans.push(Span::styled(
            format!("{}/{} ", step.index, step.total),
            Style::default().fg(CYAN),
        ));
        spans.push(Span::raw(step.label.clone()));
    }
    Line::from(spans)
}

/// What the run builds: the checkout it came from, and the tree it read. The path shortens
/// to its last directory and then to the attribute, so `#sample` survives a narrow card.
fn left_line(run: &status::State, width: u16) -> Line<'static> {
    let (flake, dirty) = left_column(&run.flake, run.dirty_tree, width);
    let mut spans = vec![
        Span::styled("\u{f126} ", Style::default().fg(MUTED)),
        Span::styled(flake, Style::default().fg(CYAN)),
    ];
    if dirty {
        spans.push(Span::styled(
            DIRTY_SUFFIX,
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        ));
    }
    Line::from(spans)
}

/// The widest spelling of the flake that fits, and whether the dirty warning still has room.
fn left_column(flake: &str, dirty_tree: bool, width: u16) -> (String, bool) {
    let candidates = flake_spellings(flake);
    let suffix = UnicodeWidthStr::width(DIRTY_SUFFIX) as u16;
    let budget = |suffix: u16| (width as usize).saturating_sub(2 + suffix as usize);
    if dirty_tree
        && let Some(flake) = candidates
            .iter()
            .find(|candidate| UnicodeWidthStr::width(candidate.as_str()) <= budget(suffix))
    {
        return (flake.clone(), true);
    }
    let flake = candidates
        .iter()
        .find(|candidate| UnicodeWidthStr::width(candidate.as_str()) <= budget(0))
        .or_else(|| candidates.last())
        .cloned()
        .unwrap_or_default();
    (flake, false)
}

/// The flake from widest to narrowest: the path, its last directory, and the attribute alone.
fn flake_spellings(flake: &str) -> Vec<String> {
    let (path, attribute) = match flake.split_once('#') {
        Some((path, attribute)) => (path, format!("#{attribute}")),
        None => (flake, String::new()),
    };
    let last = path.rsplit('/').next().unwrap_or(path);
    let mut spellings = vec![
        format!("{path}{attribute}"),
        format!("…/{last}{attribute}"),
        // The attribute names the configuration; the directory is already in the path.
        if attribute.is_empty() {
            last.to_owned()
        } else {
            attribute
        },
    ];
    spellings.dedup();
    spellings
}

fn status_badges(
    run: &status::State,
    cleanup_freed: Option<&(u64, String)>,
    cleanup_succeeded: Option<bool>,
) -> Line<'static> {
    let (badge_icon, badge_text, badge_color) = phase_badge(run);
    let mut badges = vec![Span::raw(" ")];
    // What the cleanup freed stays on the Run title once it has finished.
    if let Some((paths, freed)) = cleanup_freed {
        badges.push(Span::styled(
            format!("\u{f0e2} {paths} paths · {freed} freed"),
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ));
        badges.push(Span::styled(" · ", Style::default().fg(MUTED)));
    }
    if cleanup_succeeded == Some(false) {
        badges.push(Span::styled(
            "\u{f0e2} cleanup failed",
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        ));
        badges.push(Span::styled(" · ", Style::default().fg(MUTED)));
    }
    badges.push(Span::styled(
        format!("{badge_icon} {badge_text}"),
        Style::default()
            .fg(badge_color)
            .add_modifier(Modifier::BOLD),
    ));
    badges.push(Span::raw(" "));
    Line::from(badges)
}

/// A failed badge names the derivation that stopped the run: `failed` alone would leave
/// the culprit in the logs.
fn phase_badge(run: &status::State) -> (char, String, Color) {
    let (icon, label, color) = match run.phase {
        Phase::Complete => ('\u{f00c}', "complete", GREEN),
        Phase::Failed => ('\u{f00d}', "failed", RED),
        Phase::Cancelled => ('\u{f00d}', "cancelled", RED),
        Phase::Cleanup => ('\u{f0e2}', "cleaning", CYAN),
        Phase::Evaluating => ('\u{f002}', "evaluating", CYAN),
        Phase::Activating => ('\u{f013}', "activating", CYAN),
        Phase::Cancelling => ('\u{f00d}', "cancelling", YELLOW),
        _ => ('\u{f04b}', "running", CYAN),
    };
    let label = match (run.phase, run.first_failure()) {
        (Phase::Failed, Some(failure)) => format!("{label} · {}", failure.name),
        _ => label.to_owned(),
    };
    (icon, label, color)
}

/// The banner a failed rebuild shows in place of its Activity card. Drawn from the render
/// pass so its buttons land in the click map the panels are read back from.
pub fn render_failure_prompt(
    frame: &mut Frame<'_>,
    run: &status::State,
    copy_button: Option<(&'static str, Command)>,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    const CLOSE: &str = "[Enter  Close]";
    const GAP: &str = "   ";

    if area.width == 0 || area.height == 0 {
        return;
    }

    let (title, reason) = failure_banner_lines(run);

    let total_width = match copy_button {
        Some((copy_label, _)) => {
            UnicodeWidthStr::width(CLOSE)
                + UnicodeWidthStr::width(GAP)
                + UnicodeWidthStr::width(copy_label)
        }
        None => UnicodeWidthStr::width(CLOSE),
    };
    let start = area
        .x
        .saturating_add(area.width.saturating_sub(total_width as u16) / 2);
    // The buttons take the banner's last row; a short card drops the reason line, not the way out.
    let buttons_row = if area.height >= FAILURE_BANNER_ROWS {
        area.y.saturating_add(FAILURE_BANNER_ROWS.saturating_sub(1))
    } else {
        area.y.saturating_add(1)
    };
    let close_area = (start, buttons_row, UnicodeWidthStr::width(CLOSE) as u16, 1);
    let copy_area = copy_button.map(|(copy_label, _)| {
        (
            start
                .saturating_add(close_area.2)
                .saturating_add(UnicodeWidthStr::width(GAP) as u16),
            buttons_row,
            UnicodeWidthStr::width(copy_label) as u16,
            1,
        )
    });
    // A row the card cannot draw is a row the pointer cannot reach either.
    if buttons_row < area.y.saturating_add(area.height) {
        clicks.push((close_area, Click::Command(Command::Quit)));
        // The copy command comes in from the slice that owns the work it fires.
        if let (Some(copy_area), Some((_, command))) = (copy_area, copy_button) {
            clicks.push((copy_area, Click::Command(command)));
        }
    }

    let hovered = |target: (u16, u16, u16, u16)| {
        hover.is_some_and(|hover| {
            hover.column >= target.0
                && hover.column < target.0.saturating_add(target.2)
                && hover.row == target.1
        })
    };
    let button_style = |target, color| {
        let style = Style::default().fg(color);
        if hovered(target) {
            style.add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
        } else {
            style
        }
    };

    // One line per row, never wrapped, so buttons stay where the click map recorded them.
    let mut text = vec![clip_line(
        Line::from(Span::styled(
            title,
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        )),
        area.width,
    )];
    if area.height >= FAILURE_BANNER_ROWS {
        text.push(clip_line(
            Line::from(Span::styled(reason, Style::default().fg(MUTED))),
            area.width,
        ));
    }
    // Bold like the frame's own completion chip: this is the action, not part of the sentence.
    let mut buttons = vec![Span::styled(
        CLOSE,
        button_style(close_area, GREEN).add_modifier(Modifier::BOLD),
    )];
    if let (Some(copy_area), Some((copy_label, _))) = (copy_area, copy_button) {
        buttons.push(Span::raw(GAP));
        buttons.push(Span::styled(copy_label, button_style(copy_area, CYAN)));
    }
    text.push(Line::from(buttons));

    frame.render_widget(
        Paragraph::new(Text::from(text)).alignment(Alignment::Center),
        area,
    );
}

/// What the banner says. A named derivation points at `nix log`; otherwise the step, its
/// exit code and its last stderr line carry the reason.
fn failure_banner_lines(run: &status::State) -> (String, String) {
    let mut title = "\u{f00d} rebuild failed".to_owned();
    let reason = match run.first_failure() {
        Some(failure) => {
            title.push_str(&format!(" · {}", failure.name));
            format!("nix log {}", failure.drv)
        }
        None => {
            if let Some(step) = &run.step {
                title.push_str(&format!(" · {}/{} {}", step.index, step.total, step.label));
            }
            run.failure_reason
                .clone()
                .unwrap_or_else(|| "the Logs card has the reason".to_owned())
        }
    };
    if let Some(code) = run.step_exit_code {
        title.push_str(&format!(" · exit {code}"));
    }
    (title, reason)
}

/// The failure banner as its own dialog, for the run whose Activity card is hidden.
pub fn render_failure_dialog(
    frame: &mut Frame<'_>,
    run: &status::State,
    copy_button: Option<(&'static str, Command)>,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    // Wide enough for the `nix log` line with the derivation path in it.
    const WIDTH: u16 = 88;
    let height = dialog::height(FAILURE_BANNER_ROWS, false).min(area.height.saturating_sub(2));
    let popup = centered_rect(WIDTH, height, area);
    frame.render_widget(Clear, popup);
    // The same corner every dialog wears: it closes the run, as Esc and Enter do here.
    let block = dialog::block(clicks, hover, popup, " Failed ", RED, Command::Quit);
    let inner = dialog::body(popup);
    frame.render_widget(block, popup);
    render_failure_prompt(frame, run, copy_button, hover, clicks, inner);
}

/// The run details the compact header hides.
#[allow(clippy::too_many_arguments)]
pub fn render_details(
    frame: &mut Frame<'_>,
    run: &status::State,
    run_id: Option<&str>,
    cleanup_label: &str,
    counts: &[(&str, usize)],
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    // Six rows of the run itself, plus the row that names the tree, plus the counts.
    let popup = centered_rect(72, dialog::height(7 + counts.len() as u16, false), area);
    dialog::dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    let block = dialog::block(
        clicks,
        hover,
        popup,
        " Details ".to_owned(),
        CYAN,
        Command::CloseOverlay,
    );
    let inner = dialog::body(popup);
    frame.render_widget(block, popup);

    let elapsed = format_clock(run.elapsed());
    let mut rows = vec![
        ("run", run_id.unwrap_or("not recorded").to_owned()),
        ("flake", run.flake.clone()),
        (
            "tree",
            if run.dirty_tree {
                "dirty (uncommitted changes)"
            } else {
                "clean"
            }
            .to_owned(),
        ),
        ("action", run.action.clone()),
        ("cleanup", cleanup_label.to_owned()),
        ("scope", run.scope.label().to_owned()),
        ("elapsed", elapsed),
    ];
    rows.extend(
        counts
            .iter()
            .map(|(label, count)| (*label, count.to_string())),
    );
    let lines = rows.iter().map(|(label, value)| {
        Line::from(vec![
            Span::styled(format!(" {label:<11}"), Style::default().fg(MUTED)),
            Span::styled(value.clone(), Style::default().fg(CYAN)),
        ])
    });
    frame.render_widget(Paragraph::new(Text::from_iter(lines)), inner);
}

/// The cancel dialog: what stopping the run would mean, and the two answers. What it says
/// follows the phase being stopped: a cleanup leaves an activated rebuild behind.
pub fn render_confirm_cancel(
    frame: &mut Frame<'_>,
    area: Rect,
    phase: Phase,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
) {
    let (title, wording, keep_label, cancel_label) = if phase == Phase::Cleanup {
        (
            "Cancel cleanup?",
            [
                "The cleanup's current step will be stopped.",
                "The rebuild stays activated; nothing else changes.",
            ],
            "[N  Keep cleaning]",
            "[Y  Cancel cleanup]",
        )
    } else {
        (
            "Cancel rebuild?",
            [
                "The current process group will be stopped.",
                "Activation may already have changed the system.",
            ],
            "[N  Keep running]",
            "[Y  Cancel rebuild]",
        )
    };
    Dialog::new(title.trim(), RED, Command::CloseOverlay)
        .push(Line::from(wording[0]))
        .push(Line::from(wording[1]))
        .ask(
            Answers::new(keep_label, cancel_label)
                .colors(MUTED, RED)
                .actions(Command::CloseOverlay, super::Command::ConfirmCancel.into()),
        )
        .render_in(frame, hover, clicks, area);
}
