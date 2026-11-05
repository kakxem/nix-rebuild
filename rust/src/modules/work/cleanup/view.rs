//! The cleanup's two views: the prompt and the Activity stage list.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

use crate::common::dialog::{Answers, Dialog};
use crate::common::hit_targets::{ClickMap, Hover};
use crate::common::text::{animation_tick, format_clock};
use crate::common::theme::{CYAN, GREEN, MUTED, PURPLE, RED, SPINNER_FRAMES};
use crate::modules::work::cleanup::{CLEANUP_STAGES, State};

const QUESTION: &str = "Clean old generations and collect garbage?";

/// The cleanup decision is the app's dialog, whichever cards are on screen: a question that
/// changes the machine does not belong to a card the user may have closed. The corner answers
/// Esc, which this question binds to No.
pub fn render_cleanup_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
) {
    // No title: the question names itself, and the corner is the only chrome it needs. Red for
    // the answer that declines, green for the one that starts the work.
    Dialog::confirm("", QUESTION)
        .ask(
            Answers::new("[N  No]", "[Y  Yes]")
                .colors(RED, GREEN)
                .actions(
                    super::Command::SkipCleanup.into(),
                    super::Command::StartCleanup.into(),
                ),
        )
        .close(super::Command::SkipCleanup.into())
        .padding(10)
        .render_in(frame, hover, clicks, area);
}

/// Every stage marked done, running or to come, plus the pass's generations
/// and what the collector freed.
pub fn render_cleanup_activity(frame: &mut Frame<'_>, state: &State, inner: Rect) {
    let tick = animation_tick();
    let spinner = SPINNER_FRAMES[tick % SPINNER_FRAMES.len()];

    let mut lines = vec![Line::from(vec![
        Span::styled(
            "Cleaning old generations",
            Style::default().fg(PURPLE).add_modifier(Modifier::BOLD),
        ),
        Span::styled(cleanup_clock(state.started_at), Style::default().fg(MUTED)),
    ])];
    let mut marks = CLEANUP_STAGES
        .iter()
        .enumerate()
        .map(|(index, stage)| {
            cleanup_stage_line(index, stage, state.stage, &state.generations, spinner)
        })
        .collect::<Vec<_>>();
    if let Some((paths, freed)) = &state.freed {
        marks.push(Line::from(Span::styled(
            format!("{paths} paths deleted · {freed} freed"),
            Style::default().fg(CYAN),
        )));
    }

    let width = marks
        .iter()
        .map(Line::width)
        .max()
        .unwrap_or(0)
        .max(24)
        .min(usize::from(inner.width));
    lines.extend(marks);

    let height = (lines.len() as u16).min(inner.height);
    let area = Rect {
        x: inner.x + inner.width.saturating_sub(width as u16) / 2,
        y: inner.y + inner.height.saturating_sub(height) / 2,
        width: width as u16,
        height,
    };
    frame.render_widget(Paragraph::new(Text::from(lines)), area);
}

/// Separate clock: the rebuild's own timer stops at the cleanup decision.
fn cleanup_clock(started_at: Option<Instant>) -> String {
    let Some(started) = started_at else {
        return String::new();
    };
    format!("   {}", format_clock(started.elapsed()))
}

fn cleanup_stage_line(
    index: usize,
    stage: &str,
    cleanup_stage: Option<usize>,
    generations: &[String],
    spinner: &str,
) -> Line<'static> {
    let (mark, color) = match cleanup_stage {
        Some(current) if index < current => ("\u{f00c}", GREEN),
        Some(current) if index == current => (spinner, PURPLE),
        _ => ("\u{f10c}", MUTED),
    };
    let mut chips = vec![
        Span::styled(format!("{mark}  "), Style::default().fg(color)),
        Span::styled(
            (*stage).to_owned(),
            Style::default().fg(if index == cleanup_stage.unwrap_or(usize::MAX) {
                CYAN
            } else {
                MUTED
            }),
        ),
    ];
    if index == 0 && !generations.is_empty() {
        chips.push(Span::styled(
            generations_label(generations),
            Style::default().fg(MUTED),
        ));
    }
    Line::from(chips)
}

/// The ids a label names before it counts the rest, so a machine with a hundred
/// generations neither widens the card nor clips its own line.
const NAMED_GENERATIONS: usize = 3;

fn generations_label(generations: &[String]) -> String {
    let total = generations.len();
    let named = total.min(NAMED_GENERATIONS);
    let plural = if total == 1 { "" } else { "s" };
    let mut label = format!("    {total} generation{plural} (");
    label.push_str(&generations[..named].join(", "));
    if total > named {
        label.push_str(&format!(", +{} more", total - named));
    }
    label.push(')');
    label
}
