//! Paint a scene: sizes from the labels, boxes around members, edges from
//! border to border. The same geometry, at zoom 1, writes the SVG export.
//!
//!   Scene ── geometry(measure, transform) ──> rects ──┬─ paint()  canvas
//!                                                     └─ svg()    FILE.svg

use std::collections::HashMap;
use std::fmt::Write as _;

use eframe::egui::{self, Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, Vec2};

use crate::scene::{Edge, Scene};

// Dark theme, as the Tauri app had it.
pub const BG: Color32 = Color32::from_rgb(0x0f, 0x11, 0x15);
pub const LINE: Color32 = Color32::from_rgb(0x2a, 0x2f, 0x3a);
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);
pub const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xb2);
pub const ACCENT: Color32 = Color32::from_rgb(0xe0, 0xb8, 0x4a);
pub const WARN: Color32 = Color32::from_rgb(0xe0, 0x7a, 0x4a);
pub const BAD: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x5a);

/// Label and note sizes and padding in graph units; zoom scales them.
pub const FONT: f32 = 12.0;
pub const NOTE: f32 = 10.0;
const NOTE_GAP: f32 = 2.0;
const PAD: Vec2 = Vec2::new(10.0, 6.0);
const DIAMOND_PAD: f32 = 1.6;
const BOX_PAD: f32 = 16.0;
const BOX_TITLE: f32 = 12.0;
const RADIUS: f32 = 7.0;

pub fn hex(s: &str) -> Color32 {
    let v = u32::from_str_radix(s.trim_start_matches('#'), 16).unwrap_or(0x2a3140);
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Where everything lands, in one coordinate space (screen, or SVG units).
#[derive(Default)]
pub struct Geometry {
    pub nodes: HashMap<String, Rect>,
    /// (id, label, rect, depth), outermost first.
    pub boxes: Vec<(String, String, Rect, usize)>,
    pub zoom: f32,
}

impl Geometry {
    /// The rect of a node or a box, by id.
    pub fn rect(&self, id: &str) -> Option<Rect> {
        self.nodes
            .get(id)
            .copied()
            .or_else(|| self.boxes.iter().find(|b| b.0 == id).map(|b| b.2))
    }

    /// The node under a point, else the innermost box whose title is there.
    pub fn hit(&self, p: Pos2) -> Option<String> {
        if let Some((id, _)) = self.nodes.iter().find(|(_, r)| r.contains(p)) {
            return Some(id.clone());
        }
        self.boxes.iter().rev().find(|b| b.2.contains(p)).map(|b| b.0.clone())
    }
}

/// The edge whose line passes within `tolerance` of a point, the closest
/// one; for clicks that land on no node.
pub fn hit_edge(scene: &Scene, g: &Geometry, p: Pos2, tolerance: f32) -> Option<String> {
    let near = |a: Pos2, b: Pos2| {
        let ab = b - a;
        let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
        (a + ab * t).distance(p)
    };
    scene
        .edges
        .iter()
        .filter_map(|e| {
            let pts = route(e, g.rect(&e.from)?, g.rect(&e.to)?);
            let d = pts.windows(2).map(|w| near(w[0], w[1])).fold(f32::MAX, f32::min);
            (d <= tolerance).then(|| (e.id.clone(), d))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(id, _)| id)
}

/// The text a measure takes: `(text, font size)` gives its size at zoom 1.
pub type Measure<'a> = dyn Fn(&str, f32) -> Vec2 + 'a;

/// Lay a scene out: `measure` sizes the label (and the note under it), `to`
/// maps graph units to the target space, `zoom` scales sizes.
pub fn geometry(scene: &Scene, measure: &Measure, to: &dyn Fn(Pos2) -> Pos2, zoom: f32) -> Geometry {
    let mut g = Geometry {
        zoom,
        ..Geometry::default()
    };
    for n in &scene.nodes {
        let label = measure(&n.label, FONT);
        let text = match &n.note {
            Some(note) => {
                let s = measure(note, NOTE);
                Vec2::new(label.x.max(s.x), label.y + NOTE_GAP + s.y)
            }
            None => label,
        };
        let mut size = text + PAD * 2.0;
        if n.shape == "diamond" {
            size *= DIAMOND_PAD;
        }
        g.nodes
            .insert(n.id.clone(), Rect::from_center_size(to(Pos2::new(n.x, n.y)), size * zoom));
    }

    // Boxes wrap their members and inner boxes, so inner ones go first.
    let depth = |id: &str| {
        let mut d = 0;
        let mut at = scene.boxes.iter().find(|b| b.id == id).and_then(|b| b.parent.clone());
        while let Some(p) = at {
            d += 1;
            if d > scene.boxes.len() {
                break; // a Parent loop; check reports it
            }
            at = scene.boxes.iter().find(|b| b.id == p).and_then(|b| b.parent.clone());
        }
        d
    };
    let mut order: Vec<_> = scene.boxes.iter().collect();
    order.sort_by_key(|b| std::cmp::Reverse(depth(&b.id)));
    let mut done: HashMap<String, Rect> = HashMap::new();
    for b in order {
        let inside: Vec<Rect> = b
            .members
            .iter()
            .filter_map(|m| g.nodes.get(m).copied())
            .chain(
                scene
                    .boxes
                    .iter()
                    .filter(|c| c.parent.as_deref() == Some(b.id.as_str()))
                    .filter_map(|c| done.get(&c.id).copied()),
            )
            .collect();
        let Some(first) = inside.first() else { continue };
        let mut r = inside.iter().fold(*first, |a, c| a.union(*c)).expand(BOX_PAD * zoom);
        r.min.y -= BOX_TITLE * zoom;
        done.insert(b.id.clone(), r);
        g.boxes.push((b.id.clone(), b.label.clone(), r, depth(&b.id)));
    }
    g.boxes.sort_by_key(|b| b.3);
    g
}

/// Where a line from a rect's centre toward a point leaves the rect.
fn clip(r: Rect, toward: Pos2) -> Pos2 {
    let d = toward - r.center();
    let t = (r.width() / 2.0 / d.x.abs().max(1e-3)).min(r.height() / 2.0 / d.y.abs().max(1e-3));
    r.center() + d * t.min(1.0)
}

/// The points of an edge: straight between borders, or an elbow for trees.
fn route(e: &Edge, a: Rect, b: Rect) -> Vec<Pos2> {
    if !e.elbow {
        return vec![clip(a, b.center()), clip(b, a.center())];
    }
    let d = b.center() - a.center();
    if d.x.abs() >= d.y.abs() {
        let (from, to) = if d.x >= 0.0 {
            (a.right_center(), b.left_center())
        } else {
            (a.left_center(), b.right_center())
        };
        let mx = (from.x + to.x) / 2.0;
        vec![from, Pos2::new(mx, from.y), Pos2::new(mx, to.y), to]
    } else {
        let (from, to) = if d.y >= 0.0 {
            (a.center_bottom(), b.center_top())
        } else {
            (a.center_top(), b.center_bottom())
        };
        let my = (from.y + to.y) / 2.0;
        vec![from, Pos2::new(from.x, my), Pos2::new(to.x, my), to]
    }
}

/// What to light up.
#[derive(Default)]
pub struct Marks<'a> {
    pub selected: Option<&'a str>,
    pub hits: &'a [String],
}

pub fn paint(painter: &Painter, scene: &Scene, g: &Geometry, marks: &Marks) {
    let z = g.zoom;
    for (_, label, r, _) in &g.boxes {
        painter.rect_filled(*r, 10.0 * z, Color32::from_rgba_unmultiplied(0x3a, 0x41, 0x50, 46));
        painter.rect_stroke(*r, 10.0 * z, Stroke::new(1.0, LINE), StrokeKind::Inside);
        painter.text(
            r.center_top() + Vec2::new(0.0, 4.0 * z),
            Align2::CENTER_TOP,
            label,
            FontId::proportional(FONT * z),
            MUTED,
        );
    }
    if let Some((_, _, r, _)) = marks.selected.and_then(|s| g.boxes.iter().find(|b| b.0 == s)) {
        painter.rect_stroke(*r, 10.0 * z, Stroke::new(2.5, ACCENT), StrokeKind::Outside);
    }

    for e in &scene.edges {
        let (Some(a), Some(b)) = (g.rect(&e.from), g.rect(&e.to)) else {
            continue;
        };
        let pts = route(e, a, b);
        // The selected edge is thickest; edges of the selected node are gold.
        let picked = marks.selected == Some(e.id.as_str());
        let on = picked || marks.selected.is_some_and(|s| s == e.from || s == e.to);
        let width = if picked {
            4.0
        } else if on {
            2.0
        } else {
            1.2
        };
        let stroke = Stroke::new(width, if on { ACCENT } else { MUTED });
        match e.style.as_str() {
            "dashed" => drop(painter.add(Shape::Vec(Shape::dashed_line(&pts, stroke, 6.0 * z, 5.0 * z)))),
            "dotted" => drop(painter.add(Shape::Vec(Shape::dashed_line(&pts, stroke, 1.5 * z, 4.0 * z)))),
            _ => drop(painter.add(Shape::line(pts.clone(), stroke))),
        }
        let (to, from) = (pts[pts.len() - 1], pts[pts.len() - 2]);
        if !e.elbow {
            let dir = (to - from).normalized();
            let side = Vec2::new(-dir.y, dir.x);
            let head = 8.0 * z.sqrt();
            painter.add(Shape::convex_polygon(
                vec![to, to - dir * head + side * head * 0.5, to - dir * head - side * head * 0.5],
                stroke.color,
                Stroke::NONE,
            ));
        }
        let mid = pts[0] + (pts[pts.len() - 1] - pts[0]) / 2.0;
        if let Some(l) = &e.label {
            painter.text(mid, Align2::CENTER_CENTER, l, FontId::proportional(11.0 * z), MUTED);
        }
        if let Some(note) = &e.note {
            let below = if e.label.is_some() { 12.0 * z } else { 0.0 };
            painter.text(
                mid + Vec2::new(0.0, below),
                Align2::CENTER_CENTER,
                note,
                FontId::proportional(NOTE * z),
                MUTED,
            );
        }
    }

    for n in &scene.nodes {
        let Some(r) = g.nodes.get(&n.id).copied() else { continue };
        let fill = hex(&n.fill);
        match n.shape.as_str() {
            "diamond" => drop(painter.add(Shape::convex_polygon(
                vec![r.center_top(), r.right_center(), r.center_bottom(), r.left_center()],
                fill,
                Stroke::NONE,
            ))),
            "ellipse" => drop(painter.rect_filled(r, r.height() / 2.0, fill)),
            "box" => drop(painter.rect_filled(r, 0.0, fill)),
            _ => drop(painter.rect_filled(r, RADIUS * z, fill)),
        }
        if n.dashed {
            let c = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            painter.add(Shape::Vec(Shape::dashed_line(&c, Stroke::new(1.5, MUTED), 5.0, 4.0)));
        }
        if marks.hits.contains(&n.id) {
            painter.rect_stroke(r, RADIUS * z, Stroke::new(2.0, WARN), StrokeKind::Outside);
        }
        // The selected node, or both ends of the selected edge.
        let end = marks
            .selected
            .is_some_and(|s| scene.edges.iter().any(|e| e.id == s && (e.from == n.id || e.to == n.id)));
        if marks.selected == Some(n.id.as_str()) || end {
            painter.rect_stroke(r, RADIUS * z, Stroke::new(3.0, ACCENT), StrokeKind::Outside);
        }
        match &n.note {
            Some(note) => {
                // Label on top, the note under it, the pair centred.
                let lift = (NOTE + NOTE_GAP) * 0.6 * z;
                painter.text(
                    r.center() - Vec2::new(0.0, lift),
                    Align2::CENTER_CENTER,
                    &n.label,
                    FontId::proportional(FONT * z),
                    TEXT,
                );
                painter.text(
                    r.center() + Vec2::new(0.0, (FONT + NOTE_GAP) * 0.6 * z),
                    Align2::CENTER_CENTER,
                    note,
                    FontId::proportional(NOTE * z),
                    MUTED,
                );
            }
            None => drop(painter.text(r.center(), Align2::CENTER_CENTER, &n.label, FontId::proportional(FONT * z), TEXT)),
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn rgb(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

/// The scene as a standalone dark SVG, in graph units.
pub fn svg(ctx: &egui::Context, scene: &Scene) -> String {
    let measure = |s: &str, size: f32| ctx.fonts_mut(|f| f.layout_no_wrap(s.to_string(), FontId::proportional(size), TEXT).size());
    let g = geometry(scene, &measure, &|p| p, 1.0);
    let all = g
        .nodes
        .values()
        .chain(g.boxes.iter().map(|b| &b.2))
        .copied()
        .reduce(|a, b| a.union(b))
        .unwrap_or(Rect::ZERO)
        .expand(40.0);
    let (x, y, w, h) = (all.min.x, all.min.y, all.width(), all.height());
    let mut out = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w:.0}\" height=\"{h:.0}\" viewBox=\"{x:.1} {y:.1} {w:.1} {h:.1}\" font-family=\"-apple-system, Helvetica, Arial, sans-serif\">\n<rect x=\"{x:.1}\" y=\"{y:.1}\" width=\"{w:.1}\" height=\"{h:.1}\" fill=\"{}\"/>\n<defs><marker id=\"arrow\" viewBox=\"0 0 10 10\" refX=\"9\" refY=\"5\" markerWidth=\"7\" markerHeight=\"7\" orient=\"auto-start-reverse\"><path d=\"M0,0 L10,5 L0,10 z\" fill=\"{}\"/></marker></defs>\n",
        rgb(BG),
        rgb(MUTED)
    );
    for (_, label, r, _) in &g.boxes {
        let _ = writeln!(
            out,
            "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"10\" fill=\"#3a4150\" fill-opacity=\"0.18\" stroke=\"{}\"/>",
            r.min.x,
            r.min.y,
            r.width(),
            r.height(),
            rgb(LINE)
        );
        let _ = writeln!(
            out,
            "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{}\">{}</text>",
            r.center().x,
            r.min.y + 15.0,
            rgb(MUTED),
            esc(label)
        );
    }
    for e in &scene.edges {
        let (Some(a), Some(b)) = (g.rect(&e.from), g.rect(&e.to)) else {
            continue;
        };
        let pts = route(e, a, b);
        let d: Vec<String> = pts
            .iter()
            .enumerate()
            .map(|(i, p)| format!("{}{:.1},{:.1}", if i == 0 { "M" } else { "L" }, p.x, p.y))
            .collect();
        let dash = match e.style.as_str() {
            "dashed" => " stroke-dasharray=\"6 5\"",
            "dotted" => " stroke-dasharray=\"1.5 4\" stroke-linecap=\"round\"",
            _ => "",
        };
        let arrow = if e.elbow { "" } else { " marker-end=\"url(#arrow)\"" };
        let _ = writeln!(
            out,
            "<path d=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"{dash}{arrow}/>",
            d.join(" "),
            rgb(MUTED)
        );
        let mid = pts[0] + (pts[pts.len() - 1] - pts[0]) / 2.0;
        if let Some(note) = &e.note {
            let below = if e.label.is_some() { 14.0 } else { 4.0 };
            let _ = writeln!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"10\" fill=\"{}\">{}</text>",
                mid.x,
                mid.y + below,
                rgb(MUTED),
                esc(note)
            );
        }
        if let Some(l) = &e.label {
            let _ = writeln!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"11\" fill=\"{}\">{}</text>",
                mid.x,
                mid.y + 4.0,
                rgb(MUTED),
                esc(l)
            );
        }
    }
    for n in &scene.nodes {
        let Some(r) = g.nodes.get(&n.id) else { continue };
        let dash = if n.dashed {
            format!(" stroke=\"{}\" stroke-dasharray=\"5 4\"", rgb(MUTED))
        } else {
            String::new()
        };
        let shape = match n.shape.as_str() {
            "diamond" => format!(
                "<polygon points=\"{:.1},{:.1} {:.1},{:.1} {:.1},{:.1} {:.1},{:.1}\" fill=\"{}\"{dash}/>",
                r.center().x,
                r.min.y,
                r.max.x,
                r.center().y,
                r.center().x,
                r.max.y,
                r.min.x,
                r.center().y,
                n.fill
            ),
            "ellipse" => format!(
                "<ellipse cx=\"{:.1}\" cy=\"{:.1}\" rx=\"{:.1}\" ry=\"{:.1}\" fill=\"{}\"{dash}/>",
                r.center().x,
                r.center().y,
                r.width() / 2.0,
                r.height() / 2.0,
                n.fill
            ),
            s => format!(
                "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{:.1}\" height=\"{:.1}\" rx=\"{}\" fill=\"{}\"{dash}/>",
                r.min.x,
                r.min.y,
                r.width(),
                r.height(),
                if s == "box" { 0.0 } else { RADIUS },
                n.fill
            ),
        };
        // Label, and the note under it, as the canvas draws them.
        let (label_y, note_y) = match n.note {
            Some(_) => (
                r.center().y + 4.0 - (NOTE + NOTE_GAP) * 0.6,
                r.center().y + 3.5 + (FONT + NOTE_GAP) * 0.6,
            ),
            None => (r.center().y + 4.0, 0.0),
        };
        let _ = writeln!(
            out,
            "{shape}\n<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"12\" fill=\"{}\">{}</text>",
            r.center().x,
            label_y,
            rgb(TEXT),
            esc(&n.label)
        );
        if let Some(note) = &n.note {
            let _ = writeln!(
                out,
                "<text x=\"{:.1}\" y=\"{:.1}\" text-anchor=\"middle\" font-size=\"10\" fill=\"{}\">{}</text>",
                r.center().x,
                note_y,
                rgb(MUTED),
                esc(note)
            );
        }
    }
    out.push_str("</svg>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{Edge, Node, Scene};

    fn node(id: &str, x: f32) -> Node {
        Node {
            id: id.into(),
            name: id.into(),
            label: id.into(),
            fill: "#123456".into(),
            shape: "round".into(),
            x,
            y: 0.0,
            dashed: false,
            note: None,
        }
    }

    #[test]
    fn a_click_near_an_edge_picks_it() {
        // a ──> b, 300 units apart; 10x10 labels at zoom 1.
        let scene = Scene {
            nodes: vec![node("n:a", 0.0), node("n:b", 300.0)],
            boxes: Vec::new(),
            edges: vec![Edge {
                id: "e:0".into(),
                from: "n:a".into(),
                to: "n:b".into(),
                style: "solid".into(),
                label: None,
                elbow: false,
                note: None,
            }],
        };
        let g = geometry(&scene, &|_, _| Vec2::splat(10.0), &|p| p, 1.0);
        assert_eq!(hit_edge(&scene, &g, Pos2::new(150.0, 4.0), 6.0).as_deref(), Some("e:0"));
        assert_eq!(hit_edge(&scene, &g, Pos2::new(150.0, 20.0), 6.0), None, "too far from the line");
        assert_eq!(g.hit(Pos2::new(0.0, 0.0)).as_deref(), Some("n:a"), "a node under the click wins");
    }
}
