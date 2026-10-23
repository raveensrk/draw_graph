//! draw_graph: draw and edit a recfile as a graph.
//!
//! The library is the part the app and `draw_graph check` share:
//! [`rec`] reads and writes the text, [`schema`] and [`graph`] say what it
//! means, and [`check`] finds its problems.

pub mod check;
pub mod graph;
pub mod rec;
pub mod schema;
