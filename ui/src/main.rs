//! The draw_graph web view: Leptos, compiled to wasm, inside a Tauri window.
//!
//!   ┌──────────────────────────────┬──────────────┐
//!   │ canvas (canvas.js: Cytoscape │ side panel:  │
//!   │ draws, ELK lays out)         │ the focused  │
//!   │                              │ record's form│
//!   ├──────────────────────────────┴──────────────┤
//!   │ status: mode · file · layout · problems     │
//!   └─────────────────────────────────────────────┘
//!
//! The text is the state. Every edit goes to the Rust side (`apply`), which
//! checks it, writes the file and hands back the new text; a write from
//! vim, git or an agent arrives as a `file` event. The model is worked out
//! here from the text with the same core library, so the forms, colours and
//! rules all come from the file's schema.

use draw_graph_core::check::Problem;
use draw_graph_core::edit::Edit;
use draw_graph_core::model::{Input, Model, branches, folded, model};
use draw_graph_core::tree::{Item, Spec, common_kind, tree as walk};
use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::Deserialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], catch)]
    async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"])]
    async fn listen(event: &str, handler: &Closure<dyn FnMut(JsValue)>) -> JsValue;

    #[wasm_bindgen(js_namespace = dgCanvas, js_name = init)]
    fn canvas_init(el: &web_sys::HtmlElement, handlers: &JsValue);
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = render)]
    fn canvas_render(model: &str, layout: &str, refit: bool);
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = nearest)]
    fn canvas_nearest(id: &str, dir: &str) -> Option<String>;
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = focus)]
    fn canvas_focus(id: &str);
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = search)]
    fn canvas_search(query: &str) -> js_sys::Array;
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = positions)]
    fn canvas_positions() -> js_sys::Array;
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = fit)]
    fn canvas_fit();
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = renderTree)]
    fn canvas_render_tree(items: &str, refit: bool);
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = walk)]
    fn canvas_walk(id: &str, dir: &str, via: Option<String>) -> Option<String>;
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = follow)]
    fn canvas_follow(id: &str);
    #[wasm_bindgen(js_namespace = dgCanvas, js_name = exportSvg)]
    fn canvas_export_svg() -> String;
}

/// Layouts, in the order `L` steps through them.
const LAYOUTS: &[&str] = &["manual", "layered", "tree", "radial", "force"];

/// Room between a node and one added next to it, in px.
const STEP_X: i64 = 220;
const STEP_Y: i64 = 70;

/// One table drives the help page.
const KEYS: &[(&str, &str)] = &[
    ("h j k l, arrows", "move to the nearest node left, down, up, right (by position)"),
    (
        "Shift + arrows",
        "move along the edges: → what it unlocks, ← a prerequisite, ↑ ↓ siblings",
    ),
    ("e", "step through the focused node's edges"),
    ("Enter, i", "edit the focused record in the side panel"),
    ("Esc", "back to the canvas; close a prompt or this page"),
    ("o", "add a node after the focused one (it requires the focused one)"),
    ("O", "add a node before the focused one"),
    ("a", "add a node with no edges"),
    ("c", "connect: add an edge from the focused node to another"),
    ("dd", "delete the focused node (and its edges) or edge"),
    ("u, Ctrl-r", "undo, redo"),
    ("/", "search names and labels; smartcase"),
    ("n, N", "next, previous match"),
    ("L", "next layout: manual, layered, tree, radial, force"),
    ("s", "save the current positions into the file (switches to manual)"),
    ("f", "fit the graph to the window"),
    ("F", "follow mode on or off: zoom to and centre every node you select"),
    ("zz", "zoom to and centre the selection once"),
    ("t, T", "tree from the focused node: what it unlocks, what it needs"),
    ("V", "next saved view (the file's View records)"),
    (";", "next edge kind to follow or show; then back to all"),
    (
        "za",
        "fold or unfold the focused node's branch (all it unlocks), or a group box; in a tree, a repeat opens that copy",
    ),
    ("zR, zM", "unfold everything; fold every box (in a tree: every branch)"),
    ("G", "put the focused node in a new group, inside its current one"),
    ("E", "export what is on screen as FILE.svg next to the file"),
    ("gd", "jump from a repeat to its first copy"),
    ("?", "this page; j k scroll, q closes"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    Child,
    Parent,
    Loose,
    Connect,
    Search,
    Group,
}

impl Ask {
    fn label(self) -> &'static str {
        match self {
            Ask::Child => "New node after the focused one: name",
            Ask::Parent => "New node before the focused one: name",
            Ask::Loose => "New node: name",
            Ask::Connect => "Connect the focused node to: name",
            Ask::Search => "Search",
            Ask::Group => "New group around the focused node: name",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Normal,
    Prompt(Ask),
    Help,
}

/// The tree on screen: how to walk it, and the saved view it came from.
/// From a saved view, folds are that view's Collapse lines; from t or T,
/// they live here only.
#[derive(Clone, Debug, PartialEq)]
struct TreeState {
    spec: Spec,
    view: Option<String>,
}

#[derive(Deserialize)]
struct Opened {
    path: String,
    text: String,
}

/// Call a Rust command with string arguments; both sides speak JSON text.
async fn call(cmd: &str, args: &[(&str, &str)]) -> Result<String, String> {
    let obj = js_sys::Object::new();
    for (k, v) in args {
        let _ = js_sys::Reflect::set(&obj, &JsValue::from_str(k), &JsValue::from_str(v));
    }
    match invoke(cmd, obj.into()).await {
        Ok(v) => Ok(v.as_string().unwrap_or_default()),
        Err(e) => Err(e.as_string().unwrap_or_else(|| format!("{e:?}"))),
    }
}

fn active_tag() -> String {
    document().active_element().map(|e| e.tag_name()).unwrap_or_default()
}

fn blur() {
    if let Some(el) = document().active_element().and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok()) {
        let _ = el.blur();
    }
}

fn focus_first(selector: &str) {
    if let Some(el) = document()
        .query_selector(selector)
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = el.focus();
    }
}

fn describe(edit: &Edit) -> String {
    match edit {
        Edit::Set { key, field, .. } => format!("saved {key}: {field}"),
        Edit::SetList { key, field, values, .. } => format!("saved {key}: {} {field}", values.len()),
        Edit::SetEdge { index, field, .. } => format!("saved edge {index}: {field}"),
        Edit::AddNode { .. } => "added a node".into(),
        Edit::AddEdge { .. } => "added an edge".into(),
        Edit::DeleteNode { name } => format!("deleted {name} and its edges"),
        Edit::DeleteEdge { .. } => "deleted an edge".into(),
        Edit::AddRecord { kind, .. } => format!("added a {kind}"),
        Edit::DeleteGroup { name } => format!("removed the box {name}; its members stay"),
        Edit::Rename { name, to, .. } => format!("renamed {name} to {to}"),
        Edit::Move { positions } => format!("saved {} positions", positions.len()),
        Edit::Batch { edits } => edits.iter().map(describe).collect::<Vec<_>>().join(", "),
    }
}

#[component]
fn App() -> impl IntoView {
    let text = RwSignal::new(String::new());
    let path = RwSignal::new(String::new());
    let m = Memo::new(move |_| model(&text.get()));
    let layout = RwSignal::new(String::new());
    let focus = RwSignal::new(None::<String>);
    let mode = RwSignal::new(Mode::Normal);
    let ask = RwSignal::new(String::new());
    let msg = RwSignal::new("opening".to_string());
    let refused = RwSignal::new(Vec::<Problem>::new());
    let pending = RwSignal::new(None::<char>);
    let tree = RwSignal::new(None::<TreeState>);
    let kinds = RwSignal::new(Vec::<String>::new());
    let tag_filter = RwSignal::new(None::<String>);
    // Graph-view branch folds (za on a node): memory only.
    let branch_folds = RwSignal::new(Vec::<String>::new());
    // Follow mode (F): every new selection is zoomed to and centred.
    let follow = RwSignal::new(false);
    // The parent an edge walk (Shift + arrows) came from.
    let via = StoredValue::new(None::<String>);
    let view_at = StoredValue::new(None::<usize>);
    let items = Memo::new(move |_| {
        let mm = m.get();
        tree.get().map(|t| {
            let mut s = t.spec.clone();
            if let Some(v) = t.view.as_ref().and_then(|v| mm.views.iter().find(|x| &x.name == v)) {
                s.collapse = v.collapse.clone();
            }
            walk(&mm, &s)
        })
    });
    let item = move |id: &str| -> Option<Item> { items.with_untracked(|its| its.as_ref()?.iter().find(|i| i.id == id).cloned()) };
    let hits = RwSignal::new(Vec::<String>::new());
    let hit = RwSignal::new(0usize);
    let mounted = RwSignal::new(false);
    let shown_layout = StoredValue::new(String::new());
    let canvas = NodeRef::<leptos::html::Div>::new();
    let prompt = NodeRef::<leptos::html::Input>::new();
    let help = NodeRef::<leptos::html::Div>::new();

    // Send an edit; on success the new text redraws everything.
    let send = move |edit: Edit, then: Option<String>| {
        let json = serde_json::to_string(&edit).expect("edits serialize");
        let done = describe(&edit);
        spawn_local(async move {
            match call("apply", &[("edit", &json)]).await {
                Ok(t) => {
                    text.set(t);
                    refused.set(Vec::new());
                    msg.set(done);
                    if then.is_some() {
                        focus.set(then);
                    }
                }
                Err(e) => {
                    let ps: Vec<Problem> = serde_json::from_str(&e).unwrap_or_else(|_| vec![Problem::new(0, "edit".into(), &e, None)]);
                    msg.set(format!("refused: {}", ps[0]));
                    refused.set(ps);
                }
            }
        });
    };

    let history = move |cmd: &'static str| {
        spawn_local(async move {
            match call(cmd, &[]).await {
                Ok(t) => {
                    text.set(t);
                    msg.set(format!("{cmd} done"));
                }
                Err(e) => msg.set(e),
            }
        });
    };

    // Open the file, then follow it on disk.
    spawn_local(async move {
        match call("open", &[]).await {
            Ok(j) => {
                let o: Opened = serde_json::from_str(&j).expect("open returns {path, text}");
                let placed = model(&o.text).nodes.iter().any(|n| n.pos.is_some());
                layout.set(if placed { "manual" } else { "layered" }.to_string());
                path.set(o.path);
                text.set(o.text);
                msg.set("press ? for keys".into());
            }
            Err(e) => msg.set(format!("cannot open: {e}")),
        }
    });
    spawn_local(async move {
        let cb = Closure::<dyn FnMut(JsValue)>::new(move |ev: JsValue| {
            if let Some(t) = js_sys::Reflect::get(&ev, &"payload".into()).ok().and_then(|p| p.as_string()) {
                text.set(t);
                msg.set("reloaded: the file changed on disk".into());
            }
        });
        listen("file", &cb).await;
        cb.forget();
    });

    // Mount the canvas once its div exists.
    Effect::new(move |_| {
        let Some(el) = canvas.get() else { return };
        if mounted.get_untracked() {
            return;
        }
        let handlers = js_sys::Object::new();
        let tap = Closure::<dyn FnMut(String)>::new(move |id: String| {
            focus.set(Some(id));
            blur();
        });
        let background = Closure::<dyn FnMut()>::new(move || focus.set(None));
        let drag = Closure::<dyn FnMut(String)>::new(move |json: String| {
            let now: Vec<(String, i64, i64)> = serde_json::from_str(&json).unwrap_or_default();

            // Manual: save what moved. Any other layout: a drag pins them all.
            let positions: Vec<(String, i64, i64)> = if layout.get_untracked() == "manual" {
                m.with_untracked(|mm| {
                    now.into_iter()
                        .filter(|(n, x, y)| mm.nodes.iter().any(|v| &v.name == n && v.pos != Some((*x, *y))))
                        .collect()
                })
            } else {
                layout.set("manual".into());
                now
            };
            if !positions.is_empty() {
                send(Edit::Move { positions }, None);
            }
        });
        for (k, f) in [
            ("onTap", tap.as_ref()),
            ("onBackground", background.as_ref()),
            ("onDrag", drag.as_ref()),
        ] {
            let _ = js_sys::Reflect::set(&handlers, &k.into(), f);
        }
        tap.forget();
        background.forget();
        drag.forget();
        canvas_init(&el, &handlers);
        mounted.set(true);
    });

    // Redraw on every new model or layout; a new layout refits the view.
    Effect::new(move |_| {
        let mm = m.get();
        let lay = layout.get();
        if !mounted.get() || lay.is_empty() {
            return;
        }
        let only = kinds.get();

        // A tree draws its own items; the graph draws the model, its edges
        // narrowed to the chosen kinds.
        if let Some(its) = items.get() {
            let key = tree.with_untracked(|t| {
                t.as_ref()
                    .map(|t| format!("tree:{}:{}:{}", t.spec.root, t.spec.follow_in, t.spec.kinds.join(" ")))
            });
            let key = key.unwrap_or_default();
            let refit = shown_layout.get_value() != key;
            shown_layout.set_value(key);
            canvas_render_tree(&serde_json::to_string(&its).expect("items serialize"), refit);
        } else {
            let tagged = tag_filter.get();
            let key = format!("{lay}:{}:{}", only.join(" "), tagged.clone().unwrap_or_default());
            let refit = shown_layout.get_value() != key;
            shown_layout.set_value(key);
            let mut shown = branches(&folded(&mm), &branch_folds.get());
            if let Some(t) = &tagged {
                shown.nodes.retain(|n| n.tags.contains(t));
                let mut boxes: Vec<String> = shown.nodes.iter().filter_map(|n| n.group.clone()).collect();
                let mut i = 0;
                while i < boxes.len() {
                    if let Some(p) = mm
                        .groups
                        .iter()
                        .find(|g| g.name == boxes[i])
                        .and_then(|g| g.parent.clone())
                        .filter(|p| !boxes.contains(p))
                    {
                        boxes.push(p);
                    }
                    i += 1;
                }
                shown.groups.retain(|g| boxes.contains(&g.name));
                let kept: Vec<String> = shown.nodes.iter().map(|n| n.name.clone()).chain(boxes).collect();
                shown.edges.retain(|e| kept.contains(&e.from) && kept.contains(&e.to));
            }
            if let Some((field, _)) = mm.edge_kinds.as_ref().filter(|_| !only.is_empty()) {
                shown.edges.retain(|e| e.fields.iter().any(|(k, v)| k == field && only.contains(v)));
            }
            canvas_render(&serde_json::to_string(&shown).expect("model serializes"), &lay, refit);
        }
        if let Some(f) = focus.get_untracked() {
            canvas_focus(&f);
            if follow.get_untracked() {
                canvas_follow(&f);
            }
        }
    });

    // In a tree, a node focused by name (after an edit) moves to its first copy.
    Effect::new(move |_| {
        let f = focus.get();
        if !mounted.get() {
            return;
        }
        let copy = match (&f, items.get()) {
            (Some(f), Some(its)) if f.starts_with("n:") || f.starts_with("g:") => {
                its.iter().find(|i| i.name == f[2..]).map(|i| i.id.clone())
            }
            _ => None,
        };
        if let Some(id) = copy {
            focus.set(Some(id));
            return;
        }
        canvas_focus(f.as_deref().unwrap_or(""));
        if let Some(id) = f.as_deref().filter(|_| follow.get()) {
            canvas_follow(id);
        }
    });

    // The node an action starts from: the focused node, or a focused edge's From.
    let anchor = move || -> Option<String> {
        let f = focus.get_untracked()?;
        if let Some(n) = f.strip_prefix("n:") {
            return Some(n.to_string());
        }
        if f.starts_with("t:") {
            return item(&f).map(|i| i.name);
        }
        let i: usize = f.strip_prefix("e:")?.parse().ok()?;
        m.with_untracked(|mm| mm.edges.iter().find(|e| e.index == i).map(|e| e.from.clone()))
    };

    // A new node: required fields copied from the anchor (same Kind as its
    // neighbour), else the first allowed value; placed next to the anchor.
    let new_node = move |name: &str, from: Option<&str>, dx: i64| -> Edit {
        m.with_untracked(|mm| {
            let near = from.and_then(|a| mm.nodes.iter().find(|n| n.name == a));
            let value = |field: &str| near.and_then(|n| n.fields.iter().find(|(f, _)| f == field).map(|(_, v)| v.clone()));
            let mut fields = vec![(mm.names.node_key.clone(), name.to_string())];
            for i in mm.node_form.iter().filter(|i| i.required && i.field != mm.names.node_key) {
                let v = value(&i.field).or_else(|| i.options.first().cloned()).unwrap_or_default();
                fields.push((i.field.clone(), v));
            }
            if let Some(g) = near.and_then(|n| n.group.clone()) {
                fields.push((mm.names.group.clone(), g));
            }
            if let Some((x, y)) = near.and_then(|n| n.pos).filter(|_| mm.nodes.iter().any(|n| n.pos.is_some())) {
                let stacked = mm.edges.iter().filter(|e| Some(e.from.as_str()) == from).count() as i64;
                fields.push((mm.names.pos.clone(), format!("{} {}", x + dx, y + STEP_Y * stacked)));
            }
            Edit::AddNode { fields }
        })
    };

    // A new edge takes the kind most edges around the anchor have.
    let new_edge = move |a: &str, b: &str, around: &str| -> Edit {
        m.with_untracked(|mm| {
            let mut fields = vec![(mm.names.from.clone(), a.to_string()), (mm.names.to.clone(), b.to_string())];
            if let Some((field, kinds)) = &mm.edge_kinds {
                let near: Vec<&str> = mm
                    .edges
                    .iter()
                    .filter(|e| e.from == around || e.to == around)
                    .filter_map(|e| e.fields.iter().find(|(f, _)| f == field).map(|(_, v)| v.as_str()))
                    .collect();
                let common = kinds
                    .iter()
                    .max_by_key(|k| near.iter().filter(|n| *n == k).count())
                    .filter(|k| near.contains(&k.as_str()));
                fields.push((field.clone(), common.or(kinds.first()).cloned().unwrap_or_default()));
            }
            Edit::AddEdge { fields }
        })
    };

    let submit = move || {
        let name = ask.get_untracked().trim().to_string();
        let Mode::Prompt(kind) = mode.get_untracked() else { return };
        mode.set(Mode::Normal);
        ask.set(String::new());
        if name.is_empty() {
            return;
        }
        let a = anchor();
        let then = Some(format!("n:{name}"));
        match (kind, a.as_deref()) {
            (Ask::Search, _) => {
                let found: Vec<String> = canvas_search(&name).iter().filter_map(|v| v.as_string()).collect();
                msg.set(format!("{} matches for {name:?}", found.len()));
                hit.set(0);
                focus.set(found.first().cloned());
                hits.set(found);
            }
            (Ask::Loose, a) => send(new_node(&name, a, STEP_X), then),
            (Ask::Child, Some(a)) => send(
                Edit::Batch {
                    edits: vec![new_node(&name, Some(a), STEP_X), new_edge(a, &name, a)],
                },
                then,
            ),
            (Ask::Parent, Some(a)) => send(
                Edit::Batch {
                    edits: vec![new_node(&name, Some(a), -STEP_X), new_edge(&name, a, a)],
                },
                then,
            ),
            (Ask::Connect, Some(a)) => send(new_edge(a, &name, a), Some(format!("n:{a}"))),
            (Ask::Group, Some(a)) => {
                let (group, names) = m.with_untracked(|mm| {
                    (
                        mm.nodes.iter().find(|n| n.name == a).and_then(|n| n.group.clone()),
                        mm.names.clone(),
                    )
                });
                let mut fields = vec![(names.group_key.clone(), name.clone())];
                if let Some(g) = group {
                    fields.push(("Parent".into(), g));
                }
                let put = Edit::Set {
                    kind: "Node".into(),
                    key: a.to_string(),
                    field: names.group,
                    value: name.clone(),
                };
                send(
                    Edit::Batch {
                        edits: vec![
                            Edit::AddRecord {
                                kind: "Group".into(),
                                fields,
                            },
                            put,
                        ],
                    },
                    Some(format!("g:{name}")),
                );
            }
            (_, None) => msg.set("focus a node first (h j k l, or click one)".into()),
        }
    };

    let folds = move || -> Vec<String> {
        let Some(t) = tree.get_untracked() else { return Vec::new() };
        let saved = t
            .view
            .as_ref()
            .and_then(|v| m.with_untracked(|mm| mm.views.iter().find(|x| &x.name == v).map(|x| x.collapse.clone())));
        saved.unwrap_or(t.spec.collapse)
    };
    let set_folds = move |names: Vec<String>| {
        let Some(t) = tree.get_untracked() else { return };
        match t.view {
            Some(v) => send(
                Edit::SetList {
                    kind: "View".into(),
                    key: v,
                    field: "Collapse".into(),
                    values: names,
                },
                None,
            ),
            None => tree.update(|t| {
                if let Some(t) = t {
                    t.spec.collapse = names;
                }
            }),
        }
    };

    let open_prompt = move |kind: Ask| {
        mode.set(Mode::Prompt(kind));
        request_animation_frame(move || {
            if let Some(el) = prompt.get_untracked() {
                let _ = el.focus();
            }
        });
    };

    // Keys. Typing in a field or the prompt is left alone except for Esc
    // and Enter, which hand control back to the canvas.
    let keys = window_event_listener(leptos::ev::keydown, move |ev: web_sys::KeyboardEvent| {
        let key = ev.key();
        let tag = active_tag();
        if key == "Escape" {
            if mode.get_untracked() == Mode::Normal && pending.get_untracked().is_none() && tag_filter.get_untracked().is_some() {
                tag_filter.set(None);
                msg.set("tag filter cleared".into());
            }
            blur();
            mode.set(Mode::Normal);
            ask.set(String::new());
            pending.set(None);
            refused.set(Vec::new());
            return;
        }
        if mode.get_untracked() == Mode::Help {
            let scroll = |dy: f64| {
                if let Some(el) = help.get_untracked() {
                    el.scroll_by_with_x_and_y(0.0, dy);
                }
            };
            match key.as_str() {
                "q" | "?" => mode.set(Mode::Normal),
                "j" | "ArrowDown" => scroll(40.0),
                "k" | "ArrowUp" => scroll(-40.0),
                " " | "f" => scroll(400.0),
                "b" => scroll(-400.0),
                "g" => scroll(-1e6),
                "G" => scroll(1e6),
                _ => {}
            }
            ev.prevent_default();
            return;
        }
        if matches!(tag.as_str(), "INPUT" | "TEXTAREA" | "SELECT") {
            if key == "Enter" && tag != "TEXTAREA" && !matches!(mode.get_untracked(), Mode::Prompt(_)) {
                blur();
            }
            return;
        }
        if ev.meta_key() || ev.alt_key() {
            return;
        }

        let at = focus.get_untracked().unwrap_or_default();

        // Two-key commands: dd, za, zR, zM, gd.
        if let Some(first) = pending.get_untracked() {
            pending.set(None);
            let it = item(&at);
            match (first, key.as_str()) {
                ('d', "d") => {
                    let name = anchor().filter(|_| at.starts_with("n:") || at.starts_with("t:"));
                    if let Some(name) = name {
                        send(Edit::DeleteNode { name }, None);
                        focus.set(None);
                    } else if let Some(i) = at.strip_prefix("e:").and_then(|i| i.parse().ok()) {
                        send(Edit::DeleteEdge { index: i }, None);
                        focus.set(None);
                    } else if let Some(g) = at.strip_prefix("g:") {
                        send(Edit::DeleteGroup { name: g.to_string() }, None);
                        focus.set(None);
                    }
                }
                ('z', "a") if it.is_none() && at.starts_with("g:") => {
                    let name = at[2..].to_string();
                    let shut = m.with_untracked(|mm| mm.groups.iter().any(|g| g.name == name && g.collapsed));
                    let value = if shut { String::new() } else { "yes".to_string() };
                    send(
                        Edit::Set {
                            kind: "Group".into(),
                            key: name.clone(),
                            field: "Collapsed".into(),
                            value,
                        },
                        Some(format!("g:{name}")),
                    );
                }
                ('z', "a") if it.is_none() && at.starts_with("n:") => {
                    let name = at[2..].to_string();
                    let unlocks = m.with_untracked(|mm| mm.edges.iter().any(|e| e.from == name));
                    if !unlocks {
                        msg.set(format!("{name} unlocks nothing; nothing to fold"));
                        return;
                    }
                    let open = branch_folds.with_untracked(|f| f.contains(&name));
                    branch_folds.update(|f| {
                        if open {
                            f.retain(|x| *x != name);
                        } else {
                            f.push(name.clone());
                        }
                    });
                    msg.set(format!("{} {name}", if open { "unfolded" } else { "folded" }));
                }
                ('z', "R" | "M") if tree.with_untracked(Option::is_none) => {
                    let close = key == "M";
                    if !close {
                        branch_folds.set(Vec::new());
                    }
                    let edits: Vec<Edit> = m.with_untracked(|mm| {
                        mm.groups
                            .iter()
                            .filter(|g| g.collapsed != close)
                            .map(|g| Edit::Set {
                                kind: "Group".into(),
                                key: g.name.clone(),
                                field: "Collapsed".into(),
                                value: if close { "yes".into() } else { String::new() },
                            })
                            .collect()
                    });
                    if edits.is_empty() {
                        msg.set(format!("every box is {} already", if close { "folded" } else { "open" }));
                        return;
                    }
                    send(Edit::Batch { edits }, None);
                }
                ('z', "a") => match it {
                    None => msg.set("za folds a branch in a tree, or a box in the graph; focus one first".into()),
                    Some(i) if i.looped => msg.set(format!("{} is already above: a loop", i.name)),
                    Some(i) if i.repeat || tree.with_untracked(|t| t.as_ref().is_some_and(|t| t.spec.open.contains(&i.id))) => {
                        tree.update(|t| {
                            if let Some(t) = t {
                                match t.spec.open.iter().position(|o| *o == i.id) {
                                    Some(p) => {
                                        t.spec.open.remove(p);
                                    }
                                    None => t.spec.open.push(i.id.clone()),
                                }
                            }
                        })
                    }
                    Some(i) => {
                        let mut now = folds();
                        let leaf = i.hidden == 0
                            && items.with_untracked(|its| {
                                its.as_ref()
                                    .is_some_and(|its| !its.iter().any(|c| c.parent.as_ref() == Some(&i.id)))
                            });
                        match now.iter().position(|n| *n == i.name) {
                            Some(p) => {
                                now.remove(p);
                            }
                            None if leaf => {
                                msg.set(format!("{} has nothing below it", i.name));
                                return;
                            }
                            None => now.push(i.name.clone()),
                        }
                        set_folds(now);
                    }
                },
                ('z', "R") => {
                    tree.update(|t| {
                        if let Some(t) = t {
                            t.spec.open.clear();
                        }
                    });
                    set_folds(Vec::new());
                }
                ('z', "M") => {
                    let parents: Vec<String> = items.with_untracked(|its| {
                        let its = its.as_deref().unwrap_or_default();
                        let mut out: Vec<String> = Vec::new();
                        for i in its.iter().filter(|i| its.iter().any(|c| c.parent.as_ref() == Some(&i.id))) {
                            if !out.contains(&i.name) {
                                out.push(i.name.clone());
                            }
                        }
                        out
                    });
                    let mut now = folds();
                    now.extend(parents.into_iter().filter(|p| !now.contains(p)).collect::<Vec<_>>());
                    set_folds(now);
                }
                ('z', "z") => match focus.get_untracked() {
                    Some(id) => canvas_follow(&id),
                    None => msg.set("select a node first".into()),
                },
                ('g', "d") => match it.and_then(|i| i.first) {
                    Some(first) => {
                        focus.set(Some(first));
                        msg.set("first copy".into());
                    }
                    None => msg.set("gd jumps from a repeat (↑) to its first copy".into()),
                },
                _ => {}
            }
            ev.prevent_default();
            return;
        }

        // Shift + arrows walk the edges; plain arrows move by position.
        let walk = match key.as_str() {
            "ArrowRight" if ev.shift_key() => Some("child"),
            "ArrowLeft" if ev.shift_key() => Some("parent"),
            "ArrowDown" if ev.shift_key() => Some("next"),
            "ArrowUp" if ev.shift_key() => Some("prev"),
            _ => None,
        };
        if let Some(dir) = walk {
            ev.prevent_default();
            if at.is_empty() {
                msg.set("select a node first".into());
                return;
            }
            let hop = canvas_walk(&at, dir, via.get_value()).and_then(|j| serde_json::from_str::<(String, Option<String>)>(&j).ok());
            match hop {
                Some((next, from)) => {
                    via.set_value(from);
                    focus.set(Some(next));
                }
                None => msg.set(
                    match dir {
                        "child" => "it unlocks nothing that is drawn",
                        "parent" => "it needs nothing that is drawn",
                        _ => "no sibling to move to",
                    }
                    .into(),
                ),
            }
            return;
        }
        let key = match key.as_str() {
            "ArrowLeft" => "h".to_string(),
            "ArrowDown" => "j".to_string(),
            "ArrowUp" => "k".to_string(),
            "ArrowRight" => "l".to_string(),
            _ => key,
        };
        match (key.as_str(), ev.ctrl_key()) {
            ("h" | "j" | "k" | "l", false) => {
                if let Some(next) = canvas_nearest(&at, &key) {
                    focus.set(Some(next));
                }
            }
            ("e", false) => {
                let Some(a) = anchor() else { return };
                let ids: Vec<String> = m.with_untracked(|mm| {
                    mm.edges
                        .iter()
                        .filter(|e| e.from == a || e.to == a)
                        .map(|e| format!("e:{}", e.index))
                        .collect()
                });
                let next = ids.iter().position(|i| *i == at).map_or(0, |p| (p + 1) % ids.len().max(1));
                match ids.get(next) {
                    Some(id) => focus.set(Some(id.clone())),
                    None => msg.set(format!("{a} has no edges")),
                }
            }
            ("Enter" | "i", false) => focus_first("#panel input, #panel select, #panel textarea"),
            ("o", false) => open_prompt(Ask::Child),
            ("O", false) => open_prompt(Ask::Parent),
            ("a", false) => open_prompt(Ask::Loose),
            ("c", false) => open_prompt(Ask::Connect),
            ("/", false) => open_prompt(Ask::Search),
            ("d" | "z" | "g", false) => pending.set(key.chars().next()),
            ("G", false) => open_prompt(Ask::Group),
            ("E", false) => {
                let svg = canvas_export_svg();
                spawn_local(async move {
                    match call("export_svg", &[("svg", &svg)]).await {
                        Ok(path) => msg.set(format!("exported {path}")),
                        Err(e) => msg.set(format!("export failed: {e}")),
                    }
                });
            }
            ("t" | "T", false) => {
                let Some(a) = anchor() else {
                    msg.set("focus a node first (h j k l, or click one)".into());
                    return;
                };
                let follow_in = key == "T";
                let ks = m.with_untracked(|mm| common_kind(mm, &a, follow_in));
                let spec = Spec {
                    root: a.clone(),
                    follow_in,
                    kinds: ks.clone(),
                    direction: if follow_in { "left" } else { "right" }.into(),
                    ..Spec::default()
                };
                tree.set(Some(TreeState { spec, view: None }));
                focus.set(Some("t:0".into()));
                let what = if follow_in { "what it needs" } else { "what it unlocks" };
                let along = if ks.is_empty() { "every edge".to_string() } else { ks.join(" ") };
                msg.set(format!("tree from {a}: {what}, along {along}; ; changes the kind, L leaves"));
            }
            ("V", false) => {
                let views = m.with_untracked(|mm| mm.views.clone());
                if views.is_empty() {
                    msg.set("the file has no View records".into());
                    return;
                }
                let i = view_at.get_value().map_or(0, |i| (i + 1) % views.len());
                view_at.set_value(Some(i));
                let v = &views[i];
                match (v.layout.as_str(), &v.root) {
                    ("tree", Some(root)) => {
                        let spec = Spec {
                            root: root.clone(),
                            follow_in: v.follow_in,
                            kinds: v.edges.clone(),
                            full: v.full,
                            collapse: v.collapse.clone(),
                            open: Vec::new(),
                            direction: v
                                .direction
                                .clone()
                                .unwrap_or_else(|| if v.follow_in { "left" } else { "right" }.into()),
                        };
                        tree.set(Some(TreeState {
                            spec,
                            view: Some(v.name.clone()),
                        }));
                        focus.set(Some("t:0".into()));
                    }
                    _ => {
                        tree.set(None);
                        kinds.set(v.edges.clone());
                        layout.set(if LAYOUTS.contains(&v.layout.as_str()) {
                            v.layout.clone()
                        } else {
                            "layered".into()
                        });
                    }
                }
                let filter = v
                    .filter
                    .as_ref()
                    .map(|f| format!(" (its Filter {f:?} is not applied yet)"))
                    .unwrap_or_default();
                msg.set(format!("view {} of {}: {}{filter}", i + 1, views.len(), v.name));
            }
            (";", false) => {
                let all = m.with_untracked(|mm| mm.edge_kinds.clone().map(|(_, v)| v).unwrap_or_default());
                if all.is_empty() {
                    msg.set("this file has no edge kinds".into());
                    return;
                }
                let now = tree
                    .with_untracked(|t| t.as_ref().map(|t| t.spec.kinds.clone()))
                    .unwrap_or_else(|| kinds.get_untracked());

                // All kinds, then each kind in turn, then all again.
                let next = match now.as_slice() {
                    [] => vec![all[0].clone()],
                    [k] => all
                        .iter()
                        .position(|x| x == k)
                        .and_then(|i| all.get(i + 1))
                        .cloned()
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                };
                let label = if next.is_empty() {
                    "every kind".to_string()
                } else {
                    next.join(" ")
                };
                if tree.with_untracked(Option::is_some) {
                    tree.update(|t| {
                        if let Some(t) = t {
                            t.spec.kinds = next;
                        }
                    });
                    focus.set(Some("t:0".into()));
                } else {
                    kinds.set(next);
                }
                msg.set(format!("edges: {label}"));
            }
            ("u", false) => history("undo"),
            ("r", true) => history("redo"),
            ("n" | "N", false) => {
                let all = hits.get_untracked();
                if all.is_empty() {
                    msg.set("no search; press /".into());
                    return;
                }
                let len = all.len();
                let i = if key == "n" {
                    (hit.get_untracked() + 1) % len
                } else {
                    (hit.get_untracked() + len - 1) % len
                };
                hit.set(i);
                focus.set(Some(all[i].clone()));
                msg.set(format!("match {} of {len}", i + 1));
            }
            ("L", false) if tree.with_untracked(Option::is_some) => {
                tree.set(None);
                focus.set(None);
                msg.set(format!("back to layout {}", layout.get_untracked()));
            }
            ("L", false) => {
                let now = layout.get_untracked();
                let i = LAYOUTS.iter().position(|l| *l == now).map_or(0, |i| (i + 1) % LAYOUTS.len());
                layout.set(LAYOUTS[i].to_string());
                msg.set(format!("layout: {}", LAYOUTS[i]));
            }
            ("s", false) => {
                let positions: Vec<(String, i64, i64)> = canvas_positions()
                    .iter()
                    .filter_map(|v| {
                        let a: js_sys::Array = v.dyn_into().ok()?;
                        Some((a.get(0).as_string()?, a.get(1).as_f64()? as i64, a.get(2).as_f64()? as i64))
                    })
                    .collect();
                layout.set("manual".into());
                send(Edit::Move { positions }, None);
            }
            ("f", false) => canvas_fit(),
            ("F", false) => {
                let on = !follow.get_untracked();
                follow.set(on);
                msg.set(format!(
                    "follow {}",
                    if on { "on: the view stays on your selection" } else { "off" }
                ));
            }
            ("?", false) => mode.set(Mode::Help),
            _ => return,
        }
        ev.prevent_default();
    });
    on_cleanup(move || keys.remove());

    // The side panel: the focused record's form, else the file summary.
    let panel = move || {
        let mm = m.get();
        let f = focus.get().unwrap_or_default();

        // A tree item shows the record it is a copy of.
        let f = match items.with(|its| its.as_ref().and_then(|its| its.iter().find(|i| i.id == f)).map(|i| i.name.clone())) {
            Some(name) if mm.nodes.iter().any(|n| n.name == name) => format!("n:{name}"),
            Some(name) => format!("g:{name}"),
            None => f,
        };
        let refusal = refused.get();
        let head = (!refusal.is_empty()).then(|| {
            view! {
                <h3>"Refused"</h3>
                {refusal.into_iter().map(|p| view! { <p class="problem">{p.to_string()}</p> }).collect_view()}
            }
        });
        let body = if let Some(name) = f.strip_prefix("n:") {
            node_panel(&mm, name, send, focus, tag_filter).into_any()
        } else if let Some(i) = f.strip_prefix("e:").and_then(|i| i.parse::<usize>().ok()) {
            edge_panel(&mm, i, send, focus).into_any()
        } else if let Some(name) = f.strip_prefix("g:") {
            group_panel(&mm, name, send, focus).into_any()
        } else {
            file_panel(&mm, path.get(), tag_filter).into_any()
        };
        view! { {head} {body} }
    };

    let status = move || {
        let mm = m.get();
        let label = match mode.get() {
            Mode::Normal if pending.get() == Some('d') => "d",
            Mode::Normal if pending.get() == Some('z') => "z",
            Mode::Normal if pending.get() == Some('g') => "g",
            Mode::Normal => "NORMAL",
            Mode::Prompt(_) => "PROMPT",
            Mode::Help => "HELP",
        };
        let file = path.get().rsplit('/').next().unwrap_or_default().to_string();
        let problems = mm.problems.len();
        let bad = if problems > 0 { "bad" } else { "" };
        let following = follow.get().then(|| view! { <span class="mode">"FOLLOW"</span> });
        view! {
            <span class="mode">{label}</span>
            {following}
            <span>{file}</span>
            <span>{tree.with(|t| match t {
                Some(TreeState { view: Some(v), .. }) => format!("view {v}"),
                Some(t) => format!("tree {} ({})", t.spec.root, if t.spec.follow_in { "needs" } else { "unlocks" }),
                None => format!(
                    "layout {}{}{}",
                    layout.get(),
                    kinds.with(|k| if k.is_empty() { String::new() } else { format!(" · {}", k.join(" ")) }),
                    tag_filter.with(|f| f.as_ref().map(|f| format!(" · tag {f}")).unwrap_or_default())
                ),
            })}</span>
            <span>{format!("{} nodes · {} edges", mm.nodes.len(), mm.edges.len())}</span>
            <span class=bad>{format!("{problems} problems")}</span>
            <span class="msg">{msg.get()}</span>
        }
    };

    view! {
        <div id="app">
            <div id="canvas" node_ref=canvas></div>
            <div id="panel">{panel}</div>
            <div id="status">{status}</div>
        </div>
        <Show when=move || matches!(mode.get(), Mode::Prompt(_))>
            <div id="prompt">
                <p>{move || match mode.get() { Mode::Prompt(k) => k.label(), _ => "" }}</p>
                <input
                    node_ref=prompt
                    prop:value=move || ask.get()
                    on:input=move |ev| ask.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        // Keep this Enter from the window's handler, which
                        // would take it as "edit in the side panel".
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            ev.stop_propagation();
                            submit();
                        }
                    }
                />
            </div>
        </Show>
        <Show when=move || mode.get() == Mode::Help>
            <div id="help" node_ref=help>
                <h2>"draw_graph keys"</h2>
                <p class="sub">"Draw and edit a recfile as a graph. Every accepted edit is written to the file at once; an edit that breaks a rule is refused and the file is left as it was. Edits made in vim, git or by an agent show up here as soon as they land."</p>
                <table>
                    {KEYS.iter().map(|(k, d)| view! { <tr><td>{*k}</td><td>{*d}</td></tr> }).collect_view()}
                </table>
                <h3>"Colours and lines"</h3>
                <p class="sub">"Node colours follow the field named by %graph: color; a node's own Color wins. Dashed edges are OR choices (shared Any); dotted ones are the kinds %graph: style marks dotted. Boxes are groups."</p>
            </div>
        </Show>
    }
}

/// Inputs for a record: the schema's form, plus any field the form lacks.
/// A repeated field is shown but not edited here: one input would flatten it.
fn form_view(inputs: &[Input], values: &[(String, String)], set: Callback<(String, String)>, skip: Option<&str>) -> impl IntoView {
    let inputs: Vec<Input> = inputs.iter().filter(|i| Some(i.field.as_str()) != skip).cloned().collect();
    let values: Vec<(String, String)> = values.iter().filter(|(k, _)| Some(k.as_str()) != skip).cloned().collect();
    let (inputs, values) = (&inputs[..], &values[..]);
    let count = |f: &str| values.iter().filter(|(k, _)| k == f).count();
    let value = |f: &str| values.iter().find(|(k, _)| k == f).map(|(_, v)| v.clone()).unwrap_or_default();
    let mut all: Vec<Input> = inputs.to_vec();
    for (k, _) in values {
        if !all.iter().any(|i| &i.field == k) {
            all.push(Input {
                field: k.clone(),
                kind: "text".into(),
                options: Vec::new(),
                required: false,
            });
        }
    }

    all.into_iter()
        .map(|i| {
            let field = i.field.clone();
            let now = value(&field);
            let control = if count(&field) > 1 {
                let joined = values
                    .iter()
                    .filter(|(k, _)| *k == field)
                    .map(|(_, v)| v.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                view! { <input value=joined disabled=true title="a repeated field: edit it in the file"/> }.into_any()
            } else if i.kind == "select" {
                let blank = (!i.required).then(|| view! { <option value="">"—"</option> });
                let opts = i
                    .options
                    .iter()
                    .map(|o| view! { <option value=o.clone() selected={*o == now}>{o.clone()}</option> })
                    .collect_view();
                view! { <select on:change=move |ev| set.run((field.clone(), event_target_value(&ev)))>{blank}{opts}</select> }.into_any()
            } else if i.kind == "textarea" {
                view! { <textarea prop:value=now on:change=move |ev| set.run((field.clone(), event_target_value(&ev)))></textarea> }
                    .into_any()
            } else {
                view! { <input value=now on:change=move |ev| set.run((field.clone(), event_target_value(&ev)))/> }.into_any()
            };
            view! { <label class:req=i.required><span>{i.field.clone()}</span>{control}</label> }
        })
        .collect_view()
}

fn node_panel(
    mm: &Model,
    name: &str,
    send: impl Fn(Edit, Option<String>) + Copy + Send + Sync + 'static,
    focus: RwSignal<Option<String>>,
    tag_filter: RwSignal<Option<String>>,
) -> impl IntoView {
    let Some(n) = mm.nodes.iter().find(|n| n.name == name).cloned() else {
        return view! { <p class="sub">{format!("{name} is gone")}</p> }.into_any();
    };
    let key = mm.names.node_key.clone();
    let me = n.name.clone();
    let set = Callback::new(move |(field, value): (String, String)| {
        if field == key {
            send(
                Edit::Rename {
                    kind: "Node".into(),
                    name: me.clone(),
                    to: value.clone(),
                },
                Some(format!("n:{value}")),
            );
            return;
        }
        send(
            Edit::Set {
                kind: "Node".into(),
                key: me.clone(),
                field,
                value,
            },
            None,
        );
    });
    let edges: Vec<(usize, String)> = mm
        .edges
        .iter()
        .filter(|e| e.from == n.name || e.to == n.name)
        .map(|e| {
            let kind = e
                .label
                .clone()
                .or_else(|| e.fields.iter().find(|(f, _)| f == "Kind").map(|(_, v)| v.clone()))
                .unwrap_or_default();
            let line = if e.from == n.name {
                format!("→ {}  {kind}", e.to)
            } else {
                format!("← {}  {kind}", e.from)
            };
            (e.index, line)
        })
        .collect();
    view! {
        <h2>{n.label.clone()}</h2>
        <p class="sub">"Node · Enter edits, Esc returns"</p>
        {tag_view(&n.name, &n.tags, mm.names.tag.clone(), send, tag_filter)}
        {form_view(&mm.node_form, &n.fields, set, mm.names.tag.as_deref())}
        <h3>{format!("Edges ({})", edges.len())}</h3>
        <ul>
            {edges.into_iter().map(|(i, line)| view! { <li class="link" on:click=move |_| focus.set(Some(format!("e:{i}")))>{line}</li> }).collect_view()}
        </ul>
    }
    .into_any()
}

fn edge_panel(
    mm: &Model,
    index: usize,
    send: impl Fn(Edit, Option<String>) + Copy + Send + Sync + 'static,
    focus: RwSignal<Option<String>>,
) -> impl IntoView {
    let Some(e) = mm.edges.iter().find(|e| e.index == index).cloned() else {
        return view! { <p class="sub">"that edge is gone"</p> }.into_any();
    };
    let set = Callback::new(move |(field, value): (String, String)| send(Edit::SetEdge { index, field, value }, None));
    let (from, to) = (e.from.clone(), e.to.clone());
    view! {
        <h2>{format!("{} → {}", e.from, e.to)}</h2>
        <p class="sub">{format!("Edge · {}{}", e.style, e.any.as_ref().map(|a| format!(" · OR group {a}")).unwrap_or_default())}</p>
        {form_view(&mm.edge_form, &e.fields, set, None)}
        <h3>"Ends"</h3>
        <ul>
            <li class="link" on:click=move |_| focus.set(Some(format!("n:{from}")))>{format!("from {}", e.from)}</li>
            <li class="link" on:click=move |_| focus.set(Some(format!("n:{to}")))>{format!("to {}", e.to)}</li>
        </ul>
    }
    .into_any()
}

fn group_panel(
    mm: &Model,
    name: &str,
    send: impl Fn(Edit, Option<String>) + Copy + Send + Sync + 'static,
    focus: RwSignal<Option<String>>,
) -> impl IntoView {
    let Some(g) = mm.groups.iter().find(|g| g.name == name).cloned() else {
        return view! { <p class="sub">{format!("{name} is gone")}</p> }.into_any();
    };
    let key = mm.names.group_key.clone();
    let me = g.name.clone();
    let set = Callback::new(move |(field, value): (String, String)| {
        if field == key {
            send(
                Edit::Rename {
                    kind: "Group".into(),
                    name: me.clone(),
                    to: value.clone(),
                },
                Some(format!("g:{value}")),
            );
            return;
        }
        send(
            Edit::Set {
                kind: "Group".into(),
                key: me.clone(),
                field,
                value,
            },
            None,
        );
    });
    let members: Vec<String> = mm
        .nodes
        .iter()
        .filter(|n| n.group.as_deref() == Some(name))
        .map(|n| n.name.clone())
        .collect();
    view! {
        <h2>{g.label.clone()}</h2>
        <p class="sub">"Group"</p>
        {form_view(&mm.group_form, &g.fields, set, None)}
        <h3>{format!("Members ({})", members.len())}</h3>
        <ul>
            {members.into_iter().map(|n| { let id = format!("n:{n}"); view! { <li class="link" on:click=move |_| focus.set(Some(id.clone()))>{n}</li> } }).collect_view()}
        </ul>
    }
    .into_any()
}

/// A node's tags as chips: click one to filter by it, x removes it, the
/// field adds one on Enter. The whole list is saved as the node's Tag lines.
fn tag_view(
    node: &str,
    tags: &[String],
    field: Option<String>,
    send: impl Fn(Edit, Option<String>) + Copy + Send + Sync + 'static,
    tag_filter: RwSignal<Option<String>>,
) -> impl IntoView {
    let Some(field) = field else { return ().into_any() };
    let save = {
        let (node, field) = (node.to_string(), field.clone());
        Callback::new(move |values: Vec<String>| {
            send(
                Edit::SetList {
                    kind: "Node".into(),
                    key: node.clone(),
                    field: field.clone(),
                    values,
                },
                None,
            );
        })
    };
    let chips = tags
        .iter()
        .map(|t| {
            let rest: Vec<String> = tags.iter().filter(|x| *x != t).cloned().collect();
            let (pick, label) = (t.clone(), t.clone());
            view! {
                <span class="chip">
                    <span class="chip-name" title="show only this tag" on:click=move |_| tag_filter.set(Some(pick.clone()))>{label}</span>
                    <button title="remove this tag" on:click=move |_| save.run(rest.clone())>"×"</button>
                </span>
            }
        })
        .collect_view();
    let now: Vec<String> = tags.to_vec();
    view! {
        <h3>{format!("Tags ({})", tags.len())}</h3>
        <div class="chips">{chips}</div>
        <input
            class="tag-add"
            placeholder="add a tag, Enter"
            on:keydown=move |ev: web_sys::KeyboardEvent| {
                if ev.key() != "Enter" {
                    return;
                }
                ev.prevent_default();
                ev.stop_propagation();
                let new = event_target_value(&ev).trim().to_string();
                if new.is_empty() || now.contains(&new) {
                    return;
                }
                let mut all = now.clone();
                all.push(new);
                save.run(all);
            }
        />
    }
    .into_any()
}

fn file_panel(mm: &Model, path: String, tag_filter: RwSignal<Option<String>>) -> impl IntoView {
    let on = tag_filter.get_untracked();
    let tags = mm
        .tags
        .iter()
        .map(|(t, n)| {
            let pick = t.clone();
            let active = on.as_deref() == Some(t.as_str());
            view! {
                <li class="link" class:on=active title="show only this tag; again to clear" on:click=move |_| {
                    tag_filter.update(|f| *f = if f.as_deref() == Some(pick.as_str()) { None } else { Some(pick.clone()) })
                }>{format!("{t}  ({n})")}</li>
            }
        })
        .collect_view();
    view! {
        {file_summary(mm, path)}
        <h3>{format!("Tags ({})", mm.tags.len())}</h3>
        <ul class="tags">{tags}</ul>
    }
}

fn file_summary(mm: &Model, path: String) -> impl IntoView {
    view! {
        <h2>{path.rsplit('/').next().unwrap_or_default().to_string()}</h2>
        <p class="sub">{path.clone()}</p>
        <p>{format!("{} nodes, {} edges, {} groups", mm.nodes.len(), mm.edges.len(), mm.groups.len())}</p>
        <p class="sub">"Nothing focused. h j k l or a click picks a node; ? lists every key."</p>
        <h3>{format!("Problems ({})", mm.problems.len())}</h3>
        {mm.problems.iter().map(|p| view! { <p class="problem">{format!("line {}: {p}", p.line)}</p> }).collect_view()}
    }
}

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(App);
}
