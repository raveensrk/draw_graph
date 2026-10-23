---
id: "2f1d6c2njh"
title: "Show the app icon when draw_graph runs"
state: "todo"
due: ""
priority: ""
tag: ["release", "draw_graph"]
repeat: ""
effort: ""
postpone: 0
created: "2026-10-24"
closed: ""
---

The icon exists (`app/icons/icon.svg`, every size rendered from it) but does not show when the app runs from `~/.local/bin/draw_graph`: a plain binary has no bundle, so the Dock and the app switcher show a generic icon. Only the `.app` from the release `.dmg` carries it.

## Plan

- Launch as `draw_graph.app` (see the automatic installation task), or set the Dock icon at runtime from the bundled `icon.png`.
- Check the icon at Dock size and in Finder, Cmd-Tab and the window's title.
- Revisit the design if it reads poorly small; the source is `app/icons/icon.svg`, regenerate with `cargo tauri icon`.

## Acceptance

- Running draw_graph shows its own icon in the Dock and in Cmd-Tab.
