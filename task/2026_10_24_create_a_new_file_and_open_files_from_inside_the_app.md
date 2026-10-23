---
id: "vqnd2m4qrq"
title: "Create a new file and open files from inside the app"
state: "todo"
due: ""
priority: ""
tag: ["feature", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: ""
---

Today the app only opens an existing `.rec` given on the command line.

## Plan

- `draw_graph new FILE.rec` writes a minimal Node/Edge/Group schema with %graph lines.
- File > Open dialog and recent files in the window.

## Acceptance

- A new file opens, takes `a` to add a node, and passes `draw_graph check`.
