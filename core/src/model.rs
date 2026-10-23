//! What the UI draws: a document turned into nodes, boxes and arrows with
//! their colours, shapes and line styles worked out, plus the edit forms.
//!
//!   Doc ── schema + %graph ──> Model { nodes, groups, edges, forms }
//!
//! The UI keeps no rules of its own; everything it shows comes from here.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::check::{Problem, check};
use crate::graph::{EDGE, GROUP, NODE, Settings, VIEW};
use crate::rec::{Doc, Record};
use crate::schema::{RecType, Schema, Type};

/// Node colours by rank, dark theme: the dev_tools.py level colours, then more.
pub const PALETTE: &[&str] = &[
    "#1f5f46", "#1f4f6f", "#5f4f1f", "#6f341f", "#5f1f4f", "#2f5f5f", "#4f2f6f", "#3f3f3f",
];
pub const PLAIN: &str = "#2a3140";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct NodeView {
    pub name: String,
    pub label: String,
    pub group: Option<String>,
    pub pos: Option<(i64, i64)>,
    pub color: String,
    pub shape: String,
    /// The node's tags (`%graph: tag F`), in file order.
    pub tags: Vec<String>,
    /// Nodes hidden below this one by a branch fold; set by branches().
    #[serde(default)]
    pub hidden: usize,
    /// Every field, in file order, for the side panel.
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GroupView {
    pub name: String,
    pub label: String,
    pub parent: Option<String>,
    pub color: Option<String>,
    /// `Collapsed: yes`: drawn as one node, its members hidden.
    pub collapsed: bool,
    /// Nodes hidden inside, at any depth; set by folded().
    pub hidden: usize,
    /// Where a folded box sits: the middle of its hidden members' Pos.
    pub pos: Option<(i64, i64)>,
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct EdgeView {
    /// Place among the Edge records: what an edit names it by.
    pub index: usize,
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub any: Option<String>,
    /// solid, dashed or dotted; an OR choice is always dashed.
    pub style: String,
    pub fields: Vec<(String, String)>,
}

/// A saved View record: how to look at the graph.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SavedView {
    pub name: String,
    pub layout: String,
    pub direction: Option<String>,
    pub root: Option<String>,
    /// Walk edges backwards (`Follow: in`).
    pub follow_in: bool,
    /// Edge kinds shown or followed; empty is every kind.
    pub edges: Vec<String>,
    pub collapse: Vec<String>,
    /// `Repeat: full`; the default is fold.
    pub full: bool,
    pub filter: Option<String>,
}

/// One input in an edit form.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Input {
    pub field: String,
    /// select, number, text or textarea.
    pub kind: String,
    pub options: Vec<String>,
    pub required: bool,
}

/// The field names the settings chose, so the UI can build edits.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Names {
    pub node_key: String,
    pub group_key: String,
    pub group: String,
    pub pos: String,
    pub from: String,
    pub to: String,
    pub any: String,
    /// The tag field, when the file has one.
    pub tag: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct Model {
    pub names: Names,
    pub nodes: Vec<NodeView>,
    pub groups: Vec<GroupView>,
    pub edges: Vec<EdgeView>,
    pub node_form: Vec<Input>,
    pub edge_form: Vec<Input>,
    pub group_form: Vec<Input>,
    /// The edge kind field and its values, for new edges and the legend.
    pub edge_kinds: Option<(String, Vec<String>)>,
    pub views: Vec<SavedView>,
    /// Every tag and how many nodes carry it, commonest first.
    pub tags: Vec<(String, usize)>,
    pub problems: Vec<Problem>,
}

/// The model of a text. A text that does not parse gives an empty model
/// carrying the parse problems.
pub fn model(text: &str) -> Model {
    let doc = match Doc::parse(text) {
        Ok(doc) => doc,
        Err(_) => {
            return Model {
                problems: crate::check::check_text(text),
                ..Model::default()
            };
        }
    };
    let mut sink = Vec::new();
    let schema = Schema::read(&doc, &mut sink);
    let s = Settings::read(&schema, &mut sink);
    let recs = doc.records();
    let of = |kind: &str| {
        recs.iter()
            .copied()
            .filter(move |r| !r.is_descriptor() && r.kind.as_deref() == Some(kind))
            .collect::<Vec<_>>()
    };
    let key = |kind: &str| schema.get(kind).and_then(|t| t.key.clone()).unwrap_or_else(|| "Name".to_string());
    let node_type = schema.get(NODE);

    // A colour field without an order (tags, words) colours by first value,
    // each distinct value its own palette colour, in the order they appear.
    let firsts: Vec<String> = s.color.as_ref().map_or_else(Vec::new, |f| {
        let mut seen: Vec<String> = Vec::new();
        for v in of(NODE).iter().filter_map(|r| r.get(f)) {
            if !seen.iter().any(|x| x == v) {
                seen.push(v.to_string());
            }
        }
        seen
    });

    let nodes: Vec<NodeView> = of(NODE)
        .into_iter()
        .filter_map(|r| {
            let name = r.get(&key(NODE))?.to_string();
            let label = s.label.iter().find_map(|f| r.get(f)).unwrap_or(&name).to_string();
            let pos = r.get(&s.pos).and_then(|p| {
                let (x, y) = p.split_once(' ')?;
                Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
            });
            Some(NodeView {
                label,
                group: r.get(&s.group).map(str::to_string),
                pos,
                color: r
                    .get("Color")
                    .map(str::to_string)
                    .unwrap_or_else(|| color(r, &s, node_type, &firsts)),
                shape: r.get("Shape").map(str::to_string).unwrap_or_else(|| shape(r, &s)),
                tags: s.tag.as_ref().map(|f| r.all(f).map(str::to_string).collect()).unwrap_or_default(),
                hidden: 0,
                fields: pairs(r),
                name,
            })
        })
        .collect();

    let groups = of(GROUP)
        .into_iter()
        .filter_map(|r| {
            let name = r.get(&key(GROUP))?.to_string();
            Some(GroupView {
                label: r.get("Label").unwrap_or(&name).to_string(),
                parent: r.get(&s.parent).map(str::to_string),
                color: r.get("Color").map(str::to_string),
                collapsed: r.get("Collapsed") == Some("yes"),
                hidden: 0,
                pos: None,
                fields: pairs(r),
                name,
            })
        })
        .collect();

    let edges = of(EDGE)
        .into_iter()
        .enumerate()
        .filter_map(|(index, r)| {
            let any = r.get(&s.any).map(str::to_string);
            let style = match (&any, &s.style) {
                (Some(_), _) => "dashed".to_string(),
                (None, Some((f, m))) => r.get(f).and_then(|k| m.get(k)).cloned().unwrap_or_else(|| "solid".to_string()),
                (None, None) => "solid".to_string(),
            };
            Some(EdgeView {
                index,
                from: r.get(&s.from)?.to_string(),
                to: r.get(&s.to)?.to_string(),
                label: r.get("Label").map(str::to_string),
                any,
                style,
                fields: pairs(r),
            })
        })
        .collect();

    let edge_kinds = s.style.as_ref().and_then(|(f, _)| match schema.get(EDGE)?.types.get(f)? {
        Type::Enum(values) => Some((f.clone(), values.clone())),
        _ => None,
    });

    let views = of(VIEW)
        .into_iter()
        .filter_map(|r| {
            Some(SavedView {
                name: r.get(&key(VIEW))?.to_string(),
                layout: r.get("Layout").unwrap_or("manual").to_string(),
                direction: r.get("Direction").map(str::to_string),
                root: r.get("Root").map(str::to_string),
                follow_in: r.get("Follow") == Some("in"),
                edges: r
                    .get("Edges")
                    .map(|e| e.split_whitespace().map(str::to_string).collect())
                    .unwrap_or_default(),
                collapse: r.all("Collapse").map(str::to_string).collect(),
                full: r.get("Repeat") == Some("full"),
                filter: r.get("Filter").map(str::to_string),
            })
        })
        .collect();

    let mut tags: Vec<(String, usize)> = Vec::new();
    for t in nodes.iter().flat_map(|n| &n.tags) {
        match tags.iter_mut().find(|(x, _)| x == t) {
            Some((_, n)) => *n += 1,
            None => tags.push((t.clone(), 1)),
        }
    }
    tags.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    Model {
        views,
        tags,
        names: Names {
            node_key: key(NODE),
            group_key: key(GROUP),
            group: s.group.clone(),
            pos: s.pos.clone(),
            from: s.from.clone(),
            to: s.to.clone(),
            any: s.any.clone(),
            tag: s.tag.clone(),
        },
        nodes,
        groups,
        edges,
        node_form: form(schema.get(NODE)),
        edge_form: form(schema.get(EDGE)),
        group_form: form(schema.get(GROUP)),
        edge_kinds,
        problems: check(&doc),
    }
}

fn pairs(r: &Record) -> Vec<(String, String)> {
    r.fields().map(|f| (f.name.clone(), f.value.clone())).collect()
}

/// Colour from the `%graph: color F` field: by rank when F is ordered (L1
/// green, L2 blue, ...), else by the first value's place among `firsts`.
fn color(r: &Record, s: &Settings, t: Option<&RecType>, firsts: &[String]) -> String {
    let rank = s.color.as_ref().and_then(|f| {
        let v = r.get(f)?;
        let Some(ty) = t.and_then(|t| t.types.get(f)).filter(|ty| ty.rank(v).is_some()) else {
            return firsts.iter().position(|x| x == v);
        };
        let low = match ty {
            Type::Range(lo, _) => *lo as f64,
            _ => 0.0,
        };
        ty.rank(v).map(|x| (x - low).max(0.0) as usize)
    });
    rank.map_or(PLAIN, |i| PALETTE[i % PALETTE.len()]).to_string()
}

fn shape(r: &Record, s: &Settings) -> String {
    s.shape
        .as_ref()
        .and_then(|(f, m)| r.get(f).and_then(|v| m.get(v)).cloned())
        .unwrap_or_else(|| "round".to_string())
}

/// The edit form a descriptor implies: each allowed field, typed.
fn form(t: Option<&RecType>) -> Vec<Input> {
    let Some(t) = t else { return Vec::new() };
    let mut names: Vec<String> = t.key.iter().cloned().collect();
    for f in t.allowed.iter().flatten().chain(&t.mandatory) {
        if !names.contains(f) {
            names.push(f.clone());
        }
    }
    names
        .into_iter()
        .map(|field| {
            let (kind, options) = match t.types.get(&field) {
                Some(Type::Enum(values)) => ("select", values.clone()),
                Some(Type::Bool) => ("select", vec!["yes".into(), "no".into()]),
                Some(Type::Range(lo, hi)) if hi - lo <= 20 => ("select", (*lo..=*hi).map(|v| v.to_string()).collect()),
                Some(Type::Int | Type::Real | Type::Range(..)) => ("number", Vec::new()),
                Some(Type::Line | Type::Regexp(_) | Type::Rec(_)) => ("text", Vec::new()),
                None if field == "Note" => ("textarea", Vec::new()),
                None => ("text", Vec::new()),
            };
            Input {
                required: t.mandatory.contains(&field) || t.key.as_ref() == Some(&field),
                field,
                kind: kind.to_string(),
                options,
            }
        })
        .collect()
}

/// The model as the graph view draws it: a collapsed group becomes one
/// node, everything inside it is hidden, and edges to hidden nodes meet the
/// group instead.
///
///   [Backend ▸ 4] ──> Release builds      Backend folded: the edges from its
///                                          four members merge into one
///
/// Inside nested folds the outermost collapsed group stands for all.
pub fn folded(m: &Model) -> Model {
    let parent = |g: &str| m.groups.iter().find(|x| x.name == g).and_then(|x| x.parent.clone());
    let collapsed = |g: &str| m.groups.iter().any(|x| x.name == g && x.collapsed);

    // The outermost collapsed group around a group (itself included).
    let cover = |g: &str| -> Option<String> {
        let mut at = Some(g.to_string());
        let mut out = None;
        for _ in 0..=m.groups.len() {
            let Some(x) = at else { break };
            if collapsed(&x) {
                out = Some(x.clone());
            }
            at = parent(&x); // bounded: a Parent loop stops here, check reports it
        }
        out
    };

    // Where each hidden name now shows: node or inner group -> its cover.
    let mut stand: HashMap<String, String> = HashMap::new();
    for n in &m.nodes {
        if let Some(c) = n.group.as_deref().and_then(cover) {
            stand.insert(n.name.clone(), c);
        }
    }
    for g in &m.groups {
        if let Some(c) = g.parent.as_deref().and_then(cover) {
            stand.insert(g.name.clone(), c);
        }
    }

    let mut out = m.clone();
    out.nodes.retain(|n| !stand.contains_key(&n.name));
    out.groups.retain(|g| !stand.contains_key(&g.name));
    for g in out.groups.iter_mut().filter(|g| g.collapsed) {
        let inside: Vec<&NodeView> = m.nodes.iter().filter(|n| stand.get(&n.name) == Some(&g.name)).collect();
        g.hidden = inside.len();
        let placed: Vec<(i64, i64)> = inside.iter().filter_map(|n| n.pos).collect();
        if !placed.is_empty() {
            let len = placed.len() as i64;
            g.pos = Some((
                placed.iter().map(|p| p.0).sum::<i64>() / len,
                placed.iter().map(|p| p.1).sum::<i64>() / len,
            ));
        }
    }

    let at = |name: &str| stand.get(name).cloned().unwrap_or_else(|| name.to_string());
    let mut seen = HashSet::new();
    out.edges = m
        .edges
        .iter()
        .filter_map(|e| {
            let (from, to) = (at(&e.from), at(&e.to));
            if from == to || !seen.insert((from.clone(), to.clone(), e.style.clone())) {
                return None; // inside one fold, or merged into an edge already drawn
            }
            let merged = from != e.from || to != e.to;
            Some(EdgeView {
                from,
                to,
                label: if merged { None } else { e.label.clone() },
                ..e.clone()
            })
        })
        .collect();
    out
}

/// The graph with some branches folded: each folded node keeps its place
/// and hides what lies downstream of it, shown as "Python ▸ 9".
///
///   Bash ─> Python ─> JSON ─> jq       fold Python: JSON, jq and SQL go,
///             └────> SQL               unless another path still reaches them
///
/// A node stays when an edge reaches it from a node that is shown and not
/// folded, so folding one parent never hides what a second parent unlocks.
pub fn branches(m: &Model, folds: &[String]) -> Model {
    let folded = |n: &str| folds.iter().any(|f| f == n);

    // Everything downstream of a fold, the folded nodes themselves excepted.
    let mut below: HashSet<String> = HashSet::new();
    let mut todo: Vec<String> = folds.to_vec();
    while let Some(at) = todo.pop() {
        for e in m.edges.iter().filter(|e| e.from == at) {
            if !folded(&e.to) && below.insert(e.to.clone()) {
                todo.push(e.to.clone());
            }
        }
    }

    // Bring back what an open, shown node still reaches, until nothing moves.
    loop {
        let back: Vec<String> = below
            .iter()
            .filter(|n| m.edges.iter().any(|e| &e.to == *n && !below.contains(&e.from) && !folded(&e.from)))
            .cloned()
            .collect();
        if back.is_empty() {
            break;
        }
        for n in back {
            below.remove(&n);
        }
    }

    let mut out = m.clone();
    for n in out.nodes.iter_mut().filter(|n| folded(&n.name)) {
        // Count what this fold hides: hidden nodes downstream of it.
        let mut seen: HashSet<String> = HashSet::new();
        let mut todo = vec![n.name.clone()];
        while let Some(at) = todo.pop() {
            for e in m.edges.iter().filter(|e| e.from == at && below.contains(&e.to)) {
                if seen.insert(e.to.clone()) {
                    todo.push(e.to.clone());
                }
            }
        }
        n.hidden = seen.len();
    }
    out.nodes.retain(|n| !below.contains(&n.name));
    out.edges.retain(|e| !below.contains(&e.from) && !below.contains(&e.to));

    // A box left with nothing in it goes too.
    let mut keep: HashSet<String> = out.nodes.iter().filter_map(|n| n.group.clone()).collect();
    keep.extend(out.groups.iter().filter(|g| g.collapsed).map(|g| g.name.clone()));
    for _ in 0..=m.groups.len() {
        let up: Vec<String> = out
            .groups
            .iter()
            .filter(|g| keep.contains(&g.name))
            .filter_map(|g| g.parent.clone())
            .collect();
        keep.extend(up);
    }
    out.groups.retain(|g| keep.contains(&g.name));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../example/draw_graph_example.rec");

    #[test]
    fn example_model() {
        let m = model(EXAMPLE);
        assert_eq!((m.nodes.len(), m.groups.len(), m.edges.len()), (31, 9, 43));
        assert!(m.problems.is_empty());

        let rust = m.nodes.iter().find(|n| n.name == "Rust").unwrap();
        assert_eq!(
            (rust.color.as_str(), rust.shape.as_str(), rust.pos),
            (PALETTE[3], "round", Some((600, 0))),
            "L4 colour, tool shape"
        );

        let tts = m.nodes.iter().find(|n| n.name == "Show problems").unwrap();
        assert_eq!(tts.color, "#8a2a2a", "a node's own Color wins");

        let or: Vec<&EdgeView> = m.edges.iter().filter(|e| e.any.is_some()).collect();
        assert!(or.len() == 2 && or.iter().all(|e| e.style == "dashed"));
        assert!(m.edges.iter().any(|e| e.style == "dotted"), "relates edges are dotted");

        let level = m.node_form.iter().find(|i| i.field == "Level").unwrap();
        assert_eq!((level.kind.as_str(), level.options.len()), ("select", 5));
        assert_eq!(m.edge_kinds.as_ref().map(|(f, v)| (f.as_str(), v.len())), Some(("Kind", 4)));

        let build = m.views.iter().find(|v| v.name == "Build tree").unwrap();
        assert_eq!(
            (build.layout.as_str(), build.root.as_deref(), build.collapse.len()),
            ("tree", Some("Bash"), 1)
        );
        assert!(
            m.views
                .iter()
                .find(|v| v.name == "What Tree view needs")
                .is_some_and(|v| v.follow_in && v.full)
        );
    }

    #[test]
    fn a_folded_branch_hides_what_only_it_reaches() {
        let m = model(EXAMPLE);

        // Cytoscape.js is the only way in to ELK.js, Vim keymap and SVG
        // export, and Tree view needs only those: all four go.
        let f = branches(&m, &["Cytoscape.js".to_string()]);
        let shown = |n: &str| f.nodes.iter().any(|x| x.name == n);
        assert!(shown("Cytoscape.js") && !shown("ELK.js") && !shown("SVG export") && !shown("Vim keymap"));
        assert!(shown("JavaScript") && shown("TypeScript"), "upstream stays");
        assert!(shown("Release builds"), "CLI check and File watcher still reach it");
        let cy = f.nodes.iter().find(|n| n.name == "Cytoscape.js").unwrap();
        assert_eq!(cy.hidden, 4, "ELK.js, Vim keymap, Tree view, SVG export");
        assert!(f.edges.iter().all(|e| e.from != "ELK.js" && e.to != "ELK.js"));

        // Everything Rust unlocks has another way in: Tauri through
        // TypeScript and JavaScript, rec parser through recutils.
        let f = branches(&m, &["Rust".to_string()]);
        assert!(f.nodes.iter().any(|x| x.name == "Tauri") && f.nodes.iter().any(|x| x.name == "rec parser"));
        assert_eq!(f.nodes.iter().find(|n| n.name == "Rust").unwrap().hidden, 0);
    }

    #[test]
    fn tags_are_listed_with_counts() {
        let m = model(EXAMPLE);
        let bash = m.nodes.iter().find(|n| n.name == "Bash").unwrap();
        assert_eq!(bash.tags, vec!["shell", "cli"]);
        assert_eq!(&m.tags[..2], &[("web".to_string(), 4), ("language".to_string(), 3)]);
        assert_eq!(m.names.tag.as_deref(), Some("Tag"));
    }

    #[test]
    fn an_unordered_colour_field_colours_by_first_value() {
        let text = EXAMPLE.replace("%graph: color Level\n", "%graph: color Tag\n");
        let m = model(&text);
        let colour = |name: &str| m.nodes.iter().find(|n| n.name == name).unwrap().color.clone();
        assert_eq!(colour("HTML"), colour("CSS"), "both start with web");
        assert_ne!(colour("Bash"), colour("HTML"));
        assert_eq!(colour("Tauri"), PLAIN, "no tag, no colour");
    }

    #[test]
    fn folded_groups_stand_for_their_members() {
        // Canvas is collapsed in the example: its 3 tasks fold into it.
        let m = model(EXAMPLE);
        let f = folded(&m);
        let canvas = f.groups.iter().find(|g| g.name == "Canvas").unwrap();
        assert_eq!(canvas.hidden, 3);
        assert_eq!(canvas.pos, Some((1133, 233)), "the middle of Vim keymap, Tree view and SVG export");
        assert!(!f.nodes.iter().any(|n| n.group.as_deref() == Some("Canvas")));
        assert!(
            f.edges.iter().any(|e| e.from == "Canvas" && e.to == "Release builds"),
            "Tree view -> Release builds now leaves the box"
        );
        assert!(f.edges.iter().all(|e| e.from != "Tree view" && e.to != "Vim keymap"));

        // Folding the outer box hides the inner one too.
        let mut m2 = m.clone();
        m2.groups.iter_mut().find(|g| g.name == "App").unwrap().collapsed = true;
        let f2 = folded(&m2);
        assert!(!f2.groups.iter().any(|g| g.name == "Canvas" || g.name == "Backend"));
        assert_eq!(
            f2.groups.iter().find(|g| g.name == "App").unwrap().hidden,
            8,
            "4 Backend tasks, 3 Canvas tasks, Release builds"
        );
    }
}
