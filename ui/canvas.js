// The canvas: Cytoscape.js draws, ELK.js lays out. Leptos owns everything
// else (keys, the side panel, state) and calls in through window.dgCanvas.
//
//   Leptos ── render(model, layout) ──> canvas.js ──> Cytoscape + ELK
//          <── onTap / onDrag ─────────
//
// Nothing here knows the rules or the file; it draws what it is given.

cytoscape.use(cytoscapeElk);

// Errors and renders go to the app's log (stderr, one JSON line each), so a
// failure in here is visible without opening the web inspector.
function report(event, fields) {
  try {
    window.__TAURI__.core.invoke("ui_log", { line: JSON.stringify({ event, ...fields }) });
  } catch (e) {
    console.error(e);
  }
}
window.addEventListener("error", (e) => report("ui_error", { message: String(e.message), at: `${e.filename}:${e.lineno}` }));
window.addEventListener("unhandledrejection", (e) => report("ui_error", { message: String(e.reason && (e.reason.stack || e.reason)) }));

// Fitting a small graph would blow it up; never past this zoom.
const FIT_MAX_ZOOM = 1.2;

// Drag events closer together than this are one drag.
const DRAG_SETTLE_MS = 60;

const SHAPES = { box: "rectangle", round: "round-rectangle", diamond: "diamond", ellipse: "ellipse" };

// ELK settings per layout name; manual uses the saved Pos.
const LAYOUTS = {
  layered: { "elk.algorithm": "layered", "elk.direction": "RIGHT", "elk.layered.spacing.nodeNodeBetweenLayers": 60 },
  tree: { "elk.algorithm": "mrtree", "elk.direction": "DOWN" },
  radial: { "elk.algorithm": "radial" },
  force: { "elk.algorithm": "force" },
};

let cy = null;
let handlers = {};

function style() {
  const css = getComputedStyle(document.documentElement);
  const v = (name) => css.getPropertyValue(name).trim();
  return [
    { selector: "node", style: {
      "background-color": "data(color)", shape: "data(shape)", label: "data(label)",
      color: v("--text"), "font-size": 12, "text-valign": "center", "text-halign": "center",
      "text-wrap": "wrap", "text-max-width": 140, width: "label", height: "label",
      padding: 10, "border-width": 0,
    } },
    { selector: ":parent", style: {
      "background-color": "data(color)", "background-opacity": 0.18, shape: "round-rectangle",
      "text-valign": "top", "text-halign": "center", "font-size": 13, "font-weight": 600,
      color: v("--muted"), "border-width": 1, "border-color": v("--line"), padding: 18,
    } },
    { selector: "edge", style: {
      width: 1.5, "line-color": v("--muted"), "target-arrow-color": v("--muted"),
      "target-arrow-shape": "triangle", "curve-style": "bezier", "line-style": "data(style)",
      label: "data(label)", "font-size": 11, color: v("--muted"),
      "text-background-color": v("--bg"), "text-background-opacity": 1, "text-background-padding": 2,
    } },
    { selector: 'edge[id ^= "te:"]', style: { "curve-style": "taxi", "taxi-direction": "auto", "target-arrow-shape": "none" } },
    { selector: "node.focus", style: { "border-width": 3, "border-color": v("--accent") } },
    { selector: "edge.focus", style: { "line-color": v("--accent"), "target-arrow-color": v("--accent"), width: 3 } },
    { selector: ".match", style: { "border-width": 2, "border-color": v("--warn") } },
    { selector: ".dim", style: { opacity: 0.25 } },
    { selector: ".repeat", style: { "border-width": 1.5, "border-style": "dashed", "border-color": v("--muted"), "background-opacity": 0.55 } },
    { selector: ".looped", style: { "border-width": 1.5, "border-style": "dotted", "border-color": v("--warn"), "background-opacity": 0.55 } },
  ];
}

function elements(model) {
  const out = [];
  for (const g of model.groups) {
    out.push({ group: "nodes", data: { id: "g:" + g.name, name: g.name, kind: "group", label: g.label, color: g.color || "#3a4150", parent: g.parent ? "g:" + g.parent : undefined } });
  }
  for (const n of model.nodes) {
    out.push({
      group: "nodes",
      data: { id: "n:" + n.name, name: n.name, kind: "node", label: n.label, color: n.color, shape: SHAPES[n.shape] || "round-rectangle", parent: n.group ? "g:" + n.group : undefined },
      position: n.pos ? { x: n.pos[0], y: n.pos[1] } : undefined,
    });
  }
  const known = new Set(out.map((e) => e.data.id));
  const end = (name) => (known.has("n:" + name) ? "n:" + name : "g:" + name);
  for (const e of model.edges) {
    out.push({ group: "edges", data: { id: "e:" + e.index, index: e.index, kind: "edge", source: end(e.from), target: end(e.to), label: e.label || "", style: e.style } });
  }
  return out;
}

function run(layout, done) {
  const opts = LAYOUTS[layout];
  if (!opts) {
    done && done();
    return;
  }
  try {
    const l = cy.layout({ name: "elk", nodeDimensionsIncludeLabels: true, fit: false, elk: { ...opts, "elk.hierarchyHandling": "INCLUDE_CHILDREN" } });
    l.one("layoutstop", () => {
      report("layout", { layout, nodes: cy.nodes().length });
      done && done();
    });
    l.run();
  } catch (e) {
    report("ui_error", { message: `layout ${layout}: ${e.stack || e}` });
    done && done();
  }
}

window.dgCanvas = {
  // Mount once. handlers: onTap(id), onBackground(), onDrag(positions).
  init(container, h) {
    handlers = h;
    cy = cytoscape({ container, style: style(), wheelSensitivity: 0.3, boxSelectionEnabled: false });
    cy.on("tap", "node, edge", (ev) => handlers.onTap && handlers.onTap(ev.target.id()));
    cy.on("tap", (ev) => ev.target === cy && handlers.onBackground && handlers.onBackground());
    // Dragging a group fires once per member; report once, when they stop.
    let settle = null;
    cy.on("dragfree", "node", () => {
      clearTimeout(settle);
      settle = setTimeout(() => handlers.onDrag && handlers.onDrag(JSON.stringify(window.dgCanvas.positions())), DRAG_SETTLE_MS);
    });
  },

  // Draw a model (JSON). Same layout as last time keeps the viewport, so an
  // edit does not jump the view; a new layout fits the graph.
  render(modelJson, layout, refit) {
    const model = JSON.parse(modelJson);
    const keep = { zoom: cy.zoom(), pan: { ...cy.pan() } };
    const saved = new Map(cy.nodes().map((n) => [n.id(), { ...n.position() }]));
    cy.elements().remove();
    cy.add(elements(model));
    report("render", { layout, refit, nodes: model.nodes.length, edges: model.edges.length, drawn: cy.elements().length });

    // Manual: saved Pos, and ELK places only what has none.
    const unplaced = cy.nodes('[kind = "node"]').filter((n) => !model.nodes.find((m) => "n:" + m.name === n.id()).pos);
    const after = () => {
      if (refit) fit();
      else cy.viewport(keep);
    };
    if (layout !== "manual") {
      // Unchanged layout: keep where things were, lay out only newcomers.
      if (!refit && saved.size) {
        cy.nodes().forEach((n) => saved.has(n.id()) && n.position(saved.get(n.id())));
        const fresh = cy.nodes('[kind = "node"]').filter((n) => !saved.has(n.id()));
        fresh.forEach((n) => n.position(neighbourSpot(n)));
        after();
        return;
      }
      run(layout, after);
      return;
    }
    if (unplaced.length === cy.nodes('[kind = "node"]').length) run("layered", after);
    else {
      unplaced.forEach((n) => n.position(neighbourSpot(n)));
      after();
    }
  },

  // Draw a tree (JSON items placed by Rust): copies get their own ids
  // (t:0.2.1), so one node can show under several parents.
  //   label ▸ 3   folded, 3 children hidden
  //   label ↑     a repeat: the full branch is at its first copy (gd)
  //   label ↺     closes a loop back to a node above it
  renderTree(itemsJson, refit) {
    const items = JSON.parse(itemsJson);
    const keep = { zoom: cy.zoom(), pan: { ...cy.pan() } };
    cy.elements().remove();
    const els = [];
    for (const i of items) {
      const mark = (i.looped ? " ↺" : i.repeat ? " ↑" : "") + (i.hidden && !i.looped && !i.repeat ? ` ▸ ${i.hidden}` : "");
      els.push({
        group: "nodes",
        data: { id: i.id, name: i.name, kind: "node", label: i.label + mark, color: i.color, shape: SHAPES[i.shape] || "round-rectangle" },
        position: { x: i.x, y: i.y },
        classes: (i.repeat ? "repeat " : "") + (i.looped ? "looped" : ""),
      });
      if (i.parent) {
        const [index, style, label] = i.edge || [null, "solid", null];
        els.push({ group: "edges", data: { id: "te:" + i.id, index, kind: "edge", source: i.parent, target: i.id, label: label || "", style } });
      }
    }
    cy.add(els);
    cy.nodes().ungrabify(); // a tree's places come from its shape, not drags
    report("render", { layout: "tree", refit, nodes: items.length });
    if (refit) fit();
    else cy.viewport(keep);
  },

  // [[name, x, y], ...] for every node (not groups), rounded.
  positions() {
    return cy.nodes('[kind = "node"]').map((n) => [n.data("name"), Math.round(n.position("x")), Math.round(n.position("y"))]);
  },

  // The nearest node in a direction (h j k l), or null.
  nearest(id, dir) {
    const from = cy.getElementById(id);
    const nodes = cy.nodes('[kind = "node"]');
    if (from.empty()) return nodes.length ? nodes[0].id() : null;
    const p = from.isEdge() ? from.midpoint() : from.position();
    const axis = { h: [-1, 0], l: [1, 0], k: [0, -1], j: [0, 1] }[dir];
    let best = null;
    let score = Infinity;
    nodes.forEach((n) => {
      if (n.id() === id) return;
      const q = n.position();
      const ahead = (q.x - p.x) * axis[0] + (q.y - p.y) * axis[1];
      const side = Math.abs((q.x - p.x) * axis[1] + (q.y - p.y) * axis[0]);
      if (ahead <= 0 || side > ahead * 2) return; // behind, or too far off the line
      const s = ahead + side * 2;
      if (s < score) {
        score = s;
        best = n.id();
      }
    });
    return best;
  },

  // Highlight one element; pan to it when it is off screen.
  focus(id) {
    cy.elements().removeClass("focus");
    if (!id) return;
    const el = cy.getElementById(id);
    if (el.empty()) return;
    el.addClass("focus");
    const bb = el.renderedBoundingBox();
    const w = cy.width();
    const h = cy.height();
    if (bb.x1 < 0 || bb.y1 < 0 || bb.x2 > w || bb.y2 > h) cy.animate({ center: { eles: el }, duration: 150 });
  },

  // Mark search matches; returns their ids in reading order.
  search(query) {
    cy.elements().removeClass("match");
    if (!query) return [];
    const smart = query === query.toLowerCase(); // smartcase, as in vim
    const has = (s) => (smart ? s.toLowerCase() : s).includes(query);
    const hits = cy.nodes('[kind = "node"]').filter((n) => has(n.data("label")) || has(n.data("name")));
    hits.addClass("match");
    return hits.sort((a, b) => a.position("y") - b.position("y") || a.position("x") - b.position("x")).map((n) => n.id());
  },

  fit() {
    fit();
  },

  restyle() {
    cy.style(style());
  },
};

function fit() {
  cy.fit(undefined, 40);
  if (cy.zoom() > FIT_MAX_ZOOM) {
    cy.zoom(FIT_MAX_ZOOM);
    cy.center();
  }
}

// Where a new node goes: right of a neighbour, else the middle of the view.
function neighbourSpot(n) {
  const near = n.neighborhood('node[kind = "node"]').filter((m) => m.id() !== n.id());
  const placed = near.filter((m) => m.position("x") !== 0 || m.position("y") !== 0);
  if (placed.length) {
    const p = placed[0].position();
    return { x: p.x + 200, y: p.y + 40 * (Math.random() - 0.5) };
  }
  const ext = cy.extent();
  return { x: (ext.x1 + ext.x2) / 2, y: (ext.y1 + ext.y2) / 2 };
}
