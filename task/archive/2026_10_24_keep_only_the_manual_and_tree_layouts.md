---
id: "dj330bjzpz"
title: "Keep only the manual and tree layouts"
state: "done"
due: ""
priority: ""
tag: ["feature", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: "2026-10-24T01:42"
---

Keep two graph layouts, `manual` and `tree`; remove `layered`, `radial` and `force`. Asked for on 2026-10-24. The tree view (`t`, `T`) is separate and stays.

## Plan

- `ui/src/main.rs`: `LAYOUTS` becomes `["manual", "tree"]`, so `L` toggles between them; the help line for `L` says so.
- `ui/canvas.js`: drop `layered`, `radial` and `force` from `LAYOUTS`.
- A file or a node with no `Pos` is placed today by `layered` (the open default, and `manual`'s fallback in `render`). Place it with `tree` instead.
- Views: `View` records name a `Layout`. Update the `%type: Layout enum` and the views in `example/draw_graph_example.rec` (Learning path and Save flow use `layered`, Mind map `radial`); `V` falls back to `manual` for an unknown layout.
- PLAN.md: the Layout row and the Keys section.
- The `tree` layout's overlapping boxes are task 6axn; do that one with this one.

## Acceptance

- `L` steps only between manual and tree; a file without positions opens in tree.
- `draw_graph check example/draw_graph_example.rec` reports 0 problems; core tests pass.

## Log

- done 2026-10-24 01:42: egui app (ee0958f): only manual and tree layouts exist; L toggles them
