---
id: "9zx0kah3mw"
title: "Apply a saved view's Filter expression"
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

`V` applies a View's layout, root and edge kinds, and says when it skips the `Filter` line (e.g. `Filter: Kind = 'tool'`).

## Plan

- Decide the syntax: recsel-style `Field = 'value'`, `and`, `or`; parse and evaluate in core.
- Hide nodes that fail it in the graph view, as the tag filter does.

## Acceptance

- The example's Learning path and Save flow views show only their nodes; core tests cover the expressions.
