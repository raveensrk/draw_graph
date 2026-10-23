---
id: "q92dgrd1j6"
title: "Add automated tests for the window"
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

Only core has tests (59). Keys, panels and the canvas were checked by hand in the real window.

## Plan

- wasm-bindgen-test or a browser driver over `trunk serve` with a mocked Tauri `invoke`.

## Acceptance

- CI runs UI tests for moving, adding, deleting, folding and tags.


## 2026-10-24

UI tests are easier now: egui has egui_kittest, which drives a window in tests (keys, clicks, snapshots) with no browser.
