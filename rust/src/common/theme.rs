//! The shared palette.

use ratatui::style::Color;

pub const GREEN: Color = Color::Rgb(166, 218, 75);
pub const CYAN: Color = Color::Rgb(126, 231, 207);
pub const YELLOW: Color = Color::Rgb(255, 180, 84);
pub const RED: Color = Color::Rgb(255, 111, 125);
pub const BLUE: Color = Color::Rgb(65, 184, 255);
pub const PURPLE: Color = Color::Rgb(198, 160, 246);
pub const MUTED: Color = Color::Rgb(140, 140, 140);
/// Wash behind the marked selector row; the row keeps its own colours.
pub const SHADE: Color = Color::Rgb(48, 52, 54);

/// Frames of the running indicator. Motion is the only liveness signal: Nix reports no
/// fraction for a build or a transfer of unknown size.
pub const SPINNER_FRAMES: [&str; 8] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧"];
