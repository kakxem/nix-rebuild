//! The cleanup's two views: the prompt and the Activity stage list.

use std::time::Instant;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::common::hit_targets::{Click, ClickMap, Hover};
use crate::common::layout::centered_rect;
use crate::common::text::{animation_tick, format_clock};
use crate::common::theme::{CYAN, GREEN, MUTED, PURPLE, RED, SPINNER_FRAMES, YELLOW};
use crate::common::ui::{dialog_block, dialog_body, dialog_height, dim_backdrop};
use crate::modules::work::cleanup::{CLEANUP_STAGES, State};

// One key inside each chip, the word spelling the answer: the other keys that answer
// (Enter, Esc) stay in the corner and in the help, not repeated twice on the same row.
const NO_KEY: &str = "[N";
const NO_WORD: &str = "  No]";
const YES_KEY: &str = "[Y";
const YES_WORD: &str = "  Yes]";
const BUTTON_GAP: &str = "     ";
const QUESTION: &str = "Clean old generations and collect garbage?";

fn cells(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

/// The answers' whole line, so the box width and the click rects come from the same measure.
fn answers_width() -> u16 {
    cells(NO_KEY) + cells(NO_WORD) + cells(BUTTON_GAP) + cells(YES_KEY) + cells(YES_WORD)
}

/// Question, blank row, answers: the two border rows and the one cell of air `dialog_body`
/// keeps above and below them already separate the question from the rule.
const QUESTION_ROWS: u16 = 3;
const DIALOG_ROWS: u16 = dialog_height(QUESTION_ROWS, false);
/// Air added to each side of the widest line: two of these cells land outside the body.
const SIDE_PADDING: u16 = 10;
/// Where the answers sit inside the body: question, blank row, then them.
const ANSWERS_ROW: u16 = 2;

/// The question's own box, wide enough for the text or its buttons, whichever is longer.
fn question_rect(area: Rect) -> Rect {
    let width = cells(QUESTION)
        .max(answers_width())
        .saturating_add(SIDE_PADDING);
    centered_rect(width, DIALOG_ROWS, area)
}

/// The cleanup decision is the app's dialog, whichever cards are on screen: a question that
/// changes the machine does not belong to a card the user may have closed. The corner answers
/// Esc, which this question binds to No.
pub fn render_cleanup_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
) {
    let popup = question_rect(area);
    dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    // No title: the question names itself, and the corner is the only chrome it needs.
    let block = dialog_block(
        clicks,
        hover,
        popup,
        "",
        YELLOW,
        super::Command::SkipCleanup.into(),
    );
    frame.render_widget(block, popup);

    let inner = dialog_body(popup);
    let row = inner.y.saturating_add(ANSWERS_ROW);
    let start = inner
        .x
        .saturating_add(inner.width.saturating_sub(answers_width()) / 2);
    let no_area = (start, row, cells(NO_KEY) + cells(NO_WORD), 1);
    let yes_area = (
        start
            .saturating_add(no_area.2)
            .saturating_add(cells(BUTTON_GAP)),
        row,
        cells(YES_KEY) + cells(YES_WORD),
        1,
    );
    clicks.push((no_area, Click::Command(super::Command::SkipCleanup.into())));
    clicks.push((
        yes_area,
        Click::Command(super::Command::StartCleanup.into()),
    ));

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
    let text = Text::from(vec![
        Line::from(Span::styled(
            QUESTION,
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(vec![
            // Red for the answer that declines: grey was too quiet to rival the green.
            Span::styled(
                NO_KEY,
                button_style(no_area, RED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(NO_WORD, button_style(no_area, RED)),
            Span::raw(BUTTON_GAP),
            Span::styled(
                YES_KEY,
                button_style(yes_area, GREEN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(YES_WORD, button_style(yes_area, GREEN)),
        ]),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
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
