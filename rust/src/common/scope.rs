//! What a run rebuilds: the system configuration, the user's home, or both.

use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    System,
    Home,
    /// Both configurations at once: built but not activated.
    Both,
}

impl Scope {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "NixOS",
            Self::Home => "Home Manager",
            Self::Both => "Home + System",
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::System => "system",
            Self::Home => "home",
            Self::Both => "both",
        })
    }
}

impl FromStr for Scope {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "system" => Ok(Self::System),
            "home" => Ok(Self::Home),
            "both" => Ok(Self::Both),
            _ => Err(format!("unknown rebuild scope: {value}")),
        }
    }
}
