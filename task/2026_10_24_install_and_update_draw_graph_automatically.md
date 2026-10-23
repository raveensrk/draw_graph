---
id: "fq2t17ydwj"
title: "Install and update draw_graph automatically"
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

Installing today is by hand: `cd app && cargo tauri build --no-bundle && cp ../target/release/draw_graph ~/.local/bin/`, after every change. The release `.dmg` has to be downloaded and dragged to Applications.

## Plan

- One command installs or updates both: `draw_graph.app` in `/Applications` (Dock, Spotlight, icon) and the `draw_graph` command on PATH, which `agent1`'s harness check needs.
- Decide the route: a `script/install.sh` in the repo that builds and copies, a Homebrew tap (`brew install --cask raveensrk/tap/draw_graph`) fed by the release workflow, or the Tauri updater so the app updates itself from GitHub releases.
- Keep `~/.local/bin/draw_graph` pointing at the same build as the `.app`, so the two never drift.

## Acceptance

- On a clean Mac, one command gives a working `draw_graph` command and a `draw_graph.app` with its icon.
- Re-running it after a new release updates both.


## 2026-10-24

Half done: `script/bundle_macos.sh --install` builds and installs both the .app (~/Applications) and the command (~/.local/bin) in one command. Still open: updating from a release without a Rust toolchain (Homebrew tap or a self-updater).
