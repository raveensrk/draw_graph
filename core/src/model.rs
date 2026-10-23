//! What the UI draws: a document turned into nodes, boxes and arrows with
//! their colours, shapes and line styles worked out, plus the edit forms.
//!
//!   Doc ── schema + %graph ──> Model { nodes, groups, edges, forms }
//!
//! The UI keeps no rules of its own; everything it shows comes from here.

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
    /// Every field, in file order, for the side panel.
    pub fields: Vec<(String, String)>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct GroupView {
    pub name: String,
    pub label: String,
    pub parent: Option<String>,
    pub color: Option<String>,
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

    let nodes = of(NODE)
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
                color: r.get("Color").map(str::to_string).unwrap_or_else(|| color(r, &s, node_type)),
                shape: r.get("Shape").map(str::to_string).unwrap_or_else(|| shape(r, &s)),
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

    Model {
        views,
        names: Names {
            node_key: key(NODE),
            group_key: key(GROUP),
            group: s.group.clone(),
            pos: s.pos.clone(),
            from: s.from.clone(),
            to: s.to.clone(),
            any: s.any.clone(),
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

/// Colour from the `%graph: color F` field's rank: L1 green, L2 blue, ...
fn color(r: &Record, s: &Settings, t: Option<&RecType>) -> String {
    let rank = s.color.as_ref().and_then(|f| {
        let ty = t?.types.get(f)?;
        let v = r.get(f)?;
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
}
