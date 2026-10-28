//! How far a multi-step run has got: the label and the count the Run card shows.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StepProgress {
    pub index: usize,
    pub total: usize,
    pub label: String,
}
