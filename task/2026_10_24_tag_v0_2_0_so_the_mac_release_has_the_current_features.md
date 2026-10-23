---
id: "3sacz6yjmd"
title: "Tag v0.2.0 so the Mac release has the current features"
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

The draft release v0.1.0 was built before tags, branch folds, arrow keys, follow mode and Shift + arrows.

## Plan

- Bump `version` in `Cargo.toml` (workspace) and `app/tauri.conf.json` to 0.2.0.
- `git tag -a v0.2.0` and push the tag; `release.yml` builds the draft.
- Delete the stale v0.1.0 draft.

## Acceptance

- A v0.2.0 draft with `aarch64` and `x64` `.dmg` files, built from the tagged commit.
