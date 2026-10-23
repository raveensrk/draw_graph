---
id: "e4p3zrv42g"
title: "Sign and notarize the macOS builds"
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

Unsigned builds make macOS ask on first launch (System Settings > Privacy & Security). Needs an Apple Developer ID (99 USD a year).

## Plan

- Developer ID certificate and notarization credentials as GitHub secrets.
- tauri-action signing env vars in `release.yml`.

## Acceptance

- A downloaded `.dmg` opens with no Gatekeeper warning.
