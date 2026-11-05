//! Every box the app opens: the parts a dialog is made of, and the `Dialog` those parts build.
//! The corner, the dimming, the click areas and the hover live here, so a new question cannot
//! forget any of them and no overlay paints its own border.

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::common::actions::Command;
use crate::common::hit_targets::{Click, ClickArea, ClickMap, Hover, area_contains};
use crate::common::layout::centered_rect;
use crate::common::theme::{CYAN, GREEN, MUTED, RED, YELLOW};

/// Air between the two answers, wide enough that they read as two buttons.
pub const ANSWER_GAP: &str = "     ";

/// The cells a dialog's two answers were drawn in, plus the corner that closes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnswerAreas {
    pub dismiss: ClickArea,
    pub no: ClickArea,
    pub yes: ClickArea,
}

/// The dialog's body: one cell of air inside the border, on every side. Every dialog lays its
/// text and its hit map out from this rect, so the inset is not something a new dialog can
/// forget and a clickable row cannot drift from the row it paints. A dialog that needs more
/// air than this adds it to the box it asks for, never by insetting again here.
pub fn body(popup: Rect) -> Rect {
    Rect {
        x: popup.x.saturating_add(2),
        y: popup.y.saturating_add(2),
        width: popup.width.saturating_sub(4),
        height: popup.height.saturating_sub(4),
    }
}

/// The rows a dialog needs for `content` rows of body: the two borders and the air above it, plus
/// the hint row and the blank row that keeps the hint off the content when it carries one. A
/// dialog that adds a row of content asks for one more here instead of hand-tuning its height.
pub const fn height(content: u16, hint: bool) -> u16 {
    content + if hint { 5 } else { 4 }
}

/// The dialog's bottom row, inside the borders and outside the body: a hint sits here, hugging
/// the frame, because it is the dialog's chrome and not part of what the body lays out. A dialog
/// with a hint leaves its body's last row blank, so the hint never touches the content above it:
/// that is `content + 5` rows, the fifth being this one.
pub fn hint_row(popup: Rect) -> Rect {
    Rect {
        x: popup.x.saturating_add(1),
        y: popup.bottom().saturating_sub(2),
        width: popup.width.saturating_sub(2),
        height: 1,
    }
}

/// The `[Esc]` in a dialog's top-right corner: the cells its pointer closes it on.
fn corner(popup: Rect) -> ClickArea {
    const CLOSE_CELLS: u16 = 5;
    (popup.right().saturating_sub(6), popup.y, CLOSE_CELLS, 1)
}

/// Where a dialog's two answers land: centred on the `row`-th row of its body, counted from the
/// top of the content. The pointer is matched against these cells, so a click answers what the
/// box asks instead of a rect measured a second time.
fn answer_areas(popup: Rect, no: &str, yes: &str, row: u16) -> AnswerAreas {
    let inner = body(popup);
    let gap = UnicodeWidthStr::width(ANSWER_GAP) as u16;
    let no_width = UnicodeWidthStr::width(no) as u16;
    let yes_width = UnicodeWidthStr::width(yes) as u16;
    let total = no_width + gap + yes_width;
    let start = inner
        .x
        .saturating_add(inner.width.saturating_sub(total) / 2);
    let y = inner.y.saturating_add(row);
    AnswerAreas {
        dismiss: corner(popup),
        no: (start, y, no_width, 1),
        yes: (
            start.saturating_add(no_width).saturating_add(gap),
            y,
            yes_width,
            1,
        ),
    }
}

/// The dialog's title rule, with the corner that answers the one key that leaves it. Opening the
/// dialog here is what makes it modal: the box and the command its backdrop means are recorded
/// with the frame's own controls, and nothing behind answers until the next frame drops it.
pub fn block(
    clicks: &mut ClickMap,
    hover: Option<Hover>,
    popup: Rect,
    title: impl Into<String>,
    color: Color,
    close: Command,
) -> Block<'static> {
    clicks.open_dialog((popup.x, popup.y, popup.width, popup.height), close);
    // The way out is the key itself: `[Esc]` says nothing the footer has to repeat.
    let dismiss = corner(popup);
    clicks.push((dismiss, Click::Command(close)));
    let hovered = hover.is_some_and(|hover| {
        hover.column >= dismiss.0
            && hover.column < dismiss.0.saturating_add(dismiss.2)
            && hover.row == dismiss.1
    });
    let close_style = if hovered {
        Style::default()
            .fg(RED)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(MUTED).add_modifier(Modifier::BOLD)
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(ratatui::widgets::BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .title(Span::styled(
            title.into(),
            Style::default().fg(color).bold(),
        ))
        .title_top(Line::from(Span::styled("[Esc]", close_style)).right_aligned())
}

/// How much of each channel survives behind a dialog. A terminal cell has no alpha, so a
/// backdrop cannot be shaded: what is already painted recedes by turning darker.
const BACKDROP_PERCENT: u16 = 50;

/// What an uncoloured cell becomes behind a dialog, `Reset` being the terminal's own fg.
const BACKDROP_UNCOLOURED: Color = Color::Rgb(70, 70, 70);

/// Darken every painted cell outside the dialog's own rect, so the dialog reads first.
pub fn dim_backdrop(frame: &mut Frame<'_>, keep: Rect) {
    let area = frame.area();
    let buffer = frame.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if x >= keep.x && x < keep.right() && y >= keep.y && y < keep.bottom() {
                continue;
            }
            if let Some(cell) = buffer.cell_mut((x, y)) {
                let style = recede(cell.style());
                cell.set_style(style);
            }
        }
    }
}

/// A cell's own colour at half brightness. `Reset` text has no colour to halve, so it takes
/// the dim grey; a `Reset` background is the terminal's own and must not be painted over.
fn recede(style: Style) -> Style {
    let fg = match style.fg {
        None | Some(Color::Reset) => BACKDROP_UNCOLOURED,
        Some(color) => darken(color),
    };
    Style {
        fg: Some(fg),
        bg: style.bg.map(darken),
        ..style
    }
}

fn darken(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(part(r), part(g), part(b)),
        other => other,
    }
}

fn part(channel: u8) -> u8 {
    (u16::from(channel) * BACKDROP_PERCENT / 100) as u8
}

/// Air added to each side of the widest line: two of these cells land outside the body.
pub const PADDING: u16 = 8;

/// The two answers a question offers: what each says, the colour it is drawn in, and what a click
/// on it means. The keyboard keeps answering each one its own way.
pub struct Answers {
    pub no: String,
    pub yes: String,
    pub no_color: Color,
    pub yes_color: Color,
    pub no_action: Command,
    pub yes_action: Command,
}

impl Answers {
    /// The routine pair: green to decline, cyan to go ahead, both just closing the box until the
    /// caller says what its keys do.
    pub fn new(no: impl Into<String>, yes: impl Into<String>) -> Self {
        Self {
            no: no.into(),
            yes: yes.into(),
            no_color: GREEN,
            yes_color: CYAN,
            no_action: Command::CloseOverlay,
            yes_action: Command::CloseOverlay,
        }
    }

    pub fn colors(mut self, no: Color, yes: Color) -> Self {
        self.no_color = no;
        self.yes_color = yes;
        self
    }

    pub fn actions(mut self, no: Command, yes: Command) -> Self {
        self.no_action = no;
        self.yes_action = yes;
        self
    }
}

/// Boxes are built with the props a slice has to say and rendered once; there is no state to keep.
pub struct Dialog {
    title: String,
    color: Color,
    lines: Vec<Line<'static>>,
    answers: Option<Answers>,
    close: Command,
    width: Option<u16>,
    padding: u16,
    dim: bool,
}

/// Where the box landed and where its answers did, so the pointer is matched against the drawing.
#[derive(Debug, Clone, Copy)]
pub struct Drawn {
    pub popup: Rect,
    pub answers: Option<AnswerAreas>,
}

/// Which answer a cell belongs to. The corner says no, as `Esc` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    No,
    Yes,
}

/// The answer a click at these cells gives, if it lands on one.
pub fn answer_at(areas: AnswerAreas, column: u16, row: u16) -> Option<Answer> {
    if area_contains(areas.yes, column, row) {
        return Some(Answer::Yes);
    }
    if area_contains(areas.no, column, row) || area_contains(areas.dismiss, column, row) {
        return Some(Answer::No);
    }
    None
}

impl Dialog {
    /// A box with no question: just a title, its lines and the corner that closes it.
    pub fn new(title: impl Into<String>, color: Color, close: Command) -> Self {
        Self {
            title: title.into(),
            color,
            lines: Vec::new(),
            answers: None,
            close,
            width: None,
            padding: PADDING,
            dim: true,
        }
    }

    /// The question variant: the title, the question in the frame's warning colour, and two
    /// answers. The line under it is `detail`, when the question needs one.
    pub fn confirm(title: impl Into<String>, question: impl Into<String>) -> Self {
        Dialog::new(title, YELLOW, Command::CloseOverlay).push(Line::from(Span::styled(
            question.into(),
            Style::default().fg(YELLOW).add_modifier(Modifier::BOLD),
        )))
    }

    /// The row under the question: the count, the digests, the lock it names.
    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.lines.push(Line::from(Span::styled(
            detail.into(),
            Style::default().fg(CYAN),
        )));
        self
    }

    pub fn push(mut self, line: Line<'static>) -> Self {
        self.lines.push(line);
        self
    }

    /// The two answers, drawn on the last row above the bottom border.
    pub fn ask(mut self, answers: Answers) -> Self {
        self.answers = Some(answers);
        self
    }

    /// What `Esc`, and a click on the corner, mean. A question usually means "no".
    pub fn close(mut self, close: Command) -> Self {
        self.close = close;
        self
    }

    /// A fixed width, for a box whose body is not the text it prints.
    pub fn width(mut self, width: u16) -> Self {
        self.width = Some(width);
        self
    }

    pub fn padding(mut self, padding: u16) -> Self {
        self.padding = padding;
        self
    }

    /// A box that sits over a card it belongs to (a banner) leaves that card readable.
    pub fn dim(mut self, dim: bool) -> Self {
        self.dim = dim;
        self
    }

    /// Draw it in the middle of `area`; the returned cells are what the pointer is matched against.
    pub fn render_in(
        mut self,
        frame: &mut Frame<'_>,
        hover: Option<Hover>,
        clicks: &mut ClickMap,
        area: Rect,
    ) -> Drawn {
        let chips = self
            .answers
            .as_ref()
            .map(|answers| format!("{}{}{}", answers.no, ANSWER_GAP, answers.yes))
            .unwrap_or_default();
        let widest = self
            .lines
            .iter()
            .map(Line::width)
            .max()
            .unwrap_or(0)
            .max(UnicodeWidthStr::width(chips.as_str()));
        let width = self
            .width
            .unwrap_or_else(|| (widest as u16).saturating_add(self.padding).min(area.width));
        let content = self.lines.len() as u16 + if self.answers.is_some() { 2 } else { 0 };
        let popup = centered_rect(width, height(content, false).min(area.height), area);
        if self.dim {
            dim_backdrop(frame, popup);
        }
        frame.render_widget(Clear, popup);
        // The title keeps a cell of air on each side; an untitled box passes nothing.
        let title = if self.title.is_empty() {
            String::new()
        } else {
            format!(" {} ", self.title)
        };
        let frame_block = block(clicks, hover, popup, title, self.color, self.close);
        frame.render_widget(frame_block, popup);

        let answers = self.answers.map(|answers| {
            let areas = answer_areas(popup, &answers.no, &answers.yes, content - 1);
            // A click on either chip means what the key in it means.
            clicks.push((areas.no, Click::Command(answers.no_action)));
            clicks.push((areas.yes, Click::Command(answers.yes_action)));
            (answers, areas)
        });
        if let Some((answers, areas)) = &answers {
            self.lines.push(Line::default());
            self.lines.push(Line::from(vec![
                Span::styled(
                    answers.no.clone(),
                    chip_style(answers.no_color, areas.no, hover),
                ),
                Span::raw(ANSWER_GAP),
                Span::styled(
                    answers.yes.clone(),
                    chip_style(answers.yes_color, areas.yes, hover),
                ),
            ]));
        }
        frame.render_widget(
            Paragraph::new(Text::from(self.lines)).alignment(Alignment::Center),
            body(popup),
        );
        Drawn {
            popup,
            answers: answers.map(|(_, areas)| areas),
        }
    }

    pub fn render(
        self,
        frame: &mut Frame<'_>,
        hover: Option<Hover>,
        clicks: &mut ClickMap,
    ) -> Drawn {
        let area = frame.area();
        self.render_in(frame, hover, clicks, area)
    }
}

/// A chip is bold, and underlined while the pointer is on it.
fn chip_style(color: Color, area: ClickArea, hover: Option<Hover>) -> Style {
    let style = Style::default().fg(color).add_modifier(Modifier::BOLD);
    if hover.is_some_and(|hover| area_contains(area, hover.column, hover.row)) {
        style.add_modifier(Modifier::UNDERLINED)
    } else {
        style
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;

    fn question() -> Dialog {
        Dialog::confirm("Rollback", "Revert the lock of 20 Mar 10:46?")
            .detail("41f1002e → 9382a635")
            .ask(Answers::new(
                "[N  Keep the lock]",
                "[Y  Revert and rebuild]",
            ))
    }

    fn draw(dialog: Dialog, hover: Option<Hover>) -> (Buffer, Drawn) {
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        let mut drawn = None;
        let mut clicks = ClickMap::default();
        terminal
            .draw(|frame| drawn = Some(dialog.render(frame, hover, &mut clicks)))
            .unwrap();
        (terminal.backend().buffer().clone(), drawn.unwrap())
    }

    fn screen(buffer: &Buffer) -> String {
        crate::common::ui::buffer_to_string(buffer)
    }

    /// The colour and the modifiers the cell at `area` was drawn with.
    fn ink(buffer: &Buffer, area: ClickArea) -> (Color, Modifier) {
        let cell = buffer.cell((area.0, area.1)).expect("the cell was drawn");
        (cell.fg, cell.modifier)
    }

    #[test]
    fn the_box_carries_its_title_question_detail_answers_and_corner() {
        let (buffer, layout) = draw(question(), None);
        let screen = screen(&buffer);
        assert!(screen.contains("╭ Rollback "), "{screen}");
        assert!(
            screen.contains("Revert the lock of 20 Mar 10:46?"),
            "{screen}"
        );
        assert!(screen.contains("41f1002e → 9382a635"), "{screen}");
        assert!(
            screen.contains("[N  Keep the lock]     [Y  Revert and rebuild]"),
            "{screen}"
        );
        assert!(
            screen.contains("[Esc]"),
            "every dialog has its way out:\n{screen}"
        );
        assert!(layout.answers.is_some(), "the answers were drawn");
    }

    #[test]
    fn the_box_is_centred_in_the_area_it_was_given() {
        // The pickers give the box the whole frame; a banner gives it the card it sits in.
        let bounds = Rect::new(10, 4, 60, 12);
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        let mut drawn = None;
        let mut clicks = ClickMap::default();
        terminal
            .draw(|frame| drawn = Some(question().render_in(frame, None, &mut clicks, bounds)))
            .unwrap();
        let popup = drawn.unwrap().popup;
        assert!(
            popup.width <= bounds.width && popup.height <= bounds.height,
            "the box fits the area it was handed: {popup:?} in {bounds:?}"
        );
        assert!(
            (popup.x - bounds.x).abs_diff(bounds.right() - popup.right()) <= 1,
            "centred across, not shoved to one side: {popup:?}"
        );
        assert!(
            (popup.y - bounds.y).abs_diff(bounds.bottom() - popup.bottom()) <= 1,
            "and centred down: {popup:?}"
        );
    }

    #[test]
    fn a_box_that_asks_nothing_has_no_answer_row() {
        let dialog = Dialog::new("Failed", CYAN, Command::Quit).push(Line::from("the build broke"));
        let (buffer, layout) = draw(dialog, None);
        let screen = screen(&buffer);
        assert!(screen.contains("╭ Failed "), "{screen}");
        assert!(screen.contains("the build broke"), "{screen}");
        assert!(layout.answers.is_none());
    }

    #[test]
    fn the_pointer_lights_the_chip_under_it_and_leaves_the_other_cold() {
        let (cold, layout) = draw(question(), None);
        let answers = layout.answers.expect("the box asks");
        let (hot, _) = draw(
            question(),
            Some(Hover {
                column: answers.yes.0 + 1,
                row: answers.yes.1,
            }),
        );
        assert!(!ink(&cold, answers.yes).1.contains(Modifier::UNDERLINED));
        assert!(ink(&hot, answers.yes).1.contains(Modifier::UNDERLINED));
        assert!(!ink(&hot, answers.no).1.contains(Modifier::UNDERLINED));
        assert_eq!(
            ink(&cold, answers.no),
            ink(&hot, answers.no),
            "the other chip does not change"
        );
        assert_eq!(ink(&hot, answers.yes).0, CYAN, "the chip keeps its colour");
    }

    #[test]
    fn the_corner_lights_up_like_the_chips() {
        let (_, layout) = draw(question(), None);
        let answers = layout.answers.expect("the box asks");
        let (hot, _) = draw(
            question(),
            Some(Hover {
                column: answers.dismiss.0 + 1,
                row: answers.dismiss.1,
            }),
        );
        let cell = hot.cell((answers.dismiss.0, answers.dismiss.1)).unwrap();
        assert!(cell.modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn a_click_on_a_chip_or_the_corner_names_one_answer() {
        let (_, layout) = draw(question(), None);
        let answers = layout.answers.expect("the box asks");
        let middle = |area: ClickArea| (area.0 + area.2 / 2, area.1);
        let yes = middle(answers.yes);
        let no = middle(answers.no);
        let dismiss = middle(answers.dismiss);
        assert_eq!(answer_at(answers, yes.0, yes.1), Some(Answer::Yes));
        assert_eq!(answer_at(answers, no.0, no.1), Some(Answer::No));
        assert_eq!(answer_at(answers, dismiss.0, dismiss.1), Some(Answer::No));
        assert_eq!(
            answer_at(answers, 0, 0),
            None,
            "the backdrop answers nothing"
        );
    }

    #[test]
    fn a_click_on_a_chip_dispatches_the_action_the_caller_gave_it() {
        let mut terminal = Terminal::new(TestBackend::new(80, 18)).unwrap();
        let mut clicks = ClickMap::default();
        let mut drawn = None;
        terminal
            .draw(|frame| {
                drawn = Some(
                    question()
                        .ask(
                            Answers::new("[N  Keep the lock]", "[Y  Revert and rebuild]")
                                .actions(Command::CloseOverlay, Command::Quit),
                        )
                        .render(frame, None, &mut clicks),
                )
            })
            .unwrap();
        let answers = drawn.unwrap().answers.expect("the box asks");
        let middle = (answers.yes.0 + answers.yes.2 / 2, answers.yes.1);
        assert_eq!(
            clicks.under(middle.0, middle.1),
            Some(Click::Command(Command::Quit)),
            "the chip answers with the command its key would"
        );
    }
}
