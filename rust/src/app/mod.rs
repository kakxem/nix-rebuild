//! The composition root: the shared model, the root reduce, the frame and its event loop.
//! It reads every slice; no slice reads it.
#[cfg(test)]
pub mod testing;

pub mod dashboard;
pub mod model;
pub mod reduce;
pub mod runtime;
