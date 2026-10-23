//! The tree view: a graph walked from one root, as a tree that folds.
//!
//!   Tauri ─┬─ File watcher ─── Release builds        <- first copy, in full
//!          └─ Tree view ─┬─ Release builds  ↑        <- later copy: folded,
//!                        └─ Mind maps                   "same as above"
//!
//! A node reached a second time (several parents) is a repeat. With
//! `Repeat: fold` (the default) only its first copy shows its branch, so
//! copies never multiply down a chain; `full` shows every copy in full.
//! A node already on the path from the root closes a loop and stops there.
//!
//! Folds are by node name: folding a node folds every copy of it. A repeat
//! copy the user opens by hand is kept by its item id.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::model::Model;

/// What a name looks like: its label, colour and shape.
type Look = (String, String, String);

/// Room per depth level and per leaf, in px.
pub const STEP_X: f64 = 230.0;
pub const STEP_Y: f64 = 56.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Spec {
    pub root: String,
    /// Walk edges backwards: what the root needs, not what it unlocks.
    pub follow_in: bool,
    /// Edge kinds to follow; empty follows every edge.
    pub kinds: Vec<String>,
    /// Show every repeat in full (`Repeat: full`).
    pub full: bool,
    /// Folded node names (a view's Collapse lines).
    pub collapse: Vec<String>,
    /// Repeat copies opened by hand, by item id.
    pub open: Vec<String>,
    /// right, down, left or up: where children grow.
    pub direction: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Item {
    /// `t:0.2.1`: the path of child indexes from the root.
    pub id: String,
    pub name: String,
    pub label: String,
    pub color: String,
    pub shape: String,
    pub depth: usize,
    pub parent: Option<String>,
    pub x: f64,
    pub y: f64,
    /// Children hidden by a fold or a repeat; 0 when open or a leaf.
    pub hidden: usize,
    /// A later copy of a node shown above; `first` is that copy's id.
    pub repeat: bool,
    pub first: Option<String>,
    /// Closes a loop: the node is already on the path from the root.
    pub looped: bool,
    /// The edge from the parent: its index, style and label.
    pub edge: Option<(usize, String, Option<String>)>,
}

/// The children of a name along the followed edges, in file order.
fn children<'a>(m: &'a Model, spec: &Spec, name: &str) -> Vec<(&'a str, usize, String, Option<String>)> {
    let kind_field = m.edge_kinds.as_ref().map(|(f, _)| f.as_str());
    m.edges
        .iter()
        .filter(|e| if spec.follow_in { e.to == name } else { e.from == name })
        .filter(|e| {
            spec.kinds.is_empty()
                || kind_field
                    .and_then(|f| e.fields.iter().find(|(k, _)| k == f))
                    .is_some_and(|(_, v)| spec.kinds.contains(v))
        })
        .map(|e| {
            (
                if spec.follow_in { e.from.as_str() } else { e.to.as_str() },
                e.index,
                e.style.clone(),
                e.label.clone(),
            )
        })
        .collect()
}

/// Walk the tree and place it. Empty when the root is not a node or group.
pub fn tree(m: &Model, spec: &Spec) -> Vec<Item> {
    let look = |name: &str| -> Option<Look> {
        if let Some(n) = m.nodes.iter().find(|n| n.name == name) {
            return Some((n.label.clone(), n.color.clone(), n.shape.clone()));
        }
        let g = m.groups.iter().find(|g| g.name == name)?;
        Some((
            g.label.clone(),
            g.color.clone().unwrap_or_else(|| crate::model::PLAIN.to_string()),
            "box".to_string(),
        ))
    };
    if look(&spec.root).is_none() {
        return Vec::new();
    }

    let folded: HashSet<&str> = spec.collapse.iter().map(String::as_str).collect();
    let open: HashSet<&str> = spec.open.iter().map(String::as_str).collect();
    let mut first: HashMap<String, String> = HashMap::new();
    let mut items: Vec<Item> = Vec::new();

    // Depth first, so the first copy of a node is the one read first.
    struct Walk<'a> {
        m: &'a Model,
        spec: &'a Spec,
        folded: &'a HashSet<&'a str>,
        open: &'a HashSet<&'a str>,
    }
    #[allow(clippy::too_many_arguments)]
    fn visit(
        w: &Walk,
        look: &dyn Fn(&str) -> Option<Look>,
        name: &str,
        id: String,
        parent: Option<String>,
        edge: Option<(usize, String, Option<String>)>,
        path: &mut Vec<String>,
        first: &mut HashMap<String, String>,
        items: &mut Vec<Item>,
    ) {
        let Some((label, color, shape)) = look(name) else { return };
        let kids = children(w.m, w.spec, name);
        let looped = path.iter().any(|p| p == name);
        let earlier = first.get(name).cloned();
        let repeat = earlier.is_some() && !w.spec.full && !w.open.contains(id.as_str());
        let fold = w.folded.contains(name);
        let closed = looped || repeat || fold;
        if earlier.is_none() {
            first.insert(name.to_string(), id.clone());
        }

        items.push(Item {
            id: id.clone(),
            name: name.to_string(),
            label,
            color,
            shape,
            depth: path.len(),
            parent,
            x: 0.0,
            y: 0.0,
            hidden: if closed { kids.len() } else { 0 },
            repeat: earlier.is_some() && !looped,
            first: earlier,
            looped,
            edge,
        });
        if closed {
            return;
        }

        path.push(name.to_string());
        for (i, (child, index, style, label)) in kids.into_iter().enumerate() {
            visit(
                w,
                look,
                child,
                format!("{id}.{i}"),
                Some(id.clone()),
                Some((index, style, label)),
                path,
                first,
                items,
            );
        }
        path.pop();
    }

    let w = Walk {
        m,
        spec,
        folded: &folded,
        open: &open,
    };
    visit(
        &w,
        &look,
        &spec.root,
        "t:0".to_string(),
        None,
        None,
        &mut Vec::new(),
        &mut first,
        &mut items,
    );
    place(&mut items, &spec.direction);
    items
}

/// A tidy tree: each leaf its own row, each parent centred on its children,
/// depth along the direction the tree grows.
fn place(items: &mut [Item], direction: &str) {
    let mut row = 0.0;
    let mut across: HashMap<String, f64> = HashMap::new();

    // Children come after their parent, so walking backwards places
    // every child before its parent.
    let kids: HashMap<String, Vec<String>> = items.iter().fold(HashMap::new(), |mut m, i| {
        if let Some(p) = &i.parent {
            m.entry(p.clone()).or_default().push(i.id.clone());
        }
        m
    });
    for i in items.iter() {
        if !kids.contains_key(&i.id) {
            across.insert(i.id.clone(), row);
            row += 1.0;
        }
    }
    for i in items.iter().rev() {
        if let Some(ks) = kids.get(&i.id) {
            let ys: Vec<f64> = ks.iter().filter_map(|k| across.get(k)).copied().collect();
            let mid = (ys.iter().cloned().fold(f64::INFINITY, f64::min) + ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max)) / 2.0;
            across.insert(i.id.clone(), mid);
        }
    }

    for i in items.iter_mut() {
        let along = i.depth as f64;
        let side = across.get(&i.id).copied().unwrap_or(0.0);
        (i.x, i.y) = match direction {
            "down" => (side * STEP_X * 0.75, along * STEP_Y * 2.0),
            "up" => (side * STEP_X * 0.75, -along * STEP_Y * 2.0),
            "left" => (-along * STEP_X, side * STEP_Y),
            _ => (along * STEP_X, side * STEP_Y),
        };
    }
}

/// The edge kind a quick tree from `root` follows: the commonest kind among
/// the edges it would walk. Empty when the file has no kind field.
pub fn common_kind(m: &Model, root: &str, follow_in: bool) -> Vec<String> {
    let Some((field, kinds)) = &m.edge_kinds else { return Vec::new() };
    let near: Vec<&str> = m
        .edges
        .iter()
        .filter(|e| if follow_in { e.to == root } else { e.from == root })
        .filter_map(|e| e.fields.iter().find(|(f, _)| f == field).map(|(_, v)| v.as_str()))
        .collect();
    kinds
        .iter()
        .max_by_key(|k| near.iter().filter(|n| **n == k.as_str()).count())
        .filter(|k| near.contains(&k.as_str()))
        .cloned()
        .into_iter()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::model;

    const EXAMPLE: &str = include_str!("../../example/draw_graph_example.rec");

    fn spec(root: &str, kinds: &[&str]) -> Spec {
        Spec {
            root: root.into(),
            kinds: kinds.iter().map(|k| k.to_string()).collect(),
            direction: "right".into(),
            ..Spec::default()
        }
    }

    fn names(items: &[Item]) -> Vec<&str> {
        items.iter().map(|i| i.name.as_str()).collect()
    }

    #[test]
    fn repeats_fold_after_the_first_copy() {
        let m = model(EXAMPLE);
        let items = tree(&m, &spec("JavaScript", &["requires"]));

        // From JavaScript, Release builds is reached through Tree view and
        // through File watcher: in full once, folded the second time.
        let copies: Vec<&Item> = items.iter().filter(|i| i.name == "Release builds").collect();
        assert_eq!(copies.len(), 2, "{:?}", names(&items));
        assert!(!copies[0].repeat && copies[1].repeat);
        assert_eq!(copies[1].first.as_deref(), Some(copies[0].id.as_str()));
    }

    #[test]
    fn full_shows_every_copy() {
        let m = model(EXAMPLE);
        let mut s = spec("Cytoscape.js", &["requires"]);
        let folded = tree(&m, &s);
        s.full = true;
        let full = tree(&m, &s);
        assert!(full.len() > folded.len(), "Tree view is reached twice; full repeats its branch");
        assert!(full.iter().all(|i| !i.repeat || i.hidden == 0));
    }

    #[test]
    fn follow_in_walks_prerequisites() {
        let m = model(EXAMPLE);
        let mut s = spec("Tree view", &["requires"]);
        s.follow_in = true;
        let items = tree(&m, &s);
        for want in ["ELK.js", "Vim keymap", "Cytoscape.js", "JavaScript", "HTML"] {
            assert!(names(&items).contains(&want), "{want} is a prerequisite: {:?}", names(&items));
        }
    }

    #[test]
    fn collapse_hides_children_and_counts_them() {
        let m = model(EXAMPLE);
        let mut s = spec("draw_graph", &["branch"]);
        let open = tree(&m, &s);
        s.collapse = vec!["draw_graph".into()];
        let shut = tree(&m, &s);
        assert_eq!(shut.len(), 1);
        assert_eq!(shut[0].hidden, open.len() - 1);
    }

    #[test]
    fn a_loop_stops_where_it_closes() {
        let m = model(EXAMPLE);
        let items = tree(&m, &spec("Edit made", &["flow"]));
        let back: Vec<&Item> = items.iter().filter(|i| i.name == "Edit made").collect();
        assert_eq!(back.len(), 2, "the root, and the loop back to it");
        assert!(back[1].looped && back[1].hidden > 0);
    }

    #[test]
    fn parents_sit_between_their_children() {
        let m = model(EXAMPLE);
        let items = tree(&m, &spec("draw_graph", &["branch"]));
        let root = &items[0];
        let ys: Vec<f64> = items.iter().filter(|i| i.parent.as_deref() == Some("t:0")).map(|i| i.y).collect();
        let (lo, hi) = (
            ys.iter().cloned().fold(f64::MAX, f64::min),
            ys.iter().cloned().fold(f64::MIN, f64::max),
        );
        assert_eq!(root.y, (lo + hi) / 2.0);
        assert!(items.iter().skip(1).all(|i| i.x == STEP_X));
    }

    #[test]
    fn quick_tree_kind_is_the_commonest() {
        let m = model(EXAMPLE);
        assert_eq!(common_kind(&m, "Rust", false), vec!["requires".to_string()]);
        assert_eq!(common_kind(&m, "draw_graph", false), vec!["branch".to_string()]);
    }
}
