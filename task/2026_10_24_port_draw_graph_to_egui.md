---
id: "zwwxj9d4kb"
title: "Port draw_graph to egui"
state: "in_progress"
due: ""
priority: ""
tag: ["feature", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: ""
started: "2026-10-24T01:24"
---

Port draw_graph to egui and retire the Tauri + Leptos + Cytoscape app. Decided on 2026-10-24 after the prototype (task "Prototype an egui version of the app").

## Plan

1. Editing: the side panel form from the schema (dropdowns for enums and ranges, text, note), tag chips, rename by the Name field; o O a c G prompts, dd, drag to move, s; every edit through `core::edit::apply`, written atomically, with undo and redo (as `app/src/gui.rs` does).
2. Tree view (t, T, `;`, za, zR, zM, gd) and saved views (V), from `core::tree`; layouts manual and tree only (task "Keep only the manual and tree layouts").
3. Group boxes (za, G, dd), the tag filter, search (/ n N, smartcase), e, help page (?), SVG export (E) written in Rust, a font with ▸ and arrows.
4. The swap: the egui crate becomes the `draw_graph` binary with `check` and the help table from `app/src/main.rs`; remove `app/` and `ui/`; release builds a `.app` and `.dmg` without tauri-action; update ci.yml, PLAN.md, AGENTS.md, README.md.

## Acceptance

- `draw_graph ~/repos/agent1/dev_tools.rec` opens the egui app with every feature the Tauri app had.
- `draw_graph check` unchanged; core tests pass; CI green; a v* tag builds the Mac `.dmg`.
