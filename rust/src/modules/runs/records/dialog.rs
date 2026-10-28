//! The question before the runs are cut. It is the app's dialog, so a closed card cannot
//! hide a decision about this machine's recorded runs.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph, Wrap};
use unicode_width::UnicodeWidthStr;

use crate::common::actions::Command as Envelope;
use crate::common::hit_targets::{Click, ClickMap, Hover};
use crate::common::layout::centered_rect;
use crate::common::theme::{GREEN, MUTED, RED, YELLOW};
use crate::common::ui::{dialog_block, dialog_body, dialog_height, dim_backdrop};

const QUESTION: &str = "Remove all previous runs?";
// One key inside each chip, the word spelling the answer; Enter and Esc stay in the corner.
const NO_KEY: &str = "[N";
const NO_WORD: &str = "  No]";
const YES_KEY: &str = "[Y";
const YES_WORD: &str = "  Yes]";
const BUTTON_GAP: &str = "     ";
/// Air added to each side of the widest line: two of these cells land outside the body.
const SIDE_PADDING: u16 = 10;
/// Question, the count, the blank row and the answers.
const CONTENT_ROWS: u16 = 4;
/// Where the answers sit inside the body: question, count, blank, then them.
const ANSWERS_ROW: u16 = 3;

fn cells(text: &str) -> u16 {
    UnicodeWidthStr::width(text) as u16
}

fn answers_width() -> u16 {
    cells(NO_KEY) + cells(NO_WORD) + cells(BUTTON_GAP) + cells(YES_KEY) + cells(YES_WORD)
}

/// What the question says it would delete, in the number the records actually hold.
fn detail(count: usize) -> String {
    let runs = if count == 1 {
        "1 recorded run".to_owned()
    } else {
        format!("{count} recorded runs")
    };
    format!("{runs} will be deleted.")
}

pub fn render_remove_dialog(
    frame: &mut Frame<'_>,
    count: usize,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    let detail = detail(count);
    let width = cells(QUESTION)
        .max(cells(&detail))
        .max(answers_width())
        .saturating_add(SIDE_PADDING);
    let popup = centered_rect(width, dialog_height(CONTENT_ROWS, false), area);
    dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);
    // No title: the question names itself, and the corner answers the key that declines.
    let block = dialog_block(clicks, hover, popup, "", YELLOW, Envelope::CloseOverlay);
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
    clicks.push((no_area, Click::Command(Envelope::CloseOverlay)));
    clicks.push((
        yes_area,
        Click::Command(super::Command::ConfirmRemove.into()),
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
        Line::from(Span::styled(detail, Style::default().fg(MUTED))),
        Line::default(),
        Line::from(vec![
            // Deleting is not a routine step, so the answer that declines is the green one.
            Span::styled(
                NO_KEY,
                button_style(no_area, GREEN).add_modifier(Modifier::BOLD),
            ),
            Span::styled(NO_WORD, button_style(no_area, GREEN)),
            Span::raw(BUTTON_GAP),
            Span::styled(
                YES_KEY,
                button_style(yes_area, RED).add_modifier(Modifier::BOLD),
            ),
            Span::styled(YES_WORD, button_style(yes_area, RED)),
        ]),
    ]);
    frame.render_widget(
        Paragraph::new(text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        inner,
    );
}
