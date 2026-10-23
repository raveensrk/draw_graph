//! Prototype: draw_graph in egui, one native window with no web view.
//!
//!   ┌──────────────────────────────┬──────────────┐
//!   │ canvas: painted every frame  │ side panel:  │
//!   │ from core's model            │ the selected │
//!   │                              │ node, read   │
//!   ├──────────────────────────────┴──────────────┤
//!   │ status: file · counts · last message        │
//!   └─────────────────────────────────────────────┘
//!
//! Read only: it opens a recfile, draws it at its saved Pos, moves around
//! it and follows the file on disk. Everything it knows comes from
//! draw_graph_core, the same model the Tauri app draws.
//!
//!   draw_graph_egui FILE.rec

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use draw_graph_core::model::{Model, branches, folded, model};
use eframe::egui::{self, Align2, Color32, FontId, Key, Modifiers, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Vec2};
use notify::{EventKind, RecursiveMode, Watcher};

// Dark theme, the Tauri app's colours.
const BG: Color32 = Color32::from_rgb(0x0f, 0x11, 0x15);
const LINE: Color32 = Color32::from_rgb(0x2a, 0x2f, 0x3a);
const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xb2);
const ACCENT: Color32 = Color32::from_rgb(0xe0, 0xb8, 0x4a);

/// Node label size and padding in graph units; zoom scales them.
const FONT: f32 = 12.0;
const PAD: Vec2 = Vec2::new(10.0, 6.0);
const BOX_PAD: f32 = 16.0;

/// Follow mode's zoom, and how fast the view glides there (per frame, 0..1).
const FOLLOW_ZOOM: f32 = 1.3;
const GLIDE: f32 = 0.25;

/// Grid for nodes that have no Pos, in graph units.
const GRID: Vec2 = Vec2::new(200.0, 60.0);
const GRID_COLS: usize = 8;

#[derive(Clone, Copy, PartialEq)]
struct View {
    pan: Vec2,
    zoom: f32,
}

struct App {
    path: PathBuf,
    /// The file as read; `shown` is it with folds applied, as drawn.
    full: Model,
    shown: Model,
    /// Where each node sits, in graph units.
    at: HashMap<String, Pos2>,
    view: View,
    /// Where the view is gliding to (follow, fit); None when still.
    target: Option<View>,
    selected: Option<String>,
    /// The parent an edge walk came from, for siblings and the way back.
    via: Option<String>,
    folds: Vec<String>,
    follow: bool,
    pending: Option<char>,
    fitted: bool,
    msg: String,
    changes: Receiver<()>,
    _watcher: Option<notify::RecommendedWatcher>,
}

fn color(hex: &str) -> Color32 {
    let v = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0x2a3140);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

impl App {
    fn new(cc: &eframe::CreationContext, path: PathBuf) -> App {
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        // Watch the folder (editors replace files) and wake the window.
        let (tx, changes) = channel();
        let ctx = cc.egui_ctx.clone();
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

        let mut app = App {
            path,
            full: Model::default(),
            shown: Model::default(),
            at: HashMap::new(),
            view: View {
                pan: Vec2::ZERO,
                zoom: 1.0,
            },
            target: None,
            selected: None,
            via: None,
            folds: Vec::new(),
            follow: false,
            pending: None,
            fitted: false,
            msg: "arrows, Shift + arrows, za, zR, F follow, zz, f fit, Esc".into(),
            changes,
            _watcher: watcher,
        };
        app.load();
        app
    }

    fn load(&mut self) {
        match std::fs::read_to_string(&self.path) {
            Ok(text) => self.full = model(&text),
            Err(e) => self.msg = format!("cannot read {}: {e}", self.path.display()),
        }
        self.refold();
    }

    /// Apply the box folds and branch folds; place what has no Pos.
    fn refold(&mut self) {
        self.shown = branches(&folded(&self.full), &self.folds);
        self.at.clear();
        let mut loose = 0;
        for n in &self.shown.nodes {
            let p = match n.pos {
                Some((x, y)) => Pos2::new(x as f32, y as f32),
                None => {
                    loose += 1;
                    Pos2::new(
                        GRID.x * ((loose - 1) % GRID_COLS) as f32,
                        -GRID.y * (1 + (loose - 1) / GRID_COLS) as f32,
                    )
                }
            };
            self.at.insert(n.name.clone(), p);
        }
        for g in self.shown.groups.iter().filter(|g| g.collapsed) {
            if let Some((x, y)) = g.pos {
                self.at.insert(g.name.clone(), Pos2::new(x as f32, y as f32));
            }
        }
    }

    fn to_screen(&self, canvas: Rect, p: Pos2) -> Pos2 {
        canvas.min + self.view.pan + p.to_vec2() * self.view.zoom
    }

    /// The view that shows the whole graph in the canvas.
    fn fit_view(&self, canvas: Rect) -> View {
        let pts: Vec<Pos2> = self.at.values().copied().collect();
        if pts.is_empty() {
            return self.view;
        }
        let bb = Rect::from_points(&pts).expand(80.0);
        let zoom = (canvas.width() / bb.width()).min(canvas.height() / bb.height()).clamp(0.05, 2.0);
        View {
            zoom,
            pan: canvas.size() / 2.0 - bb.center().to_vec2() * zoom,
        }
    }

    /// The view that puts one name in the middle at FOLLOW_ZOOM.
    fn follow_view(&self, canvas: Rect, name: &str) -> Option<View> {
        let p = *self.at.get(name)?;
        Some(View {
            zoom: FOLLOW_ZOOM,
            pan: canvas.size() / 2.0 - p.to_vec2() * FOLLOW_ZOOM,
        })
    }

    /// The nearest drawn node in a direction, as canvas.js nearest() does:
    /// in a 63° cone, scored by distance ahead plus twice the distance aside.
    fn nearest(&self, dir: Vec2) -> Option<String> {
        let Some(from) = self.selected.as_ref().and_then(|s| self.at.get(s)) else {
            return self.shown.nodes.first().map(|n| n.name.clone());
        };
        self.at
            .iter()
            .filter(|(n, _)| Some(*n) != self.selected.as_ref())
            .filter_map(|(n, p)| {
                let d = *p - *from;
                let ahead = d.dot(dir);
                let aside = (d - dir * ahead).length();
                (ahead > 0.0 && aside <= ahead * 2.0).then_some((n, ahead + aside * 2.0))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(n, _)| n.clone())
    }

    /// Walk the drawn edges, as canvas.js walk() does.
    fn walk(&mut self, dir: &str) {
        let Some(at) = self.selected.clone() else {
            self.msg = "select a node first".into();
            return;
        };
        let here = self.at.get(&at).copied().unwrap_or_default();
        let near = |names: Vec<String>, at: &HashMap<String, Pos2>| {
            names.into_iter().min_by(|a, b| {
                let da = at.get(a).map_or(f32::MAX, |p| p.distance(here));
                let db = at.get(b).map_or(f32::MAX, |p| p.distance(here));
                da.total_cmp(&db)
            })
        };
        let kids = |n: &str| -> Vec<String> {
            self.shown
                .edges
                .iter()
                .filter(|e| e.from == n && self.at.contains_key(&e.to))
                .map(|e| e.to.clone())
                .collect()
        };
        let parents: Vec<String> = self
            .shown
            .edges
            .iter()
            .filter(|e| e.to == at && self.at.contains_key(&e.from))
            .map(|e| e.from.clone())
            .collect();

        let hop = match dir {
            "child" => near(kids(&at), &self.at).map(|n| (n, Some(at.clone()))),
            "parent" => {
                let back = self.via.clone().filter(|v| parents.contains(v));
                back.or_else(|| near(parents.clone(), &self.at)).map(|n| (n, None))
            }
            _ => {
                let parent = self
                    .via
                    .clone()
                    .filter(|v| parents.contains(v))
                    .or_else(|| near(parents.clone(), &self.at));
                parent.and_then(|p| {
                    let mut sibs = kids(&p);
                    sibs.sort_by(|a, b| {
                        let (pa, pb) = (self.at[a], self.at[b]);
                        pa.y.total_cmp(&pb.y).then(pa.x.total_cmp(&pb.x))
                    });
                    let i = sibs.iter().position(|s| *s == at)?;
                    let step = if dir == "next" { 1 } else { sibs.len() - 1 };
                    (sibs.len() > 1).then(|| (sibs[(i + step) % sibs.len()].clone(), Some(p)))
                })
            }
        };
        match hop {
            Some((n, via)) => {
                self.via = via;
                self.select(Some(n));
            }
            None => self.msg = "no drawn edge that way".into(),
        }
    }

    fn select(&mut self, name: Option<String>) {
        self.selected = name;
        self.msg.clear();
    }

    fn keys(&mut self, ctx: &egui::Context, canvas: Rect) {
        let (mut go, mut walk) = (None, None);
        let mut toggle = None::<&str>;
        ctx.input_mut(|i| {
            for (k, d) in [
                (Key::ArrowRight, "child"),
                (Key::ArrowLeft, "parent"),
                (Key::ArrowDown, "next"),
                (Key::ArrowUp, "prev"),
            ] {
                if i.consume_key(Modifiers::SHIFT, k) {
                    walk = Some(d);
                }
            }
            for (keys, v) in [
                ([Key::ArrowLeft, Key::H], Vec2::new(-1.0, 0.0)),
                ([Key::ArrowRight, Key::L], Vec2::new(1.0, 0.0)),
                ([Key::ArrowUp, Key::K], Vec2::new(0.0, -1.0)),
                ([Key::ArrowDown, Key::J], Vec2::new(0.0, 1.0)),
            ] {
                if keys.iter().any(|k| i.consume_key(Modifiers::NONE, *k)) {
                    go = Some(v);
                }
            }
            if i.consume_key(Modifiers::SHIFT, Key::F) {
                toggle = Some("follow");
            } else if i.consume_key(Modifiers::NONE, Key::F) {
                toggle = Some("fit");
            } else if i.consume_key(Modifiers::NONE, Key::Escape) {
                toggle = Some("escape");
            } else if i.consume_key(Modifiers::NONE, Key::Z) {
                toggle = Some("z");
            } else if i.consume_key(Modifiers::NONE, Key::A) {
                toggle = Some("a");
            } else if i.consume_key(Modifiers::SHIFT, Key::R) {
                toggle = Some("R");
            }
        });

        if let Some(d) = walk {
            self.pending = None;
            self.walk(d);
        }
        if let Some(v) = go {
            self.pending = None;
            if let Some(n) = self.nearest(v) {
                self.select(Some(n));
            }
        }
        // A pending z waits for the next key; frames without one keep it.
        if toggle.is_none() {
            return;
        }
        match (self.pending.take(), toggle) {
            (_, Some("follow")) => {
                self.follow = !self.follow;
                self.msg = format!("follow {}", if self.follow { "on" } else { "off" });
            }
            (_, Some("fit")) => self.target = Some(self.fit_view(canvas)),
            (_, Some("escape")) => self.select(None),
            (Some('z'), Some("a")) => self.fold(),
            (Some('z'), Some("z")) => self.target = self.selected.as_ref().and_then(|s| self.follow_view(canvas, s)),
            (Some('z'), Some("R")) => {
                self.folds.clear();
                self.refold();
                self.msg = "unfolded everything".into();
            }
            (_, Some("z")) => self.pending = Some('z'),
            _ => {}
        }
    }

    /// za: fold or unfold the selected node's branch.
    fn fold(&mut self) {
        let Some(n) = self.selected.clone() else { return };
        if !self.full.edges.iter().any(|e| e.from == n) {
            self.msg = format!("{n} unlocks nothing");
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
        self.refold();
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = resp.rect;
        painter.rect_filled(canvas, 0.0, BG);
        if !self.fitted {
            self.view = self.fit_view(canvas);
            self.fitted = true;
        }

        // Pan by dragging or scrolling; zoom with pinch or ctrl/cmd + scroll,
        // around the pointer.
        if resp.dragged() {
            self.view.pan += resp.drag_delta();
            self.target = None;
        }
        if resp.hovered() {
            let (scroll, zoom, pointer) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta(), i.pointer.hover_pos()));
            if zoom != 1.0
                && let Some(p) = pointer
            {
                let before = (p - canvas.min - self.view.pan) / self.view.zoom;
                self.view.zoom = (self.view.zoom * zoom).clamp(0.05, 4.0);
                self.view.pan = p - canvas.min - before * self.view.zoom;
                self.target = None;
            } else if scroll != Vec2::ZERO {
                self.view.pan += scroll;
                self.target = None;
            }
        }
        self.keys(ui.ctx(), canvas);
        if self.follow
            && let Some(v) = self.selected.as_ref().and_then(|s| self.follow_view(canvas, s))
            && self.target.is_none()
            && (v.pan - self.view.pan).length() > 0.5
        {
            self.target = Some(v);
        }

        // Glide toward the target view.
        if let Some(t) = self.target {
            self.view.pan += (t.pan - self.view.pan) * GLIDE;
            self.view.zoom += (t.zoom - self.view.zoom) * GLIDE;
            if (t.pan - self.view.pan).length() < 0.5 && (t.zoom - self.view.zoom).abs() < 0.002 {
                self.view = t;
                self.target = None;
            }
            ui.ctx().request_repaint();
        }

        // Node rectangles on screen: label size plus padding.
        let z = self.view.zoom;
        let font = FontId::proportional(FONT * z);
        let mut rects: HashMap<String, Rect> = HashMap::new();
        let mut labels: Vec<(String, String, Color32, bool)> = Vec::new();
        for n in &self.shown.nodes {
            // egui's built-in fonts lack ▸ and arrows: plain ASCII marks.
            let label = if n.hidden > 0 {
                format!("{} +{}", n.label, n.hidden)
            } else {
                n.label.clone()
            };
            labels.push((n.name.clone(), label, color(&n.color), n.hidden > 0));
        }
        for g in self.shown.groups.iter().filter(|g| g.collapsed && self.at.contains_key(&g.name)) {
            labels.push((
                g.name.clone(),
                format!("{} +{}", g.label, g.hidden),
                color(g.color.as_deref().unwrap_or("#3a4150")),
                true,
            ));
        }
        let galleys: Vec<_> = labels
            .iter()
            .map(|(name, label, fill, folded)| {
                let g = painter.layout_no_wrap(label.clone(), font.clone(), TEXT);
                let c = self.to_screen(canvas, self.at[name]);
                let r = Rect::from_center_size(c, g.size() + PAD * 2.0 * z);
                rects.insert(name.clone(), r);
                (name.clone(), g, *fill, *folded, r)
            })
            .collect();

        // Group boxes around their members, inner boxes first so outer
        // ones can wrap them.
        let depth = |g: &str| {
            let mut d = 0;
            let mut at = self.shown.groups.iter().find(|x| x.name == g).and_then(|x| x.parent.clone());
            while let Some(p) = at {
                d += 1;
                at = self.shown.groups.iter().find(|x| x.name == p).and_then(|x| x.parent.clone());
                if d > self.shown.groups.len() {
                    break;
                }
            }
            d
        };
        let mut open: Vec<_> = self.shown.groups.iter().filter(|g| !g.collapsed).collect();
        open.sort_by_key(|g| std::cmp::Reverse(depth(&g.name)));
        let mut boxes: Vec<(String, Rect, usize)> = Vec::new();
        for g in open {
            let inside: Vec<Rect> = self
                .shown
                .nodes
                .iter()
                .filter(|n| n.group.as_deref() == Some(g.name.as_str()))
                .filter_map(|n| rects.get(&n.name).copied())
                .chain(
                    self.shown
                        .groups
                        .iter()
                        .filter(|c| c.parent.as_deref() == Some(g.name.as_str()))
                        .filter_map(|c| rects.get(&c.name).copied()),
                )
                .collect();
            let Some(first) = inside.first() else { continue };
            let r = inside.iter().fold(*first, |a, b| a.union(*b)).expand(BOX_PAD * z);
            rects.insert(g.name.clone(), r);
            boxes.push((g.label.clone(), r, depth(&g.name)));
        }
        boxes.sort_by_key(|b| b.2);
        for (label, r, _) in &boxes {
            painter.rect_filled(*r, 10.0 * z, Color32::from_rgba_unmultiplied(0x3a, 0x41, 0x50, 46));
            painter.rect_stroke(*r, 10.0 * z, Stroke::new(1.0, LINE), StrokeKind::Inside);
            painter.text(
                r.center_top() + Vec2::new(0.0, -2.0),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(12.0 * z),
                MUTED,
            );
        }

        // Edges: from border to border, with an arrowhead.
        let clip = |r: Rect, toward: Pos2| {
            let d = toward - r.center();
            let t = (r.width() / 2.0 / d.x.abs().max(1e-3)).min(r.height() / 2.0 / d.y.abs().max(1e-3));
            r.center() + d * t.min(1.0)
        };
        for e in &self.shown.edges {
            let (Some(a), Some(b)) = (rects.get(&e.from), rects.get(&e.to)) else {
                continue;
            };
            let (from, to) = (clip(*a, b.center()), clip(*b, a.center()));
            let on = self.selected.as_ref().is_some_and(|s| *s == e.from || *s == e.to);
            let stroke = Stroke::new(if on { 2.0 } else { 1.2 }, if on { ACCENT } else { MUTED });
            match e.style.as_str() {
                "dashed" => {
                    painter.add(Shape::Vec(Shape::dashed_line(&[from, to], stroke, 6.0 * z, 5.0 * z)));
                }
                "dotted" => {
                    painter.add(Shape::Vec(Shape::dashed_line(&[from, to], stroke, 1.5 * z, 4.0 * z)));
                }
                _ => {
                    painter.line_segment([from, to], stroke);
                }
            }
            let dir = (to - from).normalized();
            let side = Vec2::new(-dir.y, dir.x);
            let head = 8.0 * z.sqrt();
            painter.add(Shape::convex_polygon(
                vec![to, to - dir * head + side * head * 0.5, to - dir * head - side * head * 0.5],
                stroke.color,
                Stroke::NONE,
            ));
            if let Some(l) = &e.label {
                painter.text(
                    from + (to - from) / 2.0,
                    Align2::CENTER_CENTER,
                    l,
                    FontId::proportional(11.0 * z),
                    MUTED,
                );
            }
        }

        // Nodes on top.
        for (name, g, fill, folded, r) in galleys {
            painter.rect_filled(r, 7.0 * z, fill);
            if folded {
                painter.add(Shape::Vec(Shape::dashed_line(
                    &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()],
                    Stroke::new(1.5, MUTED),
                    5.0,
                    4.0,
                )));
            }
            if self.selected.as_deref() == Some(name.as_str()) {
                painter.rect_stroke(r, 7.0 * z, Stroke::new(3.0, ACCENT), StrokeKind::Outside);
            }
            painter.galley(r.center() - g.size() / 2.0, g, TEXT);
        }

        // Click: select what is under the pointer, or nothing.
        if resp.clicked()
            && let Some(p) = resp.interact_pointer_pos()
        {
            let hit = rects
                .iter()
                .filter(|(n, r)| r.contains(p) && self.at.contains_key(*n))
                .map(|(n, _)| n.clone())
                .next();
            self.via = None;
            self.select(hit);
        }
    }

    fn panel(&self, ui: &mut egui::Ui) {
        let Some(n) = self.selected.as_ref().and_then(|s| self.full.nodes.iter().find(|n| n.name == *s)) else {
            ui.heading(self.path.file_name().and_then(|f| f.to_str()).unwrap_or_default());
            ui.label(format!(
                "{} nodes, {} edges, {} groups",
                self.full.nodes.len(),
                self.full.edges.len(),
                self.full.groups.len()
            ));
            ui.label(format!("{} problems", self.full.problems.len()));
            ui.separator();
            ui.label(egui::RichText::new(format!("TAGS ({})", self.full.tags.len())).color(MUTED));
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (t, c) in &self.full.tags {
                    ui.label(format!("{t}  ({c})"));
                }
            });
            return;
        };
        ui.heading(&n.label);
        ui.label(egui::RichText::new("Node · read only in this prototype").color(MUTED));
        ui.separator();
        if !n.tags.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for t in &n.tags {
                    ui.label(egui::RichText::new(format!(" {t} ")).background_color(LINE));
                }
            });
            ui.separator();
        }
        egui::Grid::new("fields").num_columns(2).striped(false).show(ui, |ui| {
            for (k, v) in n.fields.iter().filter(|(k, _)| Some(k) != self.full.names.tag.as_ref()) {
                ui.label(egui::RichText::new(k).color(MUTED));
                ui.label(v);
                ui.end_row();
            }
        });
        ui.separator();
        ui.label(egui::RichText::new("EDGES").color(MUTED));
        for e in self.full.edges.iter().filter(|e| e.from == n.name || e.to == n.name) {
            ui.label(if e.from == n.name {
                format!("-> {}", e.to)
            } else {
                format!("<- {}", e.from)
            });
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Live reload: the file changed on disk.
        if self.changes.try_recv().is_ok() {
            while self.changes.try_recv().is_ok() {}
            self.load();
            self.msg = "reloaded: the file changed on disk".into();
        }

        let status = format!(
            "{}{} · {} nodes · {} edges · {} problems{}",
            if self.follow { "FOLLOW · " } else { "" },
            self.path.file_name().and_then(|f| f.to_str()).unwrap_or_default(),
            self.full.nodes.len(),
            self.full.edges.len(),
            self.full.problems.len(),
            if self.msg.is_empty() {
                String::new()
            } else {
                format!(" · {}", self.msg)
            }
        );
        egui::Panel::bottom("status").show(ui, |ui| {
            ui.label(egui::RichText::new(status).color(MUTED));
        });
        egui::Panel::right("panel").default_size(320.0).show(ui, |ui| self.panel(ui));
        egui::CentralPanel::no_frame().show(ui, |ui| self.canvas(ui));
    }
}

fn main() -> eframe::Result {
    let Some(file) = std::env::args().nth(1).filter(|a| a.ends_with(".rec")) else {
        eprintln!("usage: draw_graph_egui FILE.rec");
        std::process::exit(2);
    };
    let path = PathBuf::from(&file).canonicalize().unwrap_or_else(|e| {
        eprintln!("draw_graph_egui: cannot open {file}: {e}");
        std::process::exit(2);
    });
    let title = format!(
        "draw_graph (egui) - {}",
        path.file_name().and_then(|f| f.to_str()).unwrap_or_default()
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1400.0, 900.0]).with_title(&title),
        ..Default::default()
    };
    eframe::run_native("draw_graph_egui", options, Box::new(move |cc| Ok(Box::new(App::new(cc, path)))))
}
