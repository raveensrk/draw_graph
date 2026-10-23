//! Edits the app sends, applied to the text and gated by the check.
//!
//!   UI ── Edit (JSON) ──> apply(text, edit) ── new text ──> file
//!                              │
//!                              └── adds a problem? refused, file untouched
//!
//! A record is named by its key (`Node` + `Rust`); an edge, which has no
//! key, by its place among the Edge records. A file that already has
//! problems can still be edited: only problems the edit adds refuse it.

use serde::{Deserialize, Serialize};

use crate::check::{Problem, check};
use crate::graph::{EDGE, GROUP, NODE, Settings, VIEW};
use crate::rec::Doc;
use crate::schema::Schema;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Set one field of a keyed record; an empty value removes the field.
    Set {
        kind: String,
        key: String,
        field: String,
        value: String,
    },
    /// Set one field of the n-th Edge record; empty removes it.
    SetEdge {
        index: usize,
        field: String,
        value: String,
    },
    AddNode {
        fields: Vec<(String, String)>,
    },
    AddEdge {
        fields: Vec<(String, String)>,
    },
    /// Delete a node and every edge touching it; drop it from view folds.
    DeleteNode {
        name: String,
    },
    DeleteEdge {
        index: usize,
    },
    /// Rename a node or group and every reference to it.
    Rename {
        kind: String,
        name: String,
        to: String,
    },
    /// Canvas positions, `x y` in px.
    Move {
        positions: Vec<(String, i64, i64)>,
    },
    /// Several edits, checked as one.
    Batch {
        edits: Vec<Edit>,
    },
}

/// Apply an edit to the text. Ok holds the new text; Err the problems it
/// would add (or why it cannot apply).
pub fn apply(text: &str, edit: &Edit) -> Result<String, Vec<Problem>> {
    let refuse = |message: String| vec![Problem::new(0, "edit".to_string(), &message, None)];
    let mut doc = Doc::parse(text).map_err(|errors| refuse(format!("the file does not parse: {}", errors[0])))?;
    let before = check(&doc);

    run(&mut doc, edit).map_err(refuse)?;

    // Same problem = same subject and message; lines move with an edit.
    let after = check(&doc);
    let added: Vec<Problem> = after
        .into_iter()
        .filter(|p| !before.iter().any(|b| b.subject == p.subject && b.message == p.message))
        .collect();
    if !added.is_empty() {
        return Err(added);
    }
    Ok(doc.text())
}

fn run(doc: &mut Doc, edit: &Edit) -> Result<(), String> {
    let mut sink = Vec::new();
    let schema = Schema::read(doc, &mut sink);
    let s = Settings::read(&schema, &mut sink);
    let order = |kind: &str| schema.get(kind).and_then(|t| t.allowed.clone()).unwrap_or_default();
    let key = |kind: &str| schema.get(kind).and_then(|t| t.key.clone()).unwrap_or_else(|| "Name".to_string());

    match edit {
        Edit::Set {
            kind,
            key: name,
            field,
            value,
        } => {
            let n = find(doc, kind, &key(kind), name)?;
            set(doc, n, field, value, &order(kind))
        }
        Edit::SetEdge { index, field, value } => {
            let n = edge(doc, *index)?;
            set(doc, n, field, value, &order(EDGE))
        }
        Edit::AddNode { fields } => insert(doc, NODE, fields),
        Edit::AddEdge { fields } => insert(doc, EDGE, fields),
        Edit::DeleteNode { name } => {
            let n = find(doc, NODE, &key(NODE), name)?;
            doc.delete(n);

            // Its edges go with it, last first so indexes stay right.
            let touching: Vec<usize> = records_of(doc, EDGE)
                .into_iter()
                .filter(|(_, r)| r.get(&s.from) == Some(name) || r.get(&s.to) == Some(name))
                .map(|(n, _)| n)
                .collect();
            for n in touching.into_iter().rev() {
                doc.delete(n);
            }
            for (n, _) in records_of(doc, VIEW) {
                doc.remove(n, "Collapse", Some(name));
            }
            Ok(())
        }
        Edit::DeleteEdge { index } => {
            let n = edge(doc, *index)?;
            doc.delete(n);
            Ok(())
        }
        Edit::Rename { kind, name, to } => {
            let n = find(doc, kind, &key(kind), name)?;
            doc.set(n, &key(kind), to);

            // References: edge ends, node groups, group parents, views.
            let refs: Vec<(usize, String)> = doc
                .records()
                .iter()
                .enumerate()
                .filter(|(_, r)| !r.is_descriptor())
                .flat_map(|(n, r)| {
                    let fields: Vec<&str> = match r.kind.as_deref() {
                        Some(EDGE) => vec![&s.from, &s.to],
                        Some(NODE) if kind == GROUP => vec![&s.group],
                        Some(GROUP) if kind == GROUP => vec![&s.parent],
                        Some(VIEW) => vec!["Root"],
                        _ => vec![],
                    };
                    fields
                        .into_iter()
                        .filter(|f| r.get(f) == Some(name))
                        .map(move |f| (n, f.to_string()))
                        .collect::<Vec<_>>()
                })
                .collect();
            for (n, field) in refs {
                doc.set(n, &field, to);
            }
            for (n, r) in records_of(doc, VIEW) {
                if r.all("Collapse").any(|c| c == name) {
                    doc.remove(n, "Collapse", Some(name));
                    doc.add(n, "Collapse", to);
                }
            }
            Ok(())
        }
        Edit::Move { positions } => {
            for (name, x, y) in positions {
                let n = find(doc, NODE, &key(NODE), name)?;
                doc.set_ordered(n, &s.pos, &format!("{x} {y}"), &order(NODE));
            }
            Ok(())
        }
        Edit::Batch { edits } => {
            for e in edits {
                run(doc, e)?;
            }
            Ok(())
        }
    }
}

fn set(doc: &mut Doc, n: usize, field: &str, value: &str, order: &[String]) -> Result<(), String> {
    if value.is_empty() {
        doc.remove(n, field, None);
        return Ok(());
    }
    doc.set_ordered(n, field, value, order);
    Ok(())
}

fn insert(doc: &mut Doc, kind: &str, fields: &[(String, String)]) -> Result<(), String> {
    if fields.is_empty() {
        return Err(format!("a new {kind} needs fields"));
    }
    let pairs: Vec<(&str, &str)> = fields
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    doc.insert(kind, &pairs);
    Ok(())
}

/// (index in records(), record) for each record of a kind.
fn records_of(doc: &Doc, kind: &str) -> Vec<(usize, crate::rec::Record)> {
    doc.records()
        .into_iter()
        .enumerate()
        .filter(|(_, r)| !r.is_descriptor() && r.kind.as_deref() == Some(kind))
        .map(|(n, r)| (n, r.clone()))
        .collect()
}

fn find(doc: &Doc, kind: &str, key: &str, name: &str) -> Result<usize, String> {
    records_of(doc, kind)
        .into_iter()
        .find(|(_, r)| r.get(key) == Some(name))
        .map(|(n, _)| n)
        .ok_or_else(|| format!("no {kind} named {name:?}"))
}

fn edge(doc: &Doc, index: usize) -> Result<usize, String> {
    records_of(doc, EDGE)
        .get(index)
        .map(|(n, _)| *n)
        .ok_or_else(|| format!("no edge {index}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../../example/draw_graph_example.rec");

    fn node(name: &str) -> Edit {
        Edit::AddNode {
            fields: vec![
                ("Name".into(), name.into()),
                ("Kind".into(), "idea".into()),
                ("Group".into(), "Ideas".into()),
            ],
        }
    }

    #[test]
    fn set_changes_one_record() {
        // L3 still sits between Bash (L1) and Tauri (L4), so rise allows it.
        let edit = Edit::Set {
            kind: "Node".into(),
            key: "Rust".into(),
            field: "Level".into(),
            value: "3".into(),
        };
        let text = apply(EXAMPLE, &edit).unwrap();
        let changed: Vec<(&str, &str)> = EXAMPLE.lines().zip(text.lines()).filter(|(a, b)| a != b).collect();
        assert_eq!(changed, vec![("Level: 4", "Level: 3")]);
    }

    #[test]
    fn add_node_and_edge_then_delete() {
        let text = apply(EXAMPLE, &node("Undo history")).unwrap();
        let edge = Edit::AddEdge {
            fields: vec![
                ("From".into(), "draw_graph".into()),
                ("To".into(), "Undo history".into()),
                ("Kind".into(), "branch".into()),
            ],
        };
        let text = apply(&text, &edge).unwrap();
        assert!(text.contains("From: draw_graph\nTo: Undo history\nKind: branch\n"));

        let text = apply(
            &text,
            &Edit::DeleteNode {
                name: "Undo history".into(),
            },
        )
        .unwrap();
        assert_eq!(text, EXAMPLE, "deleting the node takes its edge too and restores the file");
    }

    #[test]
    fn edit_that_adds_a_problem_is_refused() {
        let edit = Edit::Set {
            kind: "Node".into(),
            key: "Bash".into(),
            field: "Level".into(),
            value: "9".into(),
        };
        let problems = apply(EXAMPLE, &edit).unwrap_err();
        assert!(
            problems
                .iter()
                .any(|p| p.message.contains("\"9\" is not a whole number from 1 to 5")),
            "{problems:?}"
        );
    }

    #[test]
    fn rename_updates_every_reference() {
        let edit = Edit::Rename {
            kind: "Node".into(),
            name: "rec parser".into(),
            to: "Rec IO".into(),
        };
        let text = apply(EXAMPLE, &edit).unwrap();

        // Every field that names it follows; prose in a Note does not.
        let refs = |t: &str, name: &str| t.lines().filter(|l| l.split_once(": ").is_some_and(|(_, v)| v == name)).count();
        assert_eq!(refs(&text, "rec parser"), 0);
        assert_eq!(refs(&text, "Rec IO"), refs(EXAMPLE, "rec parser"));
        assert_eq!(refs(EXAMPLE, "rec parser"), 8, "Name, 3 From, 3 To, 1 Collapse");
    }

    #[test]
    fn rename_group_updates_members_and_children() {
        let edit = Edit::Rename {
            kind: "Group".into(),
            name: "Frontend".into(),
            to: "Web view".into(),
        };
        let text = apply(EXAMPLE, &edit).unwrap();
        assert!(text.contains("Name: Web view\n") && text.contains("Parent: Web view\n"));
    }

    #[test]
    fn move_writes_pos_in_schema_order() {
        let text = apply(
            EXAMPLE,
            &Edit::Move {
                positions: vec![("Bash".into(), 5, -7)],
            },
        )
        .unwrap();
        assert!(text.contains(
            "Name: Bash\nKind: tool\nLevel: 1\nTier: base\nGroup: Languages\nNote: Also stands for shell scripting.\nPos: 5 -7\n"
        ));
    }

    #[test]
    fn empty_value_removes_a_field() {
        let edit = Edit::Set {
            kind: "Node".into(),
            key: "Bash".into(),
            field: "Note".into(),
            value: String::new(),
        };
        let text = apply(EXAMPLE, &edit).unwrap();
        assert!(!text.contains("Also stands for shell scripting."));
    }

    #[test]
    fn batch_is_checked_as_one() {
        // Alone, the edge to a missing node is refused; with the node first, fine.
        let edge = Edit::AddEdge {
            fields: vec![
                ("From".into(), "draw_graph".into()),
                ("To".into(), "Themes".into()),
                ("Kind".into(), "branch".into()),
            ],
        };
        assert!(apply(EXAMPLE, &edge).is_err());
        assert!(
            apply(
                EXAMPLE,
                &Edit::Batch {
                    edits: vec![node("Themes"), edge]
                }
            )
            .is_ok()
        );
    }

    #[test]
    fn edits_cross_as_json() {
        // The shape the UI sends: {"op":"move","positions":[["Bash",1,2]]}.
        let json = r#"{"op":"move","positions":[["Bash",1,2]]}"#;
        let edit: Edit = serde_json::from_str(json).unwrap();
        assert_eq!(
            edit,
            Edit::Move {
                positions: vec![("Bash".into(), 1, 2)]
            }
        );
        assert_eq!(serde_json::to_string(&edit).unwrap(), json);
    }
}
