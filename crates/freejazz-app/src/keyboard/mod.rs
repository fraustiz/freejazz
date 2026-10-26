//! The keyboard drawn on screen: physical layout, key labels from the Windows layout,
//! key groups and the selection every section works on.

pub mod groups;
pub mod labels;
pub mod layout;
pub mod selection;
pub mod view;

pub use groups::Group;
pub use layout::{KeyDef, Layout};
pub use view::{KeyLook, KeyboardView};
