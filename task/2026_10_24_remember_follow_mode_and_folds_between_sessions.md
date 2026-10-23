---
id: "ymfqgfst3t"
title: "Remember follow mode and folds between sessions"
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

Follow mode, branch folds and quick-tree folds reset when the window closes; only saved-view and box folds live in the file.

## Plan

- Keep them per file outside the data file (app config dir or web view storage), so git stays quiet.

## Acceptance

- Close and reopen: follow mode and folded branches are as they were.
