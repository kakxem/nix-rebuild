//! The question before the runs are cut. It is the app's dialog, so a closed card cannot
//! hide a decision about this machine's recorded runs.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::common::actions::Command as Envelope;
use crate::common::dialog::{Answers, Dialog};
use crate::common::hit_targets::{ClickMap, Hover};
use crate::common::theme::{GREEN, MUTED, RED, YELLOW};

const QUESTION: &str = "Remove all previous runs?";

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
    // No title: the question names itself, and the corner answers the key that declines.
    // Deleting is not a routine step, so the answer that declines is the green one.
    Dialog::new("", YELLOW, Envelope::CloseOverlay)
        .push(Line::from(Span::styled(
            QUESTION,
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )))
        .push(Line::from(Span::styled(
            detail(count),
            Style::default().fg(MUTED),
        )))
        .ask(
            Answers::new("[N  No]", "[Y  Yes]")
                .colors(GREEN, RED)
                .actions(Envelope::CloseOverlay, super::Command::ConfirmRemove.into()),
        )
        .padding(10)
        .render_in(frame, hover, clicks, area);
}
