---
id: "qty2kfexea"
title: "Small cleanups: width label, Trunk pin, PLAN status line"
state: "done"
due: ""
priority: ""
tag: ["maintenance", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: "2026-10-24T01:42"
---

- Cytoscape 3.30 warns that `width: label` / `height: label` are deprecated (`ui/canvas.js` style).
- Local Trunk is held at 0.21 (needs rustc 1.96.1 for 0.22); move up after updating Rust.
- PLAN.md's status line still says steps 1 to 4; all six are done.

## Acceptance

- No deprecation warning; Trunk matches CI; PLAN.md status is current.

## Log

- done 2026-10-24 01:42: Cytoscape width label and Trunk are gone with the egui switch; PLAN.md status updated
