//! How much room a card asks for. A card declares its own size; the stack that arranges the
//! cards is the dashboard's (`app::dashboard::panels`).

use ratatui::layout::Constraint;

/// How much room a card asks for. A drag trades rows around this and never goes below the floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Height {
    /// Always these rows, and never a drag handle.
    Fixed(u16),
    /// Its own row count, capped to half the frame, so the cards above keep their room.
    Rows { rows: u16, floor: u16 },
    /// Whatever the cards above leave.
    Fill { floor: u16 },
    /// A share of the frame, clamped.
    Fraction {
        num: u16,
        den: u16,
        min: u16,
        max: u16,
    },
}

impl Height {
    /// The smallest size the card keeps, as a drag limit and as the floor of the card that
    /// holds the leftover.
    pub fn floor(self) -> u16 {
        match self {
            Height::Fixed(rows) => rows,
            Height::Rows { floor, .. } => floor,
            Height::Fill { floor } => floor,
            Height::Fraction { min, .. } => min,
        }
    }

    pub(crate) fn is_fixed(self) -> bool {
        matches!(self, Height::Fixed(_))
    }

    /// The constraint for a card nothing has pinned, resolved against the frame it sits in.
    pub(crate) fn constraint(self, frame: u16) -> Constraint {
        match self {
            Height::Fixed(rows) => Constraint::Length(rows),
            Height::Rows { rows, floor } => Constraint::Length(rows.min((frame / 2).max(floor))),
            Height::Fill { floor } => Constraint::Min(floor),
            Height::Fraction { num, den, min, max } => {
                Constraint::Length((frame * num / den).clamp(min, max))
            }
        }
    }
}

/// One card's place in the stack: how it sizes itself, and whether it is up at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelSpec {
    pub height: Height,
    pub visible: bool,
}

impl PanelSpec {
    pub fn new(height: Height, visible: bool) -> Self {
        PanelSpec { height, visible }
    }
}
