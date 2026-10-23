//! The side panel: the selected record's form, or the file summary.
//!
//! Forms come from the schema (model.rs `Input`): an enum or small range is
//! a dropdown, Note a text box, the rest one line. A field is saved when it
//! loses focus, or on Enter; the Name field renames the record and every
//! reference to it. A repeated field (Collapse) is shown, not edited.
//! Edits go to `app.outbox` and are sent after the frame.

use draw_graph_core::edit::Edit;
use draw_graph_core::model::Input;
use eframe::egui::{self, Color32, RichText};

use crate::app::App;
use crate::draw::{ACCENT, BAD, MUTED};
use crate::scene::brief;

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        if !app.refused.is_empty() {
            ui.label(RichText::new("REFUSED").color(MUTED).small());
            for p in &app.refused {
                ui.label(RichText::new(p.to_string()).color(BAD));
            }
            ui.separator();
        }
        match app.record() {
            Some(r) if r.starts_with("n:") => node(app, ui, &r[2..]),
            Some(r) if r.starts_with("g:") => group(app, ui, &r[2..]),
            Some(r) if r.starts_with("e:") => edge(app, ui, r[2..].parse().unwrap_or(usize::MAX)),
            _ => summary(app, ui),
        }
    });
}

fn heading(ui: &mut egui::Ui, title: &str, sub: &str) {
    ui.heading(title);
    ui.label(RichText::new(sub).color(MUTED));
    ui.add_space(4.0);
}

fn section(ui: &mut egui::Ui, title: String) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).color(MUTED).small());
}

/// Inputs for one record; `save(field, value)` builds the edit.
fn form(
    app: &mut App,
    ui: &mut egui::Ui,
    record: &str,
    inputs: &[Input],
    values: &[(String, String)],
    skip: Option<&str>,
    save: &dyn Fn(String, String) -> Option<(Edit, Option<String>)>,
) {
    // Fresh buffers when the record or the file changed.
    if app.buffers_for != (record.to_string(), app.version) {
        app.buffers = values.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        app.buffers_for = (record.to_string(), app.version);
    }
    let value = |f: &str| values.iter().find(|(k, _)| k == f).map(|(_, v)| v.clone()).unwrap_or_default();
    let count = |f: &str| values.iter().filter(|(k, _)| k == f).count();
    let mut all: Vec<Input> = inputs.iter().filter(|i| Some(i.field.as_str()) != skip).cloned().collect();
    for (k, _) in values {
        if Some(k.as_str()) != skip && !all.iter().any(|i| &i.field == k) {
            all.push(Input {
                field: k.clone(),
                kind: "text".into(),
                options: Vec::new(),
                required: false,
            });
        }
    }

    let focus = std::mem::take(&mut app.focus_panel);
    egui::Grid::new(("form", record))
        .num_columns(2)
        .spacing([10.0, 6.0])
        .show(ui, |ui| {
            for (n, i) in all.iter().enumerate() {
                let name = if i.required { format!("{} *", i.field) } else { i.field.clone() };
                ui.label(RichText::new(name).color(MUTED));
                let now = value(&i.field);
                if count(&i.field) > 1 {
                    let joined = values
                        .iter()
                        .filter(|(k, _)| *k == i.field)
                        .map(|(_, v)| v.as_str())
                        .collect::<Vec<_>>()
                        .join(", ");
                    ui.label(RichText::new(joined).color(MUTED))
                        .on_hover_text("a repeated field: edit it in the file");
                } else if i.kind == "select" {
                    let mut pick = now.clone();
                    egui::ComboBox::from_id_salt((record, &i.field))
                        .selected_text(if pick.is_empty() { "—".to_string() } else { pick.clone() })
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            if !i.required {
                                ui.selectable_value(&mut pick, String::new(), "—");
                            }
                            for o in &i.options {
                                ui.selectable_value(&mut pick, o.clone(), o);
                            }
                        });
                    if pick != now
                        && let Some(e) = save(i.field.clone(), pick)
                    {
                        app.outbox.push(e);
                    }
                } else {
                    let buf = app.buffers.entry(i.field.clone()).or_insert_with(|| now.clone());
                    let edit = if i.kind == "textarea" {
                        egui::TextEdit::multiline(buf).desired_rows(3)
                    } else {
                        egui::TextEdit::singleline(buf)
                    };
                    let r = ui.add(edit.desired_width(200.0));
                    if focus && n == 0 {
                        r.request_focus();
                    }
                    if r.lost_focus()
                        && *buf != now
                        && let Some(e) = save(i.field.clone(), buf.trim().to_string())
                    {
                        app.outbox.push(e);
                    }
                }
                ui.end_row();
            }
        });
}

fn node(app: &mut App, ui: &mut egui::Ui, name: &str) {
    let Some(n) = app.full.nodes.iter().find(|n| n.name == name).cloned() else {
        ui.label(RichText::new(format!("{name} is gone")).color(MUTED));
        return;
    };
    heading(ui, &n.label, "Node · Enter edits, Esc returns");

    // Tags as chips: the name filters, × removes, the field adds on Enter.
    if let Some(field) = app.full.names.tag.clone() {
        section(ui, format!("TAGS ({})", n.tags.len()));
        let save = |values: Vec<String>| Edit::SetList {
            kind: "Node".into(),
            key: n.name.clone(),
            field: field.clone(),
            values,
        };
        ui.horizontal_wrapped(|ui| {
            for t in &n.tags {
                ui.group(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    let name = egui::Label::new(t)
                        .sense(egui::Sense::click())
                        .wrap_mode(egui::TextWrapMode::Extend);
                    if ui.add(name).on_hover_text("show only this tag").clicked() {
                        app.tag_filter = Some(t.clone());
                        app.rebuild();
                    }
                    if ui.small_button("×").on_hover_text("remove this tag").clicked() {
                        app.outbox.push((save(n.tags.iter().filter(|x| *x != t).cloned().collect()), None));
                    }
                });
            }
        });
        let add = ui.memory_mut(|m| {
            m.data
                .get_temp_mut_or_default::<String>(egui::Id::new(("add tag", &n.name)))
                .clone()
        });
        let mut text = add;
        let r = ui.add(
            egui::TextEdit::singleline(&mut text)
                .hint_text("add a tag, Enter (# from the canvas)")
                .desired_width(f32::INFINITY),
        );
        if std::mem::take(&mut app.focus_tag) {
            r.request_focus();
        }
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let new = text.trim().to_string();
        if enter && !new.is_empty() && !n.tags.contains(&new) {
            let mut all = n.tags.clone();
            all.push(new);
            app.outbox.push((save(all), None));
            text.clear();
        }
        ui.memory_mut(|m| *m.data.get_temp_mut_or_default::<String>(egui::Id::new(("add tag", &n.name))) = text);
    }

    section(ui, "FIELDS".into());
    let (inputs, key, me) = (app.full.node_form.clone(), app.full.names.node_key.clone(), n.name.clone());
    let skip = app.full.names.tag.clone();
    let save = move |field: String, value: String| -> Option<(Edit, Option<String>)> {
        if field == key {
            return (!value.is_empty()).then(|| {
                (
                    Edit::Rename {
                        kind: "Node".into(),
                        name: me.clone(),
                        to: value.clone(),
                    },
                    Some(format!("n:{value}")),
                )
            });
        }
        Some((
            Edit::Set {
                kind: "Node".into(),
                key: me.clone(),
                field,
                value,
            },
            None,
        ))
    };
    form(app, ui, &format!("n:{}", n.name), &inputs, &n.fields, skip.as_deref(), &save);

    // Links in two panes: what leads here, and what this leads to.
    let links = |incoming: bool| -> Vec<Link> {
        app.full
            .edges
            .iter()
            .filter(|e| if incoming { e.to == n.name } else { e.from == n.name })
            .map(|e| {
                let other = if incoming { &e.from } else { &e.to };
                let kind = e
                    .label
                    .clone()
                    .or_else(|| e.fields.iter().find(|(f, _)| f == "Kind").map(|(_, v)| v.clone()));
                Link {
                    index: e.index,
                    other: other.clone(),
                    kind,
                    note: e.fields.iter().find(|(f, _)| f == "Note").and_then(|(_, v)| brief(v)),
                    any: e.any.is_some(),
                }
            })
            .collect()
    };
    let (ins, outs) = (links(true), links(false));
    ui.add_space(8.0);
    pane(ui, &format!("INCOMING ({}) · what leads here", ins.len()), |ui| {
        link_rows(app, ui, &ins, "←")
    });
    ui.add_space(6.0);
    pane(ui, &format!("OUTGOING ({}) · what this leads to", outs.len()), |ui| {
        link_rows(app, ui, &outs, "→")
    });
}

/// One edge as a node's panel lists it.
struct Link {
    index: usize,
    other: String,
    kind: Option<String>,
    note: Option<String>,
    any: bool,
}

/// A framed pane with a small title, full width.
fn pane(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new(title).color(MUTED).small());
        body(ui);
    });
}

/// Rows of links: the other end (click to go there), the kind, the note, and
/// a button that opens the edge itself.
fn link_rows(app: &mut App, ui: &mut egui::Ui, links: &[Link], arrow: &str) {
    if links.is_empty() {
        ui.label(RichText::new("none").color(MUTED));
        return;
    }
    for l in links {
        ui.horizontal_wrapped(|ui| {
            ui.label(arrow);
            if ui.link(&l.other).on_hover_text("go to it").clicked() {
                let kind = if app.full.nodes.iter().any(|n| n.name == l.other) {
                    "n"
                } else {
                    "g"
                };
                app.selected = Some(format!("{kind}:{}", l.other));
            }
            let mut kind = l.kind.clone().unwrap_or_default();
            if l.any {
                kind.push_str(" (or)");
            }
            if !kind.is_empty() {
                ui.label(RichText::new(kind).color(MUTED));
            }
            if ui.small_button("link").on_hover_text("open this edge").clicked() {
                app.selected = Some(format!("e:{}", l.index));
            }
        });
        if let Some(note) = &l.note {
            ui.label(RichText::new(format!("   {note}")).color(MUTED).small());
        }
    }
}

fn edge(app: &mut App, ui: &mut egui::Ui, index: usize) {
    let Some(e) = app.full.edges.iter().find(|e| e.index == index).cloned() else {
        ui.label(RichText::new("that edge is gone").color(MUTED));
        return;
    };
    let or = e.any.as_ref().map(|a| format!(" · OR group {a}")).unwrap_or_default();
    heading(ui, &format!("{} → {}", e.from, e.to), &format!("Edge · {}{or}", e.style));
    let inputs = app.full.edge_form.clone();
    let save = move |field: String, value: String| Some((Edit::SetEdge { index, field, value }, None));
    form(app, ui, &format!("e:{index}"), &inputs, &e.fields, None, &save);
    // Each end in its own pane: what it is, its tags and note.
    for (title, name) in [("FROM · the prerequisite end", &e.from), ("TO · the end it leads to", &e.to)] {
        ui.add_space(6.0);
        pane(ui, title, |ui| end(app, ui, name));
    }
}

/// One end of an edge: a node or a group, with its tags and note.
fn end(app: &mut App, ui: &mut egui::Ui, name: &str) {
    let node = app.full.nodes.iter().find(|n| n.name == name).cloned();
    let (kind, label, tags, fields) = match (&node, app.full.groups.iter().find(|g| g.name == name)) {
        (Some(n), _) => ("n", n.label.clone(), n.tags.clone(), n.fields.clone()),
        (None, Some(g)) => ("g", format!("{} (box)", g.label), Vec::new(), g.fields.clone()),
        (None, None) => {
            ui.label(RichText::new(format!("{name}: no such node")).color(BAD));
            return;
        }
    };
    if ui.link(RichText::new(label).strong()).on_hover_text("go to it").clicked() {
        app.selected = Some(format!("{kind}:{name}"));
    }
    if !tags.is_empty() {
        ui.label(RichText::new(tags.join(" · ")).color(MUTED).small());
    }
    if let Some(note) = fields.iter().find(|(f, _)| f == "Note").and_then(|(_, v)| brief(v)) {
        ui.label(RichText::new(note).color(MUTED));
    }
}

fn group(app: &mut App, ui: &mut egui::Ui, name: &str) {
    let Some(g) = app.full.groups.iter().find(|g| g.name == name).cloned() else {
        ui.label(RichText::new(format!("{name} is gone")).color(MUTED));
        return;
    };
    heading(
        ui,
        &g.label,
        if g.collapsed {
            "Group · folded (za opens it)"
        } else {
            "Group · za folds it"
        },
    );
    let (inputs, key, me) = (app.full.group_form.clone(), app.full.names.group_key.clone(), g.name.clone());
    let save = move |field: String, value: String| -> Option<(Edit, Option<String>)> {
        if field == key {
            return (!value.is_empty()).then(|| {
                (
                    Edit::Rename {
                        kind: "Group".into(),
                        name: me.clone(),
                        to: value.clone(),
                    },
                    Some(format!("g:{value}")),
                )
            });
        }
        Some((
            Edit::Set {
                kind: "Group".into(),
                key: me.clone(),
                field,
                value,
            },
            None,
        ))
    };
    form(app, ui, &format!("g:{}", g.name), &inputs, &g.fields, None, &save);
    let members: Vec<String> = app
        .full
        .nodes
        .iter()
        .filter(|n| n.group.as_deref() == Some(name))
        .map(|n| n.name.clone())
        .collect();
    section(ui, format!("MEMBERS ({})", members.len()));
    for m in members {
        if ui.selectable_label(false, &m).clicked() {
            app.selected = Some(format!("n:{m}"));
        }
    }
}

fn summary(app: &mut App, ui: &mut egui::Ui) {
    let file = app.file.path.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string();
    heading(ui, &file, &app.file.path.display().to_string());
    ui.label(format!(
        "{} nodes, {} edges, {} groups",
        app.full.nodes.len(),
        app.full.edges.len(),
        app.full.groups.len()
    ));
    ui.label(RichText::new("Nothing selected. Arrows or a click pick a node; ? lists every key.").color(MUTED));

    section(ui, format!("PROBLEMS ({})", app.full.problems.len()));
    for p in &app.full.problems {
        ui.label(RichText::new(format!("line {}: {p}", p.line)).color(BAD));
    }

    section(ui, format!("TAGS ({})", app.full.tags.len()));
    let mut pick = None;
    for (t, n) in &app.full.tags {
        let on = app.tag_filter.as_deref() == Some(t.as_str());
        let text = RichText::new(format!("{t}  ({n})")).color(if on { ACCENT } else { Color32::PLACEHOLDER });
        if ui
            .selectable_label(on, text)
            .on_hover_text("show only this tag; again to clear")
            .clicked()
        {
            pick = Some(t.clone());
        }
    }
    if let Some(t) = pick {
        app.tag_filter = if app.tag_filter.as_ref() == Some(&t) { None } else { Some(t) };
        app.rebuild();
    }
}
