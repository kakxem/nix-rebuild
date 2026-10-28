//! The cards the frame stacks, their order, and how a border drag trades rows between two.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::common::panels::{Height, PanelSpec};

/// The cards the frame stacks, in the order they are drawn: the one place that order lives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelId {
    Status,
    Activity,
    Logs,
}

impl PanelId {
    pub const ALL: [PanelId; 3] = [PanelId::Status, PanelId::Activity, PanelId::Logs];

    fn index(self) -> usize {
        match self {
            PanelId::Status => 0,
            PanelId::Activity => 1,
            PanelId::Logs => 2,
        }
    }
}

/// The heights a drag has pinned, one slot per card. Nothing here reaches the settings file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Heights {
    pins: [Option<u16>; PanelId::ALL.len()],
}

impl Heights {
    pub fn get(self, id: PanelId) -> Option<u16> {
        self.pins[id.index()]
    }

    pub fn set(&mut self, id: PanelId, height: u16) {
        self.pins[id.index()] = Some(height);
    }
}

/// Two cards that share a border, with the floor each one keeps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Split {
    pub above: PanelId,
    pub below: PanelId,
    pub min_above: u16,
    pub min_below: u16,
}

/// A drag moving one border, with the heights it started from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resize {
    pub split: Split,
    /// The row the pointer grabbed, so the drag is measured from there and not from the edge.
    pub row: u16,
    pub start_above: u16,
    pub start_below: u16,
}

impl Resize {
    /// Trade rows across the border: the lower card follows the pointer and the upper keeps
    /// what is left of the pair, so nothing outside the pair moves.
    pub fn apply(&self, heights: &mut Heights, row: u16) {
        let total = i32::from(self.start_above) + i32::from(self.start_below);
        // The border follows the pointer: moving it up grows the card below, moving it down
        // gives those rows back to the card above.
        let delta = i32::from(row) - i32::from(self.row);
        let wanted = i32::from(self.start_below) - delta;
        let low = i32::from(self.split.min_below);
        // A frame too short for both floors leaves the lower card at its floor rather than panicking.
        let high = (total - i32::from(self.split.min_above)).max(low);
        let below = wanted.clamp(low, high) as u16;
        heights.set(self.split.below, below);
        heights.set(self.split.above, (total - i32::from(below)) as u16);
    }
}

/// The cards as one frame drew them, with the borders a drag can grab.
#[derive(Debug, Clone, Default)]
pub struct Panels {
    area: Rect,
    rects: [Rect; PanelId::ALL.len()],
    splits: [Option<Split>; PanelId::ALL.len() - 1],
}

impl Panels {
    /// Resolve the stack inside `inner`, honouring the heights a drag has pinned.
    pub fn layout(
        inner: Rect,
        specs: &[PanelSpec; PanelId::ALL.len()],
        heights: Heights,
    ) -> Panels {
        let mut constraints = [Constraint::Length(0); PanelId::ALL.len()];
        let mut holds_leftover = false;
        for (index, spec) in specs.iter().enumerate() {
            if !spec.visible {
                continue;
            }
            match heights.get(PanelId::ALL[index]) {
                Some(pinned) => constraints[index] = Constraint::Length(pinned),
                None => {
                    constraints[index] = spec.height.constraint(inner.height);
                    holds_leftover |= matches!(spec.height, Height::Fill { .. });
                }
            }
        }
        // With no card asking for the leftover, the last one up holds it, so the stack stays
        // flush with the footer instead of leaving rows empty.
        if !holds_leftover {
            let last = (0..PanelId::ALL.len()).rev().find(|index| {
                specs[*index].visible
                    && !specs[*index].height.is_fixed()
                    && heights.get(PanelId::ALL[*index]).is_none()
            });
            match last {
                Some(index) => constraints[index] = Constraint::Min(specs[index].height.floor()),
                // Nothing to draw: one card takes the frame so no row is left unallocated.
                None if specs.iter().all(|spec| !spec.visible) => {
                    constraints[0] = Constraint::Min(0)
                }
                None => {}
            }
        }
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);
        let mut rects = [Rect::default(); PanelId::ALL.len()];
        rects.copy_from_slice(&rows);

        let mut splits = [None; PanelId::ALL.len() - 1];
        for index in 0..PanelId::ALL.len() - 1 {
            let (upper, lower) = (specs[index], specs[index + 1]);
            if !upper.visible
                || !lower.visible
                || upper.height.is_fixed()
                || lower.height.is_fixed()
            {
                continue;
            }
            splits[index] = Some(Split {
                above: PanelId::ALL[index],
                below: PanelId::ALL[index + 1],
                min_above: upper.height.floor(),
                min_below: lower.height.floor(),
            });
        }
        Panels {
            area: inner,
            rects,
            splits,
        }
    }

    /// The region the stack was laid out in, which the last card has to reach.
    pub fn area(&self) -> Rect {
        self.area
    }

    pub fn rect(&self, id: PanelId) -> Rect {
        self.rects[id.index()]
    }

    /// The cards in stack order, hidden ones included with the recessed rectangle they got.
    pub fn cards(&self) -> impl Iterator<Item = (PanelId, Rect)> + '_ {
        PanelId::ALL.iter().map(|id| (*id, self.rect(*id)))
    }

    pub fn splits(&self) -> impl Iterator<Item = Split> + '_ {
        self.splits.iter().flatten().copied()
    }

    /// The border under the pointer, if it is on one the frame can move.
    pub fn boundary_at(&self, column: u16, row: u16, heights: Heights) -> Option<Resize> {
        for split in self.splits() {
            let (above, below) = (self.rect(split.above), self.rect(split.below));
            if above.height < 2 || below.height < 2 {
                continue;
            }
            // The upper card's bottom rule and the lower card's top rule both count.
            let border = above.bottom().saturating_sub(1)..=below.y;
            if !border.contains(&row) || column < above.x || column >= above.right() {
                continue;
            }
            return Some(Resize {
                split,
                row,
                start_above: heights.get(split.above).unwrap_or(above.height),
                start_below: heights.get(split.below).unwrap_or(below.height),
            });
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::panels::{Height, PanelSpec};

    fn specs(run: bool, activity: bool, logs: bool) -> [PanelSpec; 3] {
        [
            PanelSpec::new(Height::Fixed(3), run),
            PanelSpec::new(Height::Fill { floor: 5 }, activity),
            PanelSpec::new(
                Height::Fraction {
                    num: 2,
                    den: 5,
                    min: 7,
                    max: 26,
                },
                logs,
            ),
        ]
    }

    #[test]
    fn the_stack_hands_the_leftover_to_the_card_that_asks_for_it() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, true, true),
            Heights::default(),
        );
        assert_eq!(
            panels.rect(PanelId::Status).height,
            3,
            "the Run card keeps its rows"
        );
        assert_eq!(
            panels.rect(PanelId::Logs).height,
            12,
            "the Logs card takes two fifths of the frame"
        );
        assert_eq!(
            panels.rect(PanelId::Activity).bottom(),
            panels.rect(PanelId::Logs).y,
            "the cards sit directly beside one another"
        );
        assert_eq!(
            panels.rect(PanelId::Activity).y,
            panels.rect(PanelId::Status).bottom(),
            "and the Activity card starts where the Run card ends"
        );
    }

    #[test]
    fn a_hidden_card_gets_no_rows_and_does_not_split() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, false, true),
            Heights::default(),
        );
        assert_eq!(
            panels.rect(PanelId::Activity).height,
            0,
            "no rows for a hidden card"
        );
        assert!(
            panels.splits().all(|split| split.above != PanelId::Status),
            "the Run card is fixed, so it is never a handle"
        );
        assert_eq!(
            panels.splits().count(),
            0,
            "Logs is the only card up under Run, and a hidden card has no border to drag"
        );
    }

    #[test]
    fn only_the_pairs_that_share_a_border_can_be_dragged() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, true, true),
            Heights::default(),
        );
        let split: Vec<(PanelId, PanelId)> = panels
            .splits()
            .map(|split| (split.above, split.below))
            .collect();
        assert_eq!(
            split,
            vec![(PanelId::Activity, PanelId::Logs)],
            "Activity to Logs is the frame's only moving border, never the fixed Run card"
        );
    }

    #[test]
    fn a_drag_trades_rows_within_its_pair_only() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, true, true),
            Heights::default(),
        );
        let mut heights = Heights::default();
        let grab = panels
            .boundary_at(5, panels.rect(PanelId::Logs).y, heights)
            .expect("the Logs border");
        grab.apply(&mut heights, grab.row + 2);
        assert_eq!(
            heights.get(PanelId::Logs),
            Some(10),
            "the Logs card gave two rows away"
        );
        assert_eq!(
            heights.get(PanelId::Activity),
            Some(panels.rect(PanelId::Activity).height + 2),
            "and the Activity card took them"
        );
        assert_eq!(
            heights.get(PanelId::Status),
            None,
            "the Run card is not in the pair"
        );
    }

    #[test]
    fn a_drag_cannot_shrink_a_card_below_its_floor() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, true, true),
            Heights::default(),
        );
        let mut heights = Heights::default();
        let grab = panels
            .boundary_at(5, panels.rect(PanelId::Logs).y, heights)
            .expect("the Logs border");
        grab.apply(&mut heights, grab.row + 200);
        assert_eq!(
            heights.get(PanelId::Logs),
            Some(7),
            "the Logs card stops at the floor the layout also refuses to go under"
        );
    }

    #[test]
    fn an_empty_stack_hands_the_frame_to_the_run_card() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(false, false, false),
            Heights::default(),
        );
        assert_eq!(
            panels.rect(PanelId::Status),
            Rect::new(0, 0, 100, 31),
            "with every card closed, the first one holds the rows so none go unallocated"
        );
    }

    #[test]
    fn a_stack_with_only_the_run_card_leaves_the_rest_blank() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, false, false),
            Heights::default(),
        );
        assert_eq!(
            panels.rect(PanelId::Status).height,
            3,
            "the Run card keeps its three rows even when it is the only card up"
        );
        for hidden in [PanelId::Activity, PanelId::Logs] {
            assert_eq!(
                panels.rect(hidden).height,
                0,
                "{hidden:?} is closed, so it gets no rows"
            );
        }
    }

    #[test]
    fn the_leftover_goes_to_the_last_card_when_no_card_fills() {
        let panels = Panels::layout(
            Rect::new(0, 0, 100, 31),
            &specs(true, false, true),
            Heights::default(),
        );
        assert_eq!(
            panels.rect(PanelId::Logs).bottom(),
            31,
            "with Activity hidden, the Logs card reaches the bottom of the stack"
        );
    }
}
