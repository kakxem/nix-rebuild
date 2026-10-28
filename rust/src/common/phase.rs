//! What the run is doing right now: the badge the cards read.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Preparing,
    Evaluating,
    Building,
    Activating,
    Cleanup,
    Complete,
    Failed,
    Cancelling,
    Cancelled,
}

impl Phase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preparing => "preparing",
            Self::Evaluating => "evaluating",
            Self::Building => "building",
            Self::Activating => "activating",
            Self::Cleanup => "cleaning",
            Self::Complete => "complete",
            Self::Failed => "failed",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn is_running(self) -> bool {
        matches!(
            self,
            Self::Preparing | Self::Evaluating | Self::Building | Self::Activating | Self::Cleanup
        )
    }

    /// Work a frame is doing on its own; `Preparing` also covers a frame with nothing to run.
    pub fn is_working(self) -> bool {
        matches!(
            self,
            Self::Evaluating | Self::Building | Self::Activating | Self::Cleanup
        )
    }
}
