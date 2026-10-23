//! The window's state and its keys.
//!
//!   ┌──────────────────────────────┬──────────────┐
//!   │ canvas (draw.rs)             │ side panel   │
//!   │                              │ (panel.rs)   │
//!   ├──────────────────────────────┴──────────────┤
//!   │ status: mode · file · view · counts · msg   │
//!   └─────────────────────────────────────────────┘
//!
//! The file text is the state: every edit goes through file.rs, then the
//! model and the scene are rebuilt from the new text. Folds, follow mode,
//! filters and the view are the window's own and never touch the file,
//! except a saved view's folds (its Collapse lines) and box folds.

use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};

use draw_graph_core::check::Problem;
use draw_graph_core::edit::Edit;
use draw_graph_core::model::{Model, branches, folded, model};
use draw_graph_core::tree::{Item, Spec, common_kind, tree};
use eframe::egui::{self, Align2, Key, Modifiers, Pos2, Rect, Sense, Vec2};
use notify::{EventKind, RecursiveMode, Watcher};

use crate::draw::{self, BG, Geometry, TEXT};
use crate::file::File;
use crate::scene::{self, Layout, Scene};

/// Follow mode's zoom, and how fast the view glides (per frame, 0..1).
const FOLLOW_ZOOM: f32 = 1.3;
const GLIDE: f32 = 0.25;
const ZOOM_RANGE: (f32, f32) = (0.05, 4.0);

/// Room between a node and one added next to it, in graph units.
const STEP: Vec2 = Vec2::new(220.0, 70.0);

/// One table drives the help page.
pub const KEYS: &[(&str, &str)] = &[
    ("arrows, h j k l", "move to the nearest node by position"),
    (
        "Shift + arrows",
        "move along the edges: → what it unlocks, ← a prerequisite, ↑ ↓ siblings",
    ),
    ("e", "step through the selected node's edges"),
    ("#", "add a tag to the selected node"),
    ("Enter, i", "edit the selection in the side panel"),
    ("Esc", "back to the canvas; close a prompt, this page or the tag filter"),
    ("o, O", "add a node after (it requires the selection) or before"),
    ("a", "add a node with no edges"),
    ("c", "connect: an edge from the selection to another node"),
    ("G", "put the selected node in a new group inside its current one"),
    ("dd", "delete the selected node (and its edges), edge or box (not its members)"),
    ("u, Ctrl-r", "undo, redo"),
    ("/, n, N", "search names, labels, tags and boxes (smartcase); next, previous"),
    ("za", "fold or unfold: a node's branch, a box, or in a tree a branch or repeat"),
    ("zR, zM", "unfold everything; fold every box (in a tree: every branch)"),
    ("zz", "zoom to and centre the selection"),
    ("t, T", "tree from the selection: what it unlocks, what it needs"),
    ("V", "next saved view (the file's View records)"),
    (";", "next edge kind to follow or show; then all"),
    ("gd", "from a repeat in a tree to its first copy"),
    ("L", "layout: manual (saved positions) or tree; leaves a tree view"),
    ("s", "save the positions on screen into the file (switches to manual)"),
    ("f, F", "fit the graph; follow mode on or off"),
    ("E", "export what is on screen as FILE.svg next to the file"),
    ("?", "this page"),
];

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Ask {
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
            Ask::Child => "New node after the selection: name",
            Ask::Parent => "New node before the selection: name",
            Ask::Loose => "New node: name",
            Ask::Connect => "Connect the selection to: name",
            Ask::Search => "Search",
            Ask::Group => "New group around the selection: name",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Normal,
    Prompt(Ask),
    Help,
}

#[derive(Clone, Copy, PartialEq)]
pub struct View {
    pub pan: Vec2,
    pub zoom: f32,
}

/// A tree on screen: how to walk it, and the saved view it came from.
#[derive(Clone)]
pub struct TreeState {
    pub spec: Spec,
    pub view: Option<String>,
}

pub struct App {
    pub file: File,
    pub full: Model,
    pub scene: Scene,
    items: Vec<Item>,
    pub geo: Geometry,
    pub layout: Layout,
    pub tree: Option<TreeState>,
    pub folds: Vec<String>,
    pub tag_filter: Option<String>,
    pub kinds: Vec<String>,
    view_at: Option<usize>,
    pub view: View,
    target: Option<View>,
    pub selected: Option<String>,
    via: Option<String>,
    pub follow: bool,
    pending: Option<char>,
    pub mode: Mode,
    pub ask: String,
    pub hits: Vec<String>,
    hit: usize,
    pub msg: String,
    pub refused: Vec<Problem>,
    fitted: bool,
    /// A node or box being dragged, and how far, in graph units.
    drag: Option<(String, Vec2)>,
    /// Edits the panel asked for this frame.
    pub outbox: Vec<(Edit, Option<String>)>,
    pub focus_panel: bool,
    pub focus_tag: bool,
    /// The panel's field text while it is being typed, by field name, for
    /// the record and file version in `buffers_for`.
    pub buffers: HashMap<String, String>,
    pub buffers_for: (String, u64),
    pub version: u64,
    marks: &'static scene::Marks,
    changes: Receiver<()>,
    _watcher: Option<notify::RecommendedWatcher>,
}

fn describe(edit: &Edit) -> String {
    match edit {
        Edit::Set { key, field, .. } => format!("saved {key}: {field}"),
        Edit::SetList { key, field, values, .. } => format!("saved {key}: {} {field}", values.len()),
        Edit::SetEdge { index, field, .. } => format!("saved edge {index}: {field}"),
        Edit::AddNode { .. } => "added a node".into(),
        Edit::AddEdge { .. } => "added an edge".into(),
        Edit::AddRecord { kind, .. } => format!("added a {kind}"),
        Edit::DeleteNode { name } => format!("deleted {name} and its edges"),
        Edit::DeleteEdge { .. } => "deleted an edge".into(),
        Edit::DeleteGroup { name } => format!("removed the box {name}; its members stay"),
        Edit::Rename { name, to, .. } => format!("renamed {name} to {to}"),
        Edit::Move { positions } => format!("saved {} positions", positions.len()),
        Edit::Batch { edits } => edits.iter().map(describe).collect::<Vec<_>>().join(", "),
    }
}

impl App {
    pub fn new(cc: &eframe::CreationContext, file: File, marks: &'static scene::Marks) -> App {
        // Watch the folder (editors replace files) and wake the window.
        let (tx, changes) = channel();
        let ctx = cc.egui_ctx.clone();
        let path = file.path.clone();
        let name = path.file_name().map(|n| n.to_owned());
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            let Ok(ev) = res else { return };
            if matches!(ev.kind, EventKind::Access(_)) || !ev.paths.iter().any(|p| p.file_name() == name.as_deref()) {
                return;
            }
            let _ = tx.send(());
            ctx.request_repaint();
        })
        .and_then(|mut w| {
            w.watch(path.parent().unwrap_or(Path::new(".")), RecursiveMode::NonRecursive)?;
            Ok(w)
        })
        .ok();

        let full = model(&file.text);
        let layout = if full.nodes.iter().any(|n| n.pos.is_some()) {
            Layout::Manual
        } else {
            Layout::Tree
        };
        let mut app = App {
            file,
            full,
            scene: Scene::default(),
            items: Vec::new(),
            geo: Geometry::default(),
            layout,
            tree: None,
            folds: Vec::new(),
            tag_filter: None,
            kinds: Vec::new(),
            view_at: None,
            view: View {
                pan: Vec2::ZERO,
                zoom: 1.0,
            },
            target: None,
            selected: None,
            via: None,
            follow: false,
            pending: None,
            mode: Mode::Normal,
            ask: String::new(),
            hits: Vec::new(),
            hit: 0,
            msg: "press ? for keys".into(),
            refused: Vec::new(),
            fitted: false,
            drag: None,
            outbox: Vec::new(),
            focus_panel: false,
            focus_tag: false,
            buffers: HashMap::new(),
            buffers_for: (String::new(), 0),
            version: 0,
            marks,
            changes,
            _watcher: watcher,
        };
        app.rebuild();
        app
    }

    /// Model and scene from the file text and the window's view state.
    pub fn rebuild(&mut self) {
        self.version += 1;
        self.full = model(&self.file.text);
        // A tree whose root is gone (deleted, renamed, undone) closes.
        if let Some(root) = self.tree.as_ref().map(|t| t.spec.root.clone())
            && !self.full.nodes.iter().any(|n| n.name == root)
            && !self.full.groups.iter().any(|g| g.name == root)
        {
            self.tree = None;
            self.selected = None;
            self.msg = format!("{root} is gone; back to the graph");
        }
        if let Some(t) = &self.tree {
            let mut spec = t.spec.clone();
            if let Some(v) = t.view.as_ref().and_then(|v| self.full.views.iter().find(|x| &x.name == v)) {
                spec.collapse = v.collapse.clone();
            }
            self.items = tree(&self.full, &spec);
            self.scene = scene::tree_scene(&self.items, &self.full, self.marks);
            return;
        }
        self.items.clear();
        let mut shown = branches(&folded(&self.full), &self.folds);

        // A tag filter keeps the nodes with that tag, their boxes and the
        // edges between them; a kind filter keeps edges of those kinds.
        if let Some(t) = &self.tag_filter {
            shown.nodes.retain(|n| n.tags.contains(t));
            let mut boxes: Vec<String> = shown.nodes.iter().filter_map(|n| n.group.clone()).collect();
            let mut i = 0;
            while i < boxes.len() {
                if let Some(p) = self
                    .full
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
        if let Some((field, _)) = self.full.edge_kinds.as_ref().filter(|_| !self.kinds.is_empty()) {
            shown
                .edges
                .retain(|e| e.fields.iter().any(|(k, v)| k == field && self.kinds.contains(v)));
        }
        self.scene = scene::graph(&shown, self.layout, self.marks);
    }

    /// Send an edit; on success rebuild and maybe select `then`.
    pub fn send(&mut self, edit: Edit, then: Option<String>) {
        match self.file.apply(&edit) {
            Ok(()) => {
                self.refused.clear();
                self.msg = describe(&edit);
                self.rebuild();
                if then.is_some() {
                    self.select(then);
                }
            }
            Err(ps) => {
                self.msg = format!("refused: {}", ps[0]);
                self.refused = ps;
                self.version += 1; // the panel's fields show the saved values again
            }
        }
    }

    fn history(&mut self, back: bool) {
        match self.file.step(back) {
            Ok(()) => {
                self.msg = format!("{} done", if back { "undo" } else { "redo" });
                self.rebuild();
            }
            Err(e) => self.msg = e,
        }
    }

    fn select(&mut self, id: Option<String>) {
        // In a tree, a node named by an edit lands on its first copy.
        let id = match id {
            Some(i) if self.tree.is_some() && (i.starts_with("n:") || i.starts_with("g:")) => {
                self.items.iter().find(|t| t.name == i[2..]).map(|t| t.id.clone()).or(Some(i))
            }
            other => other,
        };
        self.selected = id;
    }

    /// The node an action starts from: the selected node, a tree item's
    /// node, or a selected edge's From.
    pub fn anchor(&self) -> Option<String> {
        let s = self.selected.as_ref()?;
        if let Some(n) = s.strip_prefix("n:") {
            return Some(n.to_string());
        }
        if s.starts_with("t:") {
            return self.items.iter().find(|i| &i.id == s).map(|i| i.name.clone());
        }
        let i: usize = s.strip_prefix("e:")?.parse().ok()?;
        self.full.edges.iter().find(|e| e.index == i).map(|e| e.from.clone())
    }

    /// The record the panel shows for the selection: `n:`, `g:` or `e:`.
    pub fn record(&self) -> Option<String> {
        let s = self.selected.as_ref()?;
        if s.starts_with("t:") {
            let i = self.items.iter().find(|i| &i.id == s)?;
            let kind = if self.full.nodes.iter().any(|n| n.name == i.name) {
                "n"
            } else {
                "g"
            };
            return Some(format!("{kind}:{}", i.name));
        }
        Some(s.clone())
    }

    fn center(&self, id: &str) -> Option<Pos2> {
        self.scene.nodes.iter().find(|n| n.id == id).map(|n| Pos2::new(n.x, n.y))
    }

    fn fit_view(&self, canvas: Rect) -> View {
        let pts: Vec<Pos2> = self.scene.nodes.iter().map(|n| Pos2::new(n.x, n.y)).collect();
        if pts.is_empty() {
            return self.view;
        }
        let bb = Rect::from_points(&pts).expand(80.0);
        let zoom = (canvas.width() / bb.width())
            .min(canvas.height() / bb.height())
            .clamp(ZOOM_RANGE.0, 2.0);
        View {
            zoom,
            pan: canvas.size() / 2.0 - bb.center().to_vec2() * zoom,
        }
    }

    fn follow_view(&self, canvas: Rect, id: &str) -> Option<View> {
        let p = self
            .center(id)
            .or_else(|| self.geo.rect(id).map(|r| self.to_graph(canvas, r.center())))?;
        Some(View {
            zoom: FOLLOW_ZOOM,
            pan: canvas.size() / 2.0 - p.to_vec2() * FOLLOW_ZOOM,
        })
    }

    fn to_graph(&self, canvas: Rect, p: Pos2) -> Pos2 {
        ((p - canvas.min - self.view.pan) / self.view.zoom).to_pos2()
    }

    /// The nearest drawn node in a direction: in a 63° cone, scored by
    /// distance ahead plus twice the distance aside.
    fn nearest(&self, dir: Vec2) -> Option<String> {
        let Some(from) = self.selected.as_ref().and_then(|s| self.center(s)) else {
            return self.scene.nodes.first().map(|n| n.id.clone());
        };
        self.scene
            .nodes
            .iter()
            .filter(|n| Some(&n.id) != self.selected.as_ref())
            .filter_map(|n| {
                let d = Pos2::new(n.x, n.y) - from;
                let ahead = d.dot(dir);
                let aside = (d - dir * ahead).length();
                (ahead > 0.0 && aside <= ahead * 2.0).then_some((n.id.clone(), ahead + aside * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(id, _)| id)
    }

    /// Walk the drawn edges: child, parent, next or prev sibling.
    fn walk(&mut self, dir: &str) {
        let Some(at) = self.selected.clone() else {
            self.msg = "select a node first".into();
            return;
        };
        let here = self.center(&at).unwrap_or_default();
        let near = |ids: Vec<String>, app: &App| {
            ids.into_iter().min_by(|a, b| {
                let da = app.center(a).map_or(f32::MAX, |p| p.distance(here));
                let db = app.center(b).map_or(f32::MAX, |p| p.distance(here));
                da.total_cmp(&db)
            })
        };
        let kids = |id: &str, app: &App| -> Vec<String> { app.scene.edges.iter().filter(|e| e.from == id).map(|e| e.to.clone()).collect() };
        let parents: Vec<String> = self.scene.edges.iter().filter(|e| e.to == at).map(|e| e.from.clone()).collect();
        let back = self.via.clone().filter(|v| parents.contains(v));

        let hop = match dir {
            "child" => near(kids(&at, self), self).map(|n| (n, Some(at.clone()))),
            "parent" => back.or_else(|| near(parents.clone(), self)).map(|n| (n, None)),
            _ => back.or_else(|| near(parents.clone(), self)).and_then(|p| {
                let mut sibs = kids(&p, self);
                sibs.sort_by(|a, b| {
                    let (pa, pb) = (self.center(a).unwrap_or_default(), self.center(b).unwrap_or_default());
                    pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x))
                });
                let i = sibs.iter().position(|s| *s == at)?;
                let step = if dir == "next" { 1 } else { sibs.len() - 1 };
                (sibs.len() > 1).then(|| (sibs[(i + step) % sibs.len()].clone(), Some(p)))
            }),
        };
        match hop {
            Some((n, via)) => {
                self.via = via;
                self.select(Some(n));
            }
            None => self.msg = "no drawn edge that way".into(),
        }
    }

    /// A new node: required fields from the anchor (same Kind as its
    /// neighbour) else the first allowed value; placed next to the anchor.
    fn new_node(&self, name: &str, from: Option<&str>, dx: f32) -> Edit {
        let m = &self.full;
        let near = from.and_then(|a| m.nodes.iter().find(|n| n.name == a));
        let value = |field: &str| near.and_then(|n| n.fields.iter().find(|(f, _)| f == field).map(|(_, v)| v.clone()));
        let mut fields = vec![(m.names.node_key.clone(), name.to_string())];
        for i in m.node_form.iter().filter(|i| i.required && i.field != m.names.node_key) {
            fields.push((
                i.field.clone(),
                value(&i.field).or_else(|| i.options.first().cloned()).unwrap_or_default(),
            ));
        }
        if let Some(g) = near.and_then(|n| n.group.clone()) {
            fields.push((m.names.group.clone(), g));
        }
        if let Some((x, y)) = near.and_then(|n| n.pos).filter(|_| m.nodes.iter().any(|n| n.pos.is_some())) {
            let stacked = m.edges.iter().filter(|e| Some(e.from.as_str()) == from).count() as f32;
            fields.push((m.names.pos.clone(), format!("{} {}", x + dx as i64, y + (STEP.y * stacked) as i64)));
        }
        Edit::AddNode { fields }
    }

    /// A new edge takes the kind most edges around the anchor have.
    fn new_edge(&self, a: &str, b: &str, around: &str) -> Edit {
        let m = &self.full;
        let mut fields = vec![(m.names.from.clone(), a.to_string()), (m.names.to.clone(), b.to_string())];
        if let Some((field, kinds)) = &m.edge_kinds {
            let near: Vec<&str> = m
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
    }

    pub fn submit(&mut self) {
        let Mode::Prompt(kind) = self.mode else { return };
        let name = std::mem::take(&mut self.ask).trim().to_string();
        crate::file::log("submit", &format!("{kind:?} {name:?}"));
        self.mode = Mode::Normal;
        if name.is_empty() {
            return;
        }
        let a = self.anchor();
        let then = Some(format!("n:{name}"));
        match (kind, a.as_deref()) {
            (Ask::Search, _) => self.search(&name),
            (Ask::Loose, a) => {
                let e = self.new_node(&name, a, STEP.x);
                self.send(e, then);
            }
            (Ask::Child, Some(a)) => {
                let e = Edit::Batch {
                    edits: vec![self.new_node(&name, Some(a), STEP.x), self.new_edge(a, &name, a)],
                };
                self.send(e, then);
            }
            (Ask::Parent, Some(a)) => {
                let e = Edit::Batch {
                    edits: vec![self.new_node(&name, Some(a), -STEP.x), self.new_edge(&name, a, a)],
                };
                self.send(e, then);
            }
            (Ask::Connect, Some(a)) => {
                let e = self.new_edge(a, &name, a);
                self.send(e, Some(format!("n:{a}")));
            }
            (Ask::Group, Some(a)) => {
                let group = self.full.nodes.iter().find(|n| n.name == a).and_then(|n| n.group.clone());
                let names = self.full.names.clone();
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
                self.send(
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
            (_, None) => self.msg = "select a node first (arrows, or click one)".into(),
        }
    }

    /// Search names, labels, tags and box labels; smartcase as in vim.
    fn search(&mut self, q: &str) {
        let smart = q == q.to_lowercase();
        let has = |s: &str| if smart { s.to_lowercase().contains(q) } else { s.contains(q) };
        let mut found: Vec<(String, f32, f32)> = Vec::new();
        for n in &self.scene.nodes {
            let tags = self
                .full
                .nodes
                .iter()
                .find(|x| x.name == n.name)
                .map(|x| x.tags.clone())
                .unwrap_or_default();
            if has(&n.label) || has(&n.name) || tags.iter().any(|t| has(t)) {
                found.push((n.id.clone(), n.y, n.x));
            }
        }
        for b in &self.scene.boxes {
            if has(&b.label) {
                found.push((b.id.clone(), f32::MIN, 0.0));
            }
        }
        found.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.2.total_cmp(&b.2)));
        self.hits = found.into_iter().map(|f| f.0).collect();
        self.hit = 0;
        self.msg = format!("{} matches for {q:?}", self.hits.len());
        if let Some(h) = self.hits.first().cloned() {
            self.select(Some(h));
        }
    }

    fn folds_now(&self) -> Vec<String> {
        let Some(t) = &self.tree else { return Vec::new() };
        t.view
            .as_ref()
            .and_then(|v| self.full.views.iter().find(|x| &x.name == v).map(|x| x.collapse.clone()))
            .unwrap_or_else(|| t.spec.collapse.clone())
    }

    fn set_folds(&mut self, names: Vec<String>) {
        let Some(t) = self.tree.clone() else { return };
        match t.view {
            Some(v) => self.send(
                Edit::SetList {
                    kind: "View".into(),
                    key: v,
                    field: "Collapse".into(),
                    values: names,
                },
                None,
            ),
            None => {
                if let Some(t) = &mut self.tree {
                    t.spec.collapse = names;
                }
                self.rebuild();
            }
        }
    }

    /// za: fold whatever is selected.
    fn fold(&mut self) {
        let Some(at) = self.selected.clone() else {
            self.msg = "select something to fold".into();
            return;
        };
        if let Some(g) = at.strip_prefix("g:").map(str::to_string).filter(|_| self.tree.is_none()) {
            let shut = self.full.groups.iter().any(|x| x.name == g && x.collapsed);
            let value = if shut { String::new() } else { "yes".to_string() };
            self.send(
                Edit::Set {
                    kind: "Group".into(),
                    key: g.clone(),
                    field: "Collapsed".into(),
                    value,
                },
                Some(format!("g:{g}")),
            );
            return;
        }
        if let Some(n) = at.strip_prefix("n:").map(str::to_string) {
            if !self.full.edges.iter().any(|e| e.from == n) {
                self.msg = format!("{n} unlocks nothing; nothing to fold");
                return;
            }
            match self.folds.iter().position(|f| *f == n) {
                Some(i) => {
                    self.folds.remove(i);
                    self.msg = format!("unfolded {n}");
                }
                None => {
                    self.folds.push(n.clone());
                    self.msg = format!("folded {n}");
                }
            }
            self.rebuild();
            return;
        }
        let Some(item) = self.items.iter().find(|i| i.id == at).cloned() else {
            return;
        };
        if item.looped {
            self.msg = format!("{} is already above: a loop", item.name);
            return;
        }
        let opened = self.tree.as_ref().is_some_and(|t| t.spec.open.contains(&item.id));
        if item.repeat || opened {
            if let Some(t) = &mut self.tree {
                match t.spec.open.iter().position(|o| *o == item.id) {
                    Some(p) => drop(t.spec.open.remove(p)),
                    None => t.spec.open.push(item.id.clone()),
                }
            }
            self.rebuild();
            return;
        }
        let mut now = self.folds_now();
        let leaf = item.hidden == 0 && !self.items.iter().any(|c| c.parent.as_ref() == Some(&item.id));
        match now.iter().position(|n| *n == item.name) {
            Some(p) => drop(now.remove(p)),
            None if leaf => {
                self.msg = format!("{} has nothing below it", item.name);
                return;
            }
            None => now.push(item.name.clone()),
        }
        self.set_folds(now);
    }

    fn fold_all(&mut self, close: bool) {
        if self.tree.is_some() {
            if !close {
                if let Some(t) = &mut self.tree {
                    t.spec.open.clear();
                }
                self.set_folds(Vec::new());
                return;
            }
            let mut names: Vec<String> = Vec::new();
            for i in self
                .items
                .iter()
                .filter(|i| self.items.iter().any(|c| c.parent.as_ref() == Some(&i.id)))
            {
                if !names.contains(&i.name) {
                    names.push(i.name.clone());
                }
            }
            self.set_folds(names);
            return;
        }
        if !close {
            self.folds.clear();
        }
        let edits: Vec<Edit> = self
            .full
            .groups
            .iter()
            .filter(|g| g.collapsed != close)
            .map(|g| Edit::Set {
                kind: "Group".into(),
                key: g.name.clone(),
                field: "Collapsed".into(),
                value: if close { "yes".into() } else { String::new() },
            })
            .collect();
        if edits.is_empty() {
            self.rebuild();
            self.msg = if close {
                "every box is folded already".into()
            } else {
                "unfolded everything".into()
            };
            return;
        }
        self.send(Edit::Batch { edits }, None);
    }

    fn open_tree(&mut self, follow_in: bool) {
        let Some(a) = self.anchor() else {
            self.msg = "select a node first".into();
            return;
        };
        let kinds = common_kind(&self.full, &a, follow_in);
        let spec = Spec {
            root: a.clone(),
            follow_in,
            kinds: kinds.clone(),
            direction: if follow_in { "left" } else { "right" }.into(),
            ..Spec::default()
        };
        self.tree = Some(TreeState { spec, view: None });
        self.fitted = false;
        self.rebuild();
        self.selected = Some("t:0".into());
        let along = if kinds.is_empty() {
            "every edge".to_string()
        } else {
            kinds.join(" ")
        };
        self.msg = format!(
            "tree from {a}: {}, along {along}; ; changes the kind, L leaves",
            if follow_in { "what it needs" } else { "what it unlocks" }
        );
    }

    fn next_view(&mut self) {
        let views = self.full.views.clone();
        if views.is_empty() {
            self.msg = "the file has no View records".into();
            return;
        }
        let i = self.view_at.map_or(0, |i| (i + 1) % views.len());
        self.view_at = Some(i);
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
                self.tree = Some(TreeState {
                    spec,
                    view: Some(v.name.clone()),
                });
                self.selected = Some("t:0".into());
            }
            (layout, _) => {
                self.tree = None;
                self.kinds = v.edges.clone();
                self.layout = if layout == "tree" { Layout::Tree } else { Layout::Manual };
            }
        }
        self.fitted = false;
        self.rebuild();
        let filter = v
            .filter
            .as_ref()
            .map(|f| format!(" (its Filter {f:?} is not applied yet)"))
            .unwrap_or_default();
        self.msg = format!("view {} of {}: {}{filter}", i + 1, views.len(), v.name);
    }

    fn next_kind(&mut self) {
        let all = self.full.edge_kinds.clone().map(|(_, v)| v).unwrap_or_default();
        if all.is_empty() {
            self.msg = "this file has no edge kinds".into();
            return;
        }
        let now = self
            .tree
            .as_ref()
            .map(|t| t.spec.kinds.clone())
            .unwrap_or_else(|| self.kinds.clone());
        let next: Vec<String> = match now.as_slice() {
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
        self.msg = format!(
            "edges: {}",
            if next.is_empty() {
                "every kind".to_string()
            } else {
                next.join(" ")
            }
        );
        match &mut self.tree {
            Some(t) => {
                t.spec.kinds = next;
                self.selected = Some("t:0".into());
            }
            None => self.kinds = next,
        }
        self.rebuild();
    }

    fn delete(&mut self) {
        let Some(at) = self.selected.clone() else { return };
        if let Some(g) = at.strip_prefix("g:").filter(|_| self.tree.is_none()) {
            self.send(Edit::DeleteGroup { name: g.to_string() }, None);
        } else if let Some(i) = at.strip_prefix("e:").and_then(|i| i.parse().ok()) {
            self.send(Edit::DeleteEdge { index: i }, None);
        } else if let Some(name) = self.anchor() {
            self.send(Edit::DeleteNode { name }, None);
        }
        self.selected = None;
    }

    /// Positions on screen into the file, then the manual layout.
    fn save_positions(&mut self) {
        if self.tree.is_some() {
            self.msg = "positions come from the tree here; L leaves it".into();
            return;
        }
        let positions: Vec<(String, i64, i64)> = self
            .scene
            .nodes
            .iter()
            .filter(|n| n.id.starts_with("n:"))
            .map(|n| (n.name.clone(), n.x.round() as i64, n.y.round() as i64))
            .collect();
        self.layout = Layout::Manual;
        self.send(Edit::Move { positions }, None);
    }

    fn export(&mut self, ctx: &egui::Context) {
        let out = self.file.path.with_extension("svg");
        match std::fs::write(&out, draw::svg(ctx, &self.scene)) {
            Ok(()) => self.msg = format!("exported {}", out.display()),
            Err(e) => self.msg = format!("export failed: {e}"),
        }
    }

    /// Keys on the canvas. Ignored while a text field has focus.
    fn keys(&mut self, ctx: &egui::Context, canvas: Rect) {
        if ctx.egui_wants_keyboard_input() || self.mode != Mode::Normal {
            return;
        }
        // consume_key ignores Shift on letters ("logically" equal), so a
        // plain key only counts with Shift up, a shifted one with it down.
        let shift = ctx.input(|i| i.modifiers.shift);
        let pressed = |m: Modifiers, k: Key| (m.shift == shift) && ctx.input_mut(|i| i.consume_key(m, k));
        let (s, n) = (Modifiers::SHIFT, Modifiers::NONE);

        // Two-key commands: dd, za, zR, zM, zz, gd.
        if let Some(first) = self.pending {
            let second = [
                (n, Key::D, 'd'),
                (n, Key::A, 'a'),
                (s, Key::R, 'R'),
                (s, Key::M, 'M'),
                (n, Key::Z, 'z'),
            ]
            .into_iter()
            .find(|(m, k, _)| pressed(*m, *k))
            .map(|x| x.2);
            let gd = first == 'g' && pressed(n, Key::D);
            if second.is_none() && !gd && !ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))) {
                return; // no key this frame: keep waiting
            }
            self.pending = None;
            match (first, second) {
                _ if gd => {
                    let first = self
                        .items
                        .iter()
                        .find(|i| Some(&i.id) == self.selected.as_ref())
                        .and_then(|i| i.first.clone());
                    match first {
                        Some(f) => self.select(Some(f)),
                        None => self.msg = "gd jumps from a repeat to its first copy".into(),
                    }
                }
                ('d', Some('d')) => self.delete(),
                ('z', Some('a')) => self.fold(),
                ('z', Some('R')) => self.fold_all(false),
                ('z', Some('M')) => self.fold_all(true),
                ('z', Some('z')) => self.target = self.selected.as_ref().and_then(|s| self.follow_view(canvas, s)),
                _ => {}
            }
            return;
        }

        // # is text, not a key: typed as Shift+3, or by some tools with a
        // stray key code. Read it before the letters so nothing else fires.
        if ctx.input(|i| i.events.iter().any(|e| matches!(e, egui::Event::Text(t) if t == "#"))) {
            ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. })));
            self.focus_tag = true;
            return;
        }

        for (k, dir) in [
            (Key::ArrowRight, "child"),
            (Key::ArrowLeft, "parent"),
            (Key::ArrowDown, "next"),
            (Key::ArrowUp, "prev"),
        ] {
            if pressed(s, k) {
                self.walk(dir);
            }
        }
        for (keys, v) in [
            ([Key::ArrowLeft, Key::H], Vec2::new(-1.0, 0.0)),
            ([Key::ArrowRight, Key::L], Vec2::new(1.0, 0.0)),
            ([Key::ArrowUp, Key::K], Vec2::new(0.0, -1.0)),
            ([Key::ArrowDown, Key::J], Vec2::new(0.0, 1.0)),
        ] {
            if keys.iter().any(|k| pressed(n, *k))
                && let Some(next) = self.nearest(v)
            {
                self.select(Some(next));
            }
        }
        let ask = |app: &mut App, a: Ask| {
            crate::file::log("prompt", &format!("{a:?}"));
            app.mode = Mode::Prompt(a);
            app.ask.clear();
        };
        if pressed(n, Key::D) {
            self.pending = Some('d');
        } else if pressed(n, Key::Z) {
            self.pending = Some('z');
        } else if pressed(n, Key::G) {
            self.pending = Some('g');
        } else if pressed(n, Key::O) {
            ask(self, Ask::Child);
        } else if pressed(s, Key::O) {
            ask(self, Ask::Parent);
        } else if pressed(n, Key::A) {
            ask(self, Ask::Loose);
        } else if pressed(n, Key::C) {
            ask(self, Ask::Connect);
        } else if pressed(n, Key::Slash) {
            ask(self, Ask::Search);
        } else if pressed(s, Key::G) {
            ask(self, Ask::Group);
        } else if pressed(n, Key::U) {
            self.history(true);
        } else if pressed(Modifiers::CTRL, Key::R) {
            self.history(false);
        } else if pressed(n, Key::Enter) || pressed(n, Key::I) {
            self.focus_panel = true;
        } else if pressed(n, Key::E) {
            self.step_edge();
        } else if pressed(n, Key::N) {
            self.next_hit(true);
        } else if pressed(s, Key::N) {
            self.next_hit(false);
        } else if pressed(n, Key::T) {
            self.open_tree(false);
        } else if pressed(s, Key::T) {
            self.open_tree(true);
        } else if pressed(s, Key::V) {
            self.next_view();
        } else if pressed(n, Key::Semicolon) {
            self.next_kind();
        } else if pressed(s, Key::L) {
            self.toggle_layout();
        } else if pressed(n, Key::S) {
            self.save_positions();
        } else if pressed(n, Key::F) {
            self.target = Some(self.fit_view(canvas));
        } else if pressed(s, Key::F) {
            self.follow = !self.follow;
            self.msg = format!(
                "follow {}",
                if self.follow {
                    "on: the view stays on your selection"
                } else {
                    "off"
                }
            );
        } else if pressed(s, Key::E) {
            self.export(ctx);
        } else if pressed(s, Key::Questionmark) || pressed(s, Key::Slash) {
            self.mode = Mode::Help;
        } else if pressed(n, Key::Escape) {
            if self.tag_filter.take().is_some() {
                self.msg = "tag filter cleared".into();
                self.rebuild();
            } else {
                self.selected = None;
                self.refused.clear();
            }
        }
    }

    fn step_edge(&mut self) {
        let Some(a) = self.anchor() else { return };
        let ids: Vec<String> = self
            .full
            .edges
            .iter()
            .filter(|e| e.from == a || e.to == a)
            .map(|e| format!("e:{}", e.index))
            .collect();
        let at = self.selected.clone().unwrap_or_default();
        let next = ids.iter().position(|i| *i == at).map_or(0, |p| (p + 1) % ids.len().max(1));
        match ids.get(next) {
            Some(id) => self.selected = Some(id.clone()),
            None => self.msg = format!("{a} has no edges"),
        }
    }

    fn next_hit(&mut self, forward: bool) {
        if self.hits.is_empty() {
            self.msg = "no search; press /".into();
            return;
        }
        let len = self.hits.len();
        self.hit = if forward {
            (self.hit + 1) % len
        } else {
            (self.hit + len - 1) % len
        };
        let h = self.hits[self.hit].clone();
        self.select(Some(h));
        self.msg = format!("match {} of {len}", self.hit + 1);
    }

    fn toggle_layout(&mut self) {
        if self.tree.take().is_some() {
            self.selected = None;
            self.msg = format!("back to layout {}", self.layout.name());
        } else {
            self.layout = if self.layout == Layout::Manual {
                Layout::Tree
            } else {
                Layout::Manual
            };
            self.msg = format!("layout {}", self.layout.name());
        }
        self.fitted = false;
        self.rebuild();
    }

    /// The canvas: input, the glide, geometry and painting.
    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = resp.rect;
        painter.rect_filled(canvas, 0.0, BG);
        if !self.fitted {
            self.view = self.fit_view(canvas);
            self.fitted = true;
        }

        // Drag a node or box in the manual layout; drag the background to pan.
        if resp.drag_started()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let on = self
                .geo
                .hit(p)
                .filter(|id| self.tree.is_none() && self.layout == Layout::Manual && (id.starts_with("n:") || id.starts_with("g:")));
            self.drag = on.map(|id| (id, Vec2::ZERO));
        }
        if resp.dragged() {
            let d = resp.drag_delta();
            match &mut self.drag {
                Some((_, moved)) => *moved += d / self.view.zoom,
                None => self.view.pan += d,
            }
            self.target = None;
        }
        if resp.drag_stopped()
            && let Some((id, moved)) = self.drag.take()
            && moved.length() > 0.5
        {
            let names = self.dragged_names(&id);
            let positions = names
                .iter()
                .filter_map(|n| self.scene.nodes.iter().find(|x| x.id == format!("n:{n}")))
                .map(|x| (x.name.clone(), (x.x + moved.x).round() as i64, (x.y + moved.y).round() as i64))
                .collect();
            self.send(Edit::Move { positions }, Some(id));
        }

        if resp.hovered() {
            let (scroll, zoom, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0
                && let Some(p) = pointer
            {
                let before = (p - canvas.min - self.view.pan) / self.view.zoom;
                self.view.zoom = (self.view.zoom * zoom).clamp(ZOOM_RANGE.0, ZOOM_RANGE.1);
                self.view.pan = p - canvas.min - before * self.view.zoom;
                self.target = None;
            } else if scroll != Vec2::ZERO {
                self.view.pan += scroll;
                self.target = None;
            }
        }

        let before = self.selected.clone();
        self.keys(ui.ctx(), canvas);
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            self.via = None;
            let hit = self.geo.hit(p);
            self.select(hit);
        }
        if self.follow
            && self.selected != before
            && let Some(v) = self.selected.as_ref().and_then(|s| self.follow_view(canvas, s))
        {
            self.target = Some(v);
        }

        if let Some(t) = self.target {
            self.view.pan += (t.pan - self.view.pan) * GLIDE;
            self.view.zoom += (t.zoom - self.view.zoom) * GLIDE;
            if (t.pan - self.view.pan).length() < 0.5 && (t.zoom - self.view.zoom).abs() < 0.002 {
                self.view = t;
                self.target = None;
            }
            ui.ctx().request_repaint();
        }

        // Geometry with the drag applied, then paint.
        let (view, drag) = (self.view, self.drag.clone());
        let moving: Vec<String> = drag.as_ref().map(|(id, _)| self.dragged_names(id)).unwrap_or_default();
        let mut scene = self.scene.clone();
        if let Some((_, d)) = &drag {
            for n in scene.nodes.iter_mut().filter(|n| moving.contains(&n.name)) {
                n.x += d.x;
                n.y += d.y;
            }
        }
        let ctx = ui.ctx().clone();
        let measure =
            |s: &str, size: f32| ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), egui::FontId::proportional(size), TEXT).size());
        let to = |p: Pos2| canvas.min + view.pan + p.to_vec2() * view.zoom;
        self.geo = draw::geometry(&scene, &measure, &to, view.zoom);
        let painter = painter.with_clip_rect(canvas);
        draw::paint(
            &painter,
            &scene,
            &self.geo,
            &draw::Marks {
                selected: self.selected.as_deref(),
                hits: &self.hits,
            },
        );
    }

    /// The nodes a drag moves: the node, or every node inside a box.
    fn dragged_names(&self, id: &str) -> Vec<String> {
        if let Some(n) = id.strip_prefix("n:") {
            return vec![n.to_string()];
        }
        let Some(g) = id.strip_prefix("g:") else { return Vec::new() };
        let mut inside = vec![g.to_string()];
        let mut i = 0;
        while i < inside.len() {
            let kids: Vec<String> = self
                .full
                .groups
                .iter()
                .filter(|x| x.parent.as_deref() == Some(inside[i].as_str()))
                .map(|x| x.name.clone())
                .collect();
            inside.extend(kids);
            i += 1;
        }
        self.full
            .nodes
            .iter()
            .filter(|n| n.group.as_ref().is_some_and(|g| inside.contains(g)))
            .map(|n| n.name.clone())
            .collect()
    }

    fn status(&self, ui: &mut egui::Ui) {
        let mode = match (self.mode, self.pending) {
            (Mode::Normal, Some(c)) => c.to_string(),
            (Mode::Normal, None) => "NORMAL".into(),
            (Mode::Prompt(_), _) => "PROMPT".into(),
            (Mode::Help, _) => "HELP".into(),
        };
        let place = match &self.tree {
            Some(TreeState { view: Some(v), .. }) => format!("view {v}"),
            Some(t) => format!("tree {} ({})", t.spec.root, if t.spec.follow_in { "needs" } else { "unlocks" }),
            None => {
                let kinds = if self.kinds.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", self.kinds.join(" "))
                };
                let tag = self.tag_filter.as_ref().map(|t| format!(" · tag {t}")).unwrap_or_default();
                format!("layout {}{kinds}{tag}", self.layout.name())
            }
        };
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(mode).color(draw::ACCENT).strong());
            if self.follow {
                ui.label(egui::RichText::new("FOLLOW").color(draw::ACCENT).strong());
            }
            ui.label(self.file.path.file_name().and_then(|f| f.to_str()).unwrap_or_default());
            ui.label(place);
            ui.label(format!("{} nodes · {} edges", self.full.nodes.len(), self.full.edges.len()));
            let p = self.full.problems.len();
            ui.label(egui::RichText::new(format!("{p} problems")).color(if p > 0 { draw::BAD } else { draw::MUTED }));
            ui.label(egui::RichText::new(&self.msg).color(draw::MUTED));
        });
    }

    fn prompt(&mut self, ctx: &egui::Context) {
        let Mode::Prompt(kind) = self.mode else { return };
        let mut done = None;
        egui::Window::new("prompt")
            .title_bar(false)
            .resizable(false)
            .collapsible(false)
            .anchor(Align2::CENTER_TOP, [0.0, 140.0])
            .fixed_size([420.0, 60.0])
            .show(ctx, |ui| {
                ui.label(egui::RichText::new(kind.label()).color(draw::MUTED));
                let r = ui.add(egui::TextEdit::singleline(&mut self.ask).desired_width(f32::INFINITY));
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Enter)) {
                    done = Some(true);
                } else if ui.input(|i| i.key_pressed(Key::Escape)) {
                    done = Some(false);
                }
            });
        match done {
            Some(true) => self.submit(),
            Some(false) => {
                self.mode = Mode::Normal;
                self.ask.clear();
            }
            None => {}
        }
    }

    fn help(&mut self, ctx: &egui::Context) {
        if self.mode != Mode::Help {
            return;
        }
        let mut open = true;
        egui::Window::new("draw_graph keys").open(&mut open).collapsible(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).default_width(620.0).show(ctx, |ui| {
            ui.label(egui::RichText::new("Every accepted edit is written to the file at once; an edit that breaks a rule is refused and the file is left as it was. Changes made by vim, git or an agent show up as soon as they land.").color(draw::MUTED));
            ui.separator();
            egui::ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                egui::Grid::new("keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
                    for (k, d) in KEYS {
                        ui.label(egui::RichText::new(*k).color(draw::ACCENT).monospace());
                        ui.label(*d);
                        ui.end_row();
                    }
                });
            });
        });
        if !open || ctx.input(|i| i.key_pressed(Key::Escape) || i.key_pressed(Key::Q)) {
            self.mode = Mode::Normal;
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.changes.try_recv().is_ok() {
            while self.changes.try_recv().is_ok() {}
            if self.file.reload() {
                self.rebuild();
                self.msg = "reloaded: the file changed on disk".into();
            }
        }
        let ctx = ui.ctx().clone();
        egui::Panel::bottom("status").show(ui, |ui| self.status(ui));
        egui::Panel::right("panel")
            .default_size(340.0)
            .show(ui, |ui| crate::panel::show(self, ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
        self.prompt(&ctx);
        self.help(&ctx);

        // Edits the panel asked for.
        for (edit, then) in std::mem::take(&mut self.outbox) {
            self.send(edit, then);
        }
    }
}
