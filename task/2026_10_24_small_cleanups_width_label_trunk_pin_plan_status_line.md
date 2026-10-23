---
id: "qty2kfexea"
title: "Small cleanups: width label, Trunk pin, PLAN status line"
state: "todo"
due: ""
priority: ""
tag: ["maintenance", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: ""
---

- Cytoscape 3.30 warns that `width: label` / `height: label` are deprecated (`ui/canvas.js` style).
- Local Trunk is held at 0.21 (needs rustc 1.96.1 for 0.22); move up after updating Rust.
- PLAN.md's status line still says steps 1 to 4; all six are done.

## Acceptance

- No deprecation warning; Trunk matches CI; PLAN.md status is current.
