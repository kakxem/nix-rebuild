//! What a card is made of: its label, its clickable controls, and the renderer that reports the
//! width it painted, so the mouse hit map follows the screen instead of a copy of the layout.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders};
use unicode_width::UnicodeWidthStr;

use crate::common::actions::Command;
use crate::common::hit_targets::Click;
use crate::common::theme::{CYAN, GREEN, MUTED};

/// One row of the keyboard help overlay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelpEntry {
    pub key: &'static str,
    pub label: &'static str,
    /// The command this row dispatches, identical to what its own key reports.
    pub command: Option<Command>,
}

impl HelpEntry {
    /// What Enter dispatches; a row with no command just dismisses the list.
    pub fn enter_command(self) -> Command {
        self.command.unwrap_or(Command::CloseOverlay)
    }

    pub fn matches(self, query: &str) -> bool {
        if query.is_empty() {
            return true;
        }
        let needle = query.to_lowercase();
        self.key.to_lowercase().contains(&needle) || self.label.to_lowercase().contains(&needle)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ControlSpec {
    pub command: Command,
    /// Text before the hotkey, allowing shortcuts such as `Fil[T]er`.
    pub prefix: &'static str,
    /// The key the user presses, highlighted inside the label.
    pub key: &'static str,
    /// The leading glyph, which is never underlined.
    pub icon: Option<char>,
    pub label: &'static str,
    /// Whether the thing it toggles is currently on.
    pub active: bool,
    pub enabled: bool,
}

/// Paint a control as a bracketed `[ … ]` button and report its terminal-cell width;
/// that silhouette marks every clickable element.
pub fn render_control(spec: &ControlSpec, hovered: bool, _framed: bool) -> (Line<'static>, u16) {
    let (mut key_style, mut text_style) = if !spec.enabled {
        (
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
        )
    } else if spec.active {
        (
            Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
            Style::default().fg(GREEN),
        )
    } else {
        (
            Style::default().fg(MUTED),
            Style::default().fg(MUTED).add_modifier(Modifier::DIM),
        )
    };
    if hovered {
        key_style = key_style.add_modifier(Modifier::UNDERLINED);
        text_style = text_style.add_modifier(Modifier::UNDERLINED);
    }

    // No glyph, or a glyph that opens the label: `[Esc]` and `[ Filter]` share one renderer.
    let icon_text = match spec.icon {
        Some(icon) if spec.key.is_empty() && spec.label.is_empty() => icon.to_string(),
        Some(icon) => format!("{icon} "),
        None => String::new(),
    };
    let icon_span = Span::styled(icon_text, icon_style(spec));
    let prefix_span = Span::styled(spec.prefix.to_owned(), text_style);
    let key_span = Span::styled(spec.key.to_owned(), key_style);
    let label_span = Span::styled(spec.label.to_owned(), text_style);

    let spans = vec![
        Span::styled("[", Style::default().fg(MUTED)),
        icon_span,
        prefix_span,
        key_span,
        label_span,
        Span::styled("]", Style::default().fg(MUTED)),
    ];

    let line = Line::from(spans);
    let width = line_width(&line);
    (line, width)
}

/// Terminal columns, not bytes, so a Nerd Font glyph counts as one cell.
pub fn line_width(line: &Line<'static>) -> u16 {
    line.spans
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>() as u16
}

/// The icon keeps the semantic colour of its control but is never underlined.
pub fn icon_style(spec: &ControlSpec) -> Style {
    if !spec.enabled {
        Style::default().fg(MUTED).add_modifier(Modifier::DIM)
    } else if spec.active {
        Style::default().fg(GREEN)
    } else {
        Style::default().fg(MUTED).add_modifier(Modifier::DIM)
    }
}

/// Render each spec paired with its spans, so hit targets come from the widths painted.
pub fn render_controls(
    specs: Vec<ControlSpec>,
    hovered: Option<Click>,
) -> Vec<(ControlSpec, Vec<Span<'static>>)> {
    specs
        .into_iter()
        .map(|spec| {
            let is_hovered = hovered.is_some_and(|click| click.command() == spec.command);
            let (line, _) = render_control(&spec, is_hovered && spec.enabled, false);
            (spec, line.spans)
        })
        .collect()
}

/// The left-hand title of a panel, ruled off from the trailing controls.
pub fn panel_label(label: &str) -> Line<'static> {
    Line::from(Span::styled(format!("─ {label} "), Style::default().bold()))
}

pub const PANEL_CONTROL_INSET: u16 = 3;

/// The `[ … ]` buttons that trail a panel title.
pub fn panel_controls(controls: Vec<(ControlSpec, Vec<Span<'static>>)>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (i, (_, chips)) in controls.into_iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.extend(chips);
    }
    spans.push(Span::styled(
        "─".repeat(PANEL_CONTROL_INSET as usize),
        Style::default().fg(MUTED),
    ));
    Line::from(spans)
}

pub fn control_width(chips: &[Span<'static>]) -> u16 {
    control_cells(chips)
}

pub fn control_cells(chips: &[Span<'static>]) -> u16 {
    chips
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>() as u16
}

pub fn toggle_line(label: &str, on: bool, selected: bool) -> Line<'static> {
    let (mark, state_style) = if on {
        ("\u{f00c}", Style::default().fg(GREEN))
    } else {
        ("\u{f00d}", Style::default().fg(MUTED))
    };
    let label_style = if selected {
        Style::default().fg(CYAN).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(MUTED)
    };
    Line::from(vec![
        Span::styled(if selected { " \u{276f} " } else { "   " }, label_style),
        Span::styled(format!("[{mark}] "), state_style),
        Span::styled(label.to_owned(), label_style),
    ])
}

/// The frame a card draws itself in. Dialogs have `dialog::block`; cards have this, so the
/// border style, the title rule and where its controls ride are one edit for all of them.
pub fn panel_block(
    title: Line<'static>,
    color: Color,
    trailing: Option<Line<'static>>,
) -> Block<'static> {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(title);
    match trailing {
        Some(line) => block.title_top(line),
        None => block,
    }
}

/// The painted cells as lines of text, trailing blanks trimmed. Tests read the screen with it.
#[cfg(test)]
pub fn buffer_to_string(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    let mut output = String::new();
    for y in area.top()..area.bottom() {
        let mut line = String::new();
        for x in area.left()..area.right() {
            if let Some(cell) = buffer.cell((x, y)) {
                line.push_str(cell.symbol());
            }
        }
        output.push_str(line.trim_end());
        output.push('\n');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported width must match the painted cells, or a hit target is off by one.
    #[test]
    fn render_control_reports_the_width_it_paints() {
        let spec = ControlSpec {
            command: crate::modules::panels::logs::Command::ToggleRaw.into(),
            prefix: "",
            key: "R",
            icon: Some('\u{f121}'),
            label: "aw",
            active: true,
            enabled: true,
        };
        let (line, width) = render_control(&spec, false, false);
        let painted: usize = line
            .spans
            .iter()
            .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
            .sum();
        assert_eq!(
            painted, width as usize,
            "the reported width equals the painted cells",
        );
    }

    #[test]
    fn render_control_underlines_the_key_and_label_but_not_the_icon() {
        use ratatui::style::Modifier;

        let spec = ControlSpec {
            command: crate::modules::panels::logs::Command::ToggleRaw.into(),
            prefix: "",
            key: "R",
            icon: Some('\u{f121}'),
            label: "aw",
            active: true,
            enabled: true,
        };
        let (line, _) = render_control(&spec, true, false);
        let spans: Vec<(&str, bool)> = line
            .spans
            .iter()
            .map(|span| {
                (
                    span.content.as_ref(),
                    span.style.add_modifier.contains(Modifier::UNDERLINED),
                )
            })
            .collect();
        let icon = spans
            .iter()
            .find(|(text, _)| text.contains('\u{f121}'))
            .expect("icon span");
        assert!(!icon.1, "the icon is never underlined");
        for label in ["R", "aw"] {
            let span = spans
                .iter()
                .find(|(text, _)| *text == label)
                .unwrap_or_else(|| panic!("{label} span"));
            assert!(span.1, "{label} is underlined on hover");
        }
    }
}
