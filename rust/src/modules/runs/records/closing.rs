//! The closing report of a run: the line that says how to read the run back.

use std::io::{self, Write};

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::common::config::LeaveScreen;
use crate::common::theme::{CYAN, MUTED};

/// The closing line: on the footer row while the frame is up, or printed on the shell's screen.
/// Spelled without `--logs`: a bare `runs <id>` opens the summary frame first.
pub fn closing_line(id: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled("  Resume this run with: ", Style::default().fg(MUTED)),
        Span::styled("nix-rebuild runs ", Style::default().fg(CYAN)),
        Span::styled(
            id.to_owned(),
            Style::default().fg(CYAN).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Hand-written escape sequences: it prints after the frame is gone, in cooked mode.
pub fn closing_text(id: &str) -> String {
    let mut text = String::new();
    for span in closing_line(id).spans {
        text.push_str(&sgr(span.style));
        text.push_str(span.content.as_ref());
    }
    text.push_str("\x1b[0m");
    text
}

fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = Vec::new();
    if style.add_modifier.contains(Modifier::BOLD) {
        codes.push("1".into());
    }
    if let Some(Color::Rgb(red, green, blue)) = style.fg {
        codes.push(format!("38;2;{red};{green};{blue}"));
    }
    if codes.is_empty() {
        return String::new();
    }
    format!("\x1b[{}m", codes.join(";"))
}

pub fn render_closing(frame: &mut Frame<'_>, id: &str, footer: Rect) {
    if footer.width == 0 || footer.height == 0 {
        return;
    }
    let line = closing_line(id);
    let area = Rect {
        height: 1,
        ..footer
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// What closing a frame does to the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Leave {
    Frame,
    /// Wiped with nothing left: a recording the reader just picked.
    CleanSilent,
    /// Wiped, then the closing line is printed: the run just happened.
    CleanMessage,
}

/// A pure decision, testable without a terminal: a wiped frame keeps its closing
/// line only for a run that just happened, not a recording the reader picked.
pub fn leave_plan(leave: LeaveScreen, reopened: bool) -> Leave {
    match (leave, reopened) {
        (LeaveScreen::Frame, _) => Leave::Frame,
        (LeaveScreen::Clean, true) => Leave::CleanSilent,
        (LeaveScreen::Clean, false) => Leave::CleanMessage,
    }
}

/// The closing line on the shell's own screen, after the frame's screen is gone.
pub fn print_closing_line(id: &str) -> io::Result<()> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{}", closing_text(id))?;
    stdout.flush()
}
