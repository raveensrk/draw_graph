//! What the canvas draws, as plain data: nodes at positions, boxes around
//! their members, edges between ids. The canvas paints it and the SVG
//! export writes it, so the two always match.
//!
//!   Model ── graph(layout) ──┐
//!   tree items ── tree() ────┴──> Scene { nodes, boxes, edges }
//!
//! Ids: `n:Name` a node, `g:Name` a box (or a folded box drawn as a node),
//! `t:0.1` a tree item, `e:3` an edge, `te:t:0.1` a tree edge.

use std::collections::HashMap;

use draw_graph_core::model::Model;
use draw_graph_core::tree::{Item, Spec, tree};

/// The two graph layouts: saved positions, or a tidy tree of the whole graph.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Layout {
    Manual,
    Tree,
}

impl Layout {
    pub fn name(self) -> &'static str {
        match self {
            Layout::Manual => "manual",
            Layout::Tree => "tree",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    pub id: String,
    /// The record it shows: a node or group name.
    pub name: String,
    pub label: String,
    pub fill: String,
    /// box, round, diamond or ellipse.
    pub shape: String,
    pub x: f32,
    pub y: f32,
    /// Dashed outline: folded, or a repeat that hides its branch.
    pub dashed: bool,
    /// The first line of its Note, shortened: drawn under the label.
    pub note: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub id: String,
    pub label: String,
    pub parent: Option<String>,
    /// Ids of the nodes directly inside.
    pub members: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Edge {
    pub id: String,
    pub from: String,
    pub to: String,
    /// solid, dashed or dotted.
    pub style: String,
    pub label: Option<String>,
    /// Drawn as an elbow, with no arrowhead (tree view).
    pub elbow: bool,
    /// The first line of its Note, shortened: drawn by the edge.
    pub note: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Scene {
    pub nodes: Vec<Node>,
    pub boxes: Vec<Frame>,
    pub edges: Vec<Edge>,
}

/// Marks drawn after labels. egui's own fonts lack the nice ones, so the
/// window passes `fancy` only when it loaded a font that has them.
pub struct Marks {
    pub fold: &'static str,
    pub repeat: &'static str,
    pub looped: &'static str,
}

pub const FANCY: Marks = Marks {
    fold: "▸",
    repeat: "↑",
    looped: "↺",
};
pub const PLAIN: Marks = Marks {
    fold: "+",
    repeat: "^",
    looped: "@",
};

/// How much of a note the canvas shows: its first line, this many characters.
const NOTE_CHARS: usize = 40;

/// A note as the canvas shows it: the first line, cut to NOTE_CHARS with "…".
pub fn brief(note: &str) -> Option<String> {
    let line = note.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    if line.chars().count() <= NOTE_CHARS {
        return Some(line.to_string());
    }
    Some(format!("{}…", line.chars().take(NOTE_CHARS - 1).collect::<String>().trim_end()))
}

/// The Note field of a record's fields, shortened.
fn note_of(fields: &[(String, String)]) -> Option<String> {
    fields.iter().find(|(k, _)| k == "Note").and_then(|(_, v)| brief(v))
}

/// Room between tree columns and rows, in graph units.
const GAP: f32 = 30.0;

/// Grid for nodes with no Pos in the manual layout, in graph units.
const GRID: (f32, f32) = (200.0, 60.0);
const GRID_COLS: usize = 8;

/// Where every node of a model sits in a layout, by name.
pub fn positions(m: &Model, layout: Layout) -> HashMap<String, (f32, f32)> {
    let mut at: HashMap<String, (f32, f32)> = HashMap::new();
    if layout == Layout::Tree {
        // A forest: one tree per node nothing leads into, stacked downward;
        // each node at its first copy.
        let roots: Vec<&str> = m
            .nodes
            .iter()
            .filter(|n| !m.edges.iter().any(|e| e.to == n.name))
            .map(|n| n.name.as_str())
            .collect();
        let mut top = 0.0f32;
        for root in roots {
            let spec = Spec {
                root: root.to_string(),
                direction: "right".into(),
                ..Spec::default()
            };
            let items = tree(m, &spec);
            let bottom = items.iter().map(|i| i.y as f32).fold(0.0, f32::max);
            for i in items.iter().filter(|i| !i.repeat && !i.looped) {
                at.entry(i.name.clone()).or_insert((i.x as f32, i.y as f32 + top));
            }
            top += bottom + GAP * 3.0;
        }
    }
    let mut loose = 0usize;
    for n in &m.nodes {
        if at.contains_key(&n.name) {
            continue;
        }
        let p = match (layout, n.pos) {
            (Layout::Manual, Some((x, y))) => (x as f32, y as f32),
            _ => {
                loose += 1;
                let i = loose - 1;
                (GRID.0 * (i % GRID_COLS) as f32, -GRID.1 * (1 + i / GRID_COLS) as f32)
            }
        };
        at.insert(n.name.clone(), p);
    }
    for g in m.groups.iter().filter(|g| g.collapsed) {
        if let Some((x, y)) = g.pos {
            at.insert(g.name.clone(), (x as f32, y as f32));
        }
    }
    at
}

/// The graph view: the (already folded and filtered) model in a layout.
/// The tree layout leaves boxes out: it places nodes by the edges, so
/// members of one box can land far apart.
pub fn graph(m: &Model, layout: Layout, marks: &Marks) -> Scene {
    let at = positions(m, layout);
    let id_of = |name: &str| {
        if m.nodes.iter().any(|n| n.name == name) {
            format!("n:{name}")
        } else {
            format!("g:{name}")
        }
    };

    let mut nodes: Vec<Node> = m
        .nodes
        .iter()
        .map(|n| {
            let (x, y) = at.get(&n.name).copied().unwrap_or_default();
            let label = if n.hidden > 0 {
                format!("{} {}{}", n.label, marks.fold, n.hidden)
            } else {
                n.label.clone()
            };
            Node {
                id: format!("n:{}", n.name),
                name: n.name.clone(),
                label,
                fill: n.color.clone(),
                shape: n.shape.clone(),
                x,
                y,
                dashed: n.hidden > 0,
                note: note_of(&n.fields),
            }
        })
        .collect();
    for g in m.groups.iter().filter(|g| g.collapsed) {
        let (x, y) = at.get(&g.name).copied().unwrap_or_default();
        nodes.push(Node {
            id: format!("g:{}", g.name),
            name: g.name.clone(),
            label: format!("{} {}{}", g.label, marks.fold, g.hidden),
            fill: g.color.clone().unwrap_or_else(|| "#3a4150".into()),
            shape: "round".into(),
            x,
            y,
            dashed: true,
            note: note_of(&g.fields),
        });
    }

    let boxes = if layout == Layout::Tree {
        Vec::new()
    } else {
        m.groups
            .iter()
            .filter(|g| !g.collapsed)
            .map(|g| Frame {
                id: format!("g:{}", g.name),
                label: g.label.clone(),
                parent: g.parent.as_ref().map(|p| format!("g:{p}")),
                members: m
                    .nodes
                    .iter()
                    .filter(|n| n.group.as_deref() == Some(g.name.as_str()))
                    .map(|n| format!("n:{}", n.name))
                    .chain(
                        m.groups
                            .iter()
                            .filter(|c| c.collapsed && c.parent.as_deref() == Some(g.name.as_str()))
                            .map(|c| format!("g:{}", c.name)),
                    )
                    .collect(),
            })
            .collect()
    };

    let edges = m
        .edges
        .iter()
        .map(|e| Edge {
            id: format!("e:{}", e.index),
            from: id_of(&e.from),
            to: id_of(&e.to),
            style: e.style.clone(),
            label: e.label.clone(),
            elbow: false,
            note: note_of(&e.fields),
        })
        .collect();
    Scene { nodes, boxes, edges }
}

/// The tree view: items placed by core::tree, joined parent to child.
pub fn tree_scene(items: &[Item], m: &Model, marks: &Marks) -> Scene {
    let nodes = items
        .iter()
        .map(|i| {
            let mark = if i.looped {
                format!(" {}", marks.looped)
            } else if i.repeat {
                format!(" {}", marks.repeat)
            } else if i.hidden > 0 {
                format!(" {}{}", marks.fold, i.hidden)
            } else {
                String::new()
            };
            Node {
                id: i.id.clone(),
                name: i.name.clone(),
                label: format!("{}{mark}", i.label),
                fill: i.color.clone(),
                shape: i.shape.clone(),
                x: i.x as f32,
                y: i.y as f32,
                dashed: i.repeat || i.looped,
                note: m.nodes.iter().find(|n| n.name == i.name).and_then(|n| note_of(&n.fields)),
            }
        })
        .collect();
    let edges = items
        .iter()
        .filter_map(|i| {
            let parent = i.parent.clone()?;
            let (style, label) = i
                .edge
                .as_ref()
                .map(|(_, s, l)| (s.clone(), l.clone()))
                .unwrap_or(("solid".into(), None));
            Some(Edge {
                id: format!("te:{}", i.id),
                from: parent,
                to: i.id.clone(),
                style,
                label,
                elbow: true,
                note: i
                    .edge
                    .as_ref()
                    .and_then(|(index, _, _)| m.edges.iter().find(|e| e.index == *index))
                    .and_then(|e| note_of(&e.fields)),
            })
        })
        .collect();
    Scene {
        nodes,
        boxes: Vec::new(),
        edges,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use draw_graph_core::model::model;

    const EXAMPLE: &str = include_str!("../../example/draw_graph_example.rec");

    #[test]
    fn manual_uses_saved_positions() {
        let s = graph(&model(EXAMPLE), Layout::Manual, &PLAIN);
        let rust = s.nodes.iter().find(|n| n.id == "n:Rust").unwrap();
        assert_eq!((rust.x, rust.y), (600.0, 0.0));
        assert!(
            s.boxes
                .iter()
                .any(|b| b.id == "g:Languages" && b.members.contains(&"n:Rust".to_string()))
        );
        assert!(
            s.edges.iter().any(|e| e.from == "n:Rule engine" && e.to == "g:Save flow"),
            "an edge onto a box keeps the box id"
        );
    }

    #[test]
    fn tree_layout_places_every_node_once_and_drops_boxes() {
        // As the window passes it: folded first. Canvas is collapsed in the
        // example, so its 3 tasks show as one box node.
        let m = draw_graph_core::model::folded(&model(EXAMPLE));
        let s = graph(&m, Layout::Tree, &PLAIN);
        assert_eq!(s.nodes.len(), m.nodes.len() + 1);
        assert!(s.boxes.is_empty());
        let spots: std::collections::HashSet<(i64, i64)> = s.nodes.iter().map(|n| (n.x as i64, n.y as i64)).collect();
        assert_eq!(spots.len(), s.nodes.len(), "no two nodes on one spot");
    }

    #[test]
    fn notes_are_shortened_to_their_first_line() {
        assert_eq!(brief("one line"), Some("one line".into()));
        assert_eq!(brief("first\nsecond"), Some("first".into()));
        assert_eq!(brief("   \nsecond"), None);
        let long = brief("a note that goes on and on well past forty characters").unwrap();
        assert!(long.ends_with('…') && long.chars().count() <= 40, "{long}");

        let s = graph(&model(EXAMPLE), Layout::Manual, &PLAIN);
        let bash = s.nodes.iter().find(|n| n.id == "n:Bash").unwrap();
        assert_eq!(bash.note.as_deref(), Some("Also stands for shell scripting."));
        let js = s
            .edges
            .iter()
            .find_map(|e| e.note.clone().filter(|n| n.starts_with("Plain JavaScript works")))
            .unwrap();
        assert!(js.ends_with('…') && js.chars().count() == 40, "{js}");
    }

    #[test]
    fn tree_items_become_nodes_and_elbows() {
        let m = model(EXAMPLE);
        let spec = Spec {
            root: "draw_graph".into(),
            kinds: vec!["branch".into()],
            direction: "right".into(),
            ..Spec::default()
        };
        let s = tree_scene(&tree(&m, &spec), &m, &FANCY);
        assert_eq!(s.nodes.len(), 7);
        assert!(s.edges.iter().all(|e| e.elbow && e.from == "t:0"));
    }
}
