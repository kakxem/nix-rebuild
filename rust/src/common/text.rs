//! Text the frame draws: truncation to terminal cells, the run clock and the animation tick.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::common::theme::MUTED;

/// Truncate a line to `width` terminal cells, cutting with an ellipsis. Measured in cells,
/// never bytes: a Nerd Font glyph is one cell but several bytes, and the last cell holds the ellipsis.
pub fn clip_line(line: Line<'static>, width: u16) -> Line<'static> {
    let width = width as usize;
    if width == 0 {
        return Line::default();
    }
    let total: usize = line
        .spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if total <= width {
        return line;
    }

    let budget = width.saturating_sub(1);
    let mut spans = Vec::new();
    let mut used = 0;
    for span in line.spans {
        if used >= budget {
            break;
        }
        let mut kept = String::new();
        for ch in span.content.chars() {
            let cell = UnicodeWidthChar::width(ch).unwrap_or(0);
            if used + cell > budget {
                break;
            }
            used += cell;
            kept.push(ch);
        }
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
    }
    spans.push(Span::styled("…", Style::default().fg(MUTED)));
    Line::from(spans)
}

pub fn animation_tick() -> usize {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .checked_div(100)
        .unwrap_or_default() as usize
}

pub fn format_clock(duration: std::time::Duration) -> String {
    let seconds = duration.as_secs();
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// A byte count in the largest readable unit; plain bytes get no decimal.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
