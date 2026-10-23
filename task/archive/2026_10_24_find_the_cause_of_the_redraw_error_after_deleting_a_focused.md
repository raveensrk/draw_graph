---
id: "6dmjqcswer"
title: "Find the cause of the redraw error after deleting a focused box"
state: "obsolete"
due: ""
priority: ""
tag: ["bug", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: "2026-10-24T01:42"
---

Once, `cy.elements().remove()` threw `e._private.map.get(t).index` inside the on-screen renderer after `dd` on a just-created, focused group. Headless Cytoscape did not reproduce it. The safety net in `ui/canvas.js` `swap()` removes whole top-level elements and rebuilds the canvas on failure, logging `ui_error`.

## Acceptance

- The cause is understood and fixed, or a minimal repro is reported upstream; the log shows no `swap failed` after G then dd.

## Log

- obsolete 2026-10-24 01:42: Cytoscape.js was removed in the egui switch (ee0958f); this redraw path no longer exists
