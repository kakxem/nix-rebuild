//! The run selector `nix-rebuild runs` opens, outside the frame: the rows the caller reads, the
//! search that narrows them, and the sortable columns. Reading and deleting them is the
//! records slice's.

mod state;
mod view;

pub use state::{PickerStep, Selector, pick_run, picker_mouse};
#[cfg(test)]
pub use view::picker_to_string;
pub use view::{
    PICKER_CHIPS, PICKER_MARKER, PICKER_SEARCH_CHIPS, PickerChip, PickerGeometry, render_picker,
};

#[cfg(test)]
mod tests;
