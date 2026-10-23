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
use draw_graph_core::model::{Input, Model, model};
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
}

/// Layouts, in the order `L` steps through them.
const LAYOUTS: &[&str] = &["manual", "layered", "tree", "radial", "force"];

/// Room between a node and one added next to it, in px.
const STEP_X: i64 = 220;
const STEP_Y: i64 = 70;

/// One table drives the help page.
const KEYS: &[(&str, &str)] = &[
    ("h j k l", "move to the nearest node left, down, up, right"),
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
    ("?", "this page; j k scroll, q closes"),
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Ask {
    Child,
    Parent,
    Loose,
    Connect,
    Search,
}

impl Ask {
    fn label(self) -> &'static str {
        match self {
            Ask::Child => "New node after the focused one: name",
            Ask::Parent => "New node before the focused one: name",
            Ask::Loose => "New node: name",
            Ask::Connect => "Connect the focused node to: name",
            Ask::Search => "Search",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Normal,
    Prompt(Ask),
    Help,
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
        Edit::SetEdge { index, field, .. } => format!("saved edge {index}: {field}"),
        Edit::AddNode { .. } => "added a node".into(),
        Edit::AddEdge { .. } => "added an edge".into(),
        Edit::DeleteNode { name } => format!("deleted {name} and its edges"),
        Edit::DeleteEdge { .. } => "deleted an edge".into(),
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
    let pending_d = RwSignal::new(false);
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
        let refit = shown_layout.get_value() != lay;
        shown_layout.set_value(lay.clone());
        canvas_render(&serde_json::to_string(&mm).expect("model serializes"), &lay, refit);
        if let Some(f) = focus.get_untracked() {
            canvas_focus(&f);
        }
    });
    Effect::new(move |_| {
        let f = focus.get();
        if mounted.get() {
            canvas_focus(f.as_deref().unwrap_or(""));
        }
    });

    // The node an action starts from: the focused node, or a focused edge's From.
    let anchor = move || -> Option<String> {
        let f = focus.get_untracked()?;
        if let Some(n) = f.strip_prefix("n:") {
            return Some(n.to_string());
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
            (_, None) => msg.set("focus a node first (h j k l, or click one)".into()),
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
            blur();
            mode.set(Mode::Normal);
            ask.set(String::new());
            pending_d.set(false);
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

        let second_d = pending_d.get_untracked();
        pending_d.set(false);
        let at = focus.get_untracked().unwrap_or_default();
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
            ("d", false) if !second_d => pending_d.set(true),
            ("d", false) => {
                if let Some(name) = at.strip_prefix("n:") {
                    send(Edit::DeleteNode { name: name.to_string() }, None);
                    focus.set(None);
                } else if let Some(i) = at.strip_prefix("e:").and_then(|i| i.parse().ok()) {
                    send(Edit::DeleteEdge { index: i }, None);
                    focus.set(None);
                } else if at.starts_with("g:") {
                    msg.set("deleting a group comes with groups, in step 4".into());
                }
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
        let refusal = refused.get();
        let head = (!refusal.is_empty()).then(|| {
            view! {
                <h3>"Refused"</h3>
                {refusal.into_iter().map(|p| view! { <p class="problem">{p.to_string()}</p> }).collect_view()}
            }
        });
        let body = if let Some(name) = f.strip_prefix("n:") {
            node_panel(&mm, name, send, focus).into_any()
        } else if let Some(i) = f.strip_prefix("e:").and_then(|i| i.parse::<usize>().ok()) {
            edge_panel(&mm, i, send, focus).into_any()
        } else if let Some(name) = f.strip_prefix("g:") {
            group_panel(&mm, name, send, focus).into_any()
        } else {
            file_panel(&mm, path.get()).into_any()
        };
        view! { {head} {body} }
    };

    let status = move || {
        let mm = m.get();
        let label = match mode.get() {
            Mode::Normal if pending_d.get() => "d",
            Mode::Normal => "NORMAL",
            Mode::Prompt(_) => "PROMPT",
            Mode::Help => "HELP",
        };
        let file = path.get().rsplit('/').next().unwrap_or_default().to_string();
        let problems = mm.problems.len();
        let bad = if problems > 0 { "bad" } else { "" };
        view! {
            <span class="mode">{label}</span>
            <span>{file}</span>
            <span>{format!("layout {}", layout.get())}</span>
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
fn form_view(inputs: &[Input], values: &[(String, String)], set: Callback<(String, String)>) -> impl IntoView {
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
        {form_view(&mm.node_form, &n.fields, set)}
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
        {form_view(&mm.edge_form, &e.fields, set)}
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
        {form_view(&mm.group_form, &g.fields, set)}
        <h3>{format!("Members ({})", members.len())}</h3>
        <ul>
            {members.into_iter().map(|n| { let id = format!("n:{n}"); view! { <li class="link" on:click=move |_| focus.set(Some(id.clone()))>{n}</li> } }).collect_view()}
        </ul>
    }
    .into_any()
}

fn file_panel(mm: &Model, path: String) -> impl IntoView {
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
