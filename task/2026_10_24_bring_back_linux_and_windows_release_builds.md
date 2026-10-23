---
id: "4y7zp9y2jb"
title: "Bring back Linux and Windows release builds"
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

Dropped on 2026-10-24 by choice. The comment in `.github/workflows/release.yml` gives the two matrix rows; the Linux apt step is still there.

## Acceptance

- A tag builds `.AppImage`/`.deb` on ubuntu-22.04 and `.msi`/`.exe` on windows-latest.
