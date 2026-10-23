//! draw_graph core: the part the app, the UI and `draw_graph check` share.
//!
//! [`rec`] reads and writes the text, [`schema`] and [`graph`] say what it
//! means, [`check`] finds its problems, [`edit`] changes it behind that
//! check, [`model`] turns it into what the UI draws, and [`tree`] walks it as a
//! folding tree.

pub mod check;
pub mod edit;
pub mod graph;
pub mod model;
pub mod rec;
pub mod schema;
pub mod tree;
