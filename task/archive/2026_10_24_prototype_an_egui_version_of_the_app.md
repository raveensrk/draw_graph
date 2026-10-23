---
id: "kecc5m52tt"
title: "Prototype an egui version of the app"
state: "done"
due: ""
priority: ""
tag: ["feature", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: "2026-10-24T01:24"
---

Try egui (eframe) as the UI instead of Tauri + Leptos + Cytoscape.js. Suggested on 2026-10-24: with only the manual and tree layouts kept, ELK is not needed, and every rule already lives in the pure-Rust `core` crate.

## Plan

- A new workspace crate `egui/`, binary `draw_graph_egui FILE.rec`, using eframe 0.35 (the newest that builds on rustc 1.94).
- Reuse `core` as is: `model::model`, `model::folded`, `model::branches`, `edit::apply`, `check`.
- Canvas: nodes at their saved `Pos`, edges with arrowheads, group boxes, colours from the model; pan by dragging, zoom with scroll or pinch; click to select.
- Keys: arrows and h j k l move by position (port `nearest` from `ui/canvas.js`), Shift + arrows walk the edges (port `walk`), `za` folds a branch, `F` follow mode.
- Side panel: the selected node's fields and tags, read only in the prototype.
- Live reload with `notify`, as `app/src/gui.rs` does.

## Acceptance

- `draw_graph_egui ~/repos/agent1/dev_tools.rec` opens the tool list, draws all 77 tools and 80 edges at their positions, and moves with arrows and Shift + arrows.
- A written comparison with the Tauri app: binary size, start time, feel, what is missing; and a decision to switch or not.


## Findings, 2026-10-24

The prototype is `egui/` (crate `draw_graph_egui`): one file, 571 lines, read only.

Works, checked in the real window on a copy of `dev_tools.rec` (78 tools, 81 edges):

- Draws every tool at its saved `Pos`, group boxes, arrowheads, dashed and dotted edges, first-tag colours.
- Pan by drag or scroll, zoom by pinch or Cmd + scroll, click to select.
- Arrows and h j k l by position, Shift + arrows along the edges, `za` folds a branch ("Ghostty +70"), `zR`, `F` follow with a glide, `zz`, `f` fit, Esc.
- Side panel: fields, tags, edges; the tag list with counts.
- Live reload: an outside edit shows at once and keeps the selection and folds.
- Built and ran first time; no web layer, no JavaScript, no Trunk.

Measured (release builds, same file open):

| | egui | Tauri + Leptos + Cytoscape |
|---|---|---|
| Binary | 13.9 MB | 11.3 MB |
| Main process memory | 111 MB | 112 MB, plus WebKit helper processes |
| UI code | 571 lines, Rust | 2,114 lines: Rust (wasm), JavaScript, CSS, HTML |
| Build tools | cargo | cargo, Trunk, wasm target, tauri-cli |

Missing in the prototype: editing (form, tag chips, add, connect, delete, undo), tree view, saved views, the tag filter, search, SVG export, `.app` bundle. egui's built-in fonts lack ▸ and arrows, so it shows `+70` and `->`; a bundled font would fix that.

Decision: the user's.

## Log

- done 2026-10-24 01:24: egui/ prototype (7eb708e): draws, moves, folds, follows, reloads; measured against Tauri; user chose to switch
