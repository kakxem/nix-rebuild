//! What happens to old generations after a rebuild.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CleanupPolicy {
    /// Ask in the TUI; skip with an explanation when there is no terminal.
    #[default]
    Ask,
    /// Clean without asking (`--yes`).
    Always,
    /// Never clean (`--no-clean`).
    Never,
}

impl CleanupPolicy {
    /// The short form shown in the run header and the details panel.
    pub fn label(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Never => "off",
        }
    }

    pub fn runs_unattended(self) -> bool {
        matches!(self, Self::Always)
    }

    pub fn is_disabled(self) -> bool {
        matches!(self, Self::Never)
    }

    /// As `meta.conf` and the settings file write it, and as both read it back.
    pub fn stored(self) -> &'static str {
        match self {
            Self::Ask => "ask",
            Self::Always => "always",
            Self::Never => "never",
        }
    }

    pub fn from_stored(text: &str) -> Self {
        match text {
            "always" => Self::Always,
            "never" => Self::Never,
            _ => Self::Ask,
        }
    }
}
