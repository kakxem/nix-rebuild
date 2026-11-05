//! The keyboard overlay: a filterable, selectable list, so long descriptions stay readable and can be searched with `/`.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Clear, Paragraph};

use crate::common::actions::Command;
use crate::common::dialog;
use crate::common::hit_targets::{ClickMap, Hover};
use crate::common::layout::centered_rect;
use crate::common::theme::{CYAN, GREEN, MUTED};
use crate::common::ui::{ControlSpec, HelpEntry};
use crate::modules::panels::help::State;

/// The help overlay's footer control: the last one on the footer row.
pub fn footer_control() -> ControlSpec {
    ControlSpec {
        command: super::Command::OpenHelp.into(),
        prefix: "",
        key: "H",
        icon: Some('\u{f128}'),
        label: "elp",
        active: true,
        enabled: true,
    }
}

pub fn render_help(
    frame: &mut Frame<'_>,
    state: &mut State,
    hover: Option<Hover>,
    clicks: &mut ClickMap,
    area: Rect,
) {
    let entries: Vec<HelpEntry> = state.help_entries().collect();
    // Two border rows, one cell of air each side, and the hint row on the frame: the list keeps
    // a blank row of its own above the hint.
    let height = dialog::height(entries.len() as u16, true).clamp(8, area.height.saturating_sub(2));
    let popup = centered_rect(64, height, area);
    dialog::dim_backdrop(frame, popup);
    frame.render_widget(Clear, popup);

    let title = if state.filtering {
        format!(" Keyboard  /{} ", state.query)
    } else if state.query.is_empty() {
        " Keyboard ".to_string()
    } else {
        format!(" Keyboard  /{} ", state.query)
    };
    let block = dialog::block(clicks, hover, popup, title, CYAN, Command::CloseOverlay);
    let inner = dialog::body(popup);
    frame.render_widget(block, popup);

    // The air above the list comes from `dialog::body`; the blank row above the hint is the list's.
    let list = Rect {
        y: inner.y,
        height: inner.height.saturating_sub(1).max(1),
        ..inner
    };
    let rows = list.height as usize;
    let offset = state.index.saturating_sub(rows.saturating_sub(1));
    // Record the list geometry so a click can select and run a row.
    state.area = Some((list.x, list.y, list.width, list.height));
    state.offset = offset;
    let lines = entries
        .iter()
        .skip(offset)
        .take(rows)
        .enumerate()
        .map(|(i, entry)| {
            let index = offset + i;
            let selected = index == state.index;
            let (cursor, style) = if selected {
                (
                    '\u{276f}',
                    Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
                )
            } else {
                (' ', Style::default().fg(MUTED))
            };
            let key_style = style;
            let label_style = if selected {
                Style::default().fg(GREEN)
            } else {
                Style::default().fg(MUTED)
            };
            Line::from(vec![
                Span::styled(format!(" {cursor} "), style),
                Span::styled(format!("{:<10}", entry.key), key_style),
                Span::styled(entry.label, label_style),
            ])
        });
    frame.render_widget(Paragraph::new(Text::from_iter(lines)), list);

    let hint = if state.filtering {
        " type to search   Enter apply   Esc clear "
    } else if entries.is_empty() {
        " no match   Esc clears the search "
    } else {
        " ↑/↓ move   / search "
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(hint, Style::default().fg(MUTED))))
            .alignment(Alignment::Center),
        dialog::hint_row(popup),
    );
}
