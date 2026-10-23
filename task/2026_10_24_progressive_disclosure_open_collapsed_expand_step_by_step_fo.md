---
id: "y7aqbgxhqh"
title: "Progressive disclosure: open collapsed, expand step by step, fold on the way back"
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

A progressive disclosure mode, in both the manual and the tree layout: the graph opens fully collapsed and grows one node at a time as you explore it. The full graph never has to be on screen. Asked for on 2026-10-24.

## How it should feel

1. Open: only the starting nodes show, each folded ("Computer ▸ 76").
2. Expand a node: its children appear, themselves folded.
3. Expand one of those: its children appear, and so on down a path.
4. Walk back toward the start: the branch you leave folds itself again, so the screen holds only the path you are on and what hangs off it.

## Plan

- A mode switch (a key, and a View setting such as `Disclose: yes`), off by default.
- Builds on the graph-view branch folds (`branches()` in `core/src/model.rs`, `za`): start with every node in the fold set; expanding removes one; walking back adds the node you leave.
- Expand: Shift+→ into a folded node, `za`, or a click. Walk back: Shift+← (it already remembers the parent you came from).
- Follow mode (`F`) pairs with it: each expand centres the new branch.
- New nodes appear next to their parent: in `manual` near the parent's `Pos`, in `tree` by the tree layout.
- Memory only, like branch folds, so exploring never makes a git change.

## Open questions

- Where it starts: every node nothing leads into (Computer, in the tool list), or the selected node.
- Expanding a second sibling: keep the first one open, or fold it (one open branch per level).
- Whether walking back folds only the branch you leave, or everything below the node you arrive at.

## Acceptance

- In both manual and tree, the tool list opens showing only its starting node(s), folded.
- Expanding three levels deep shows just that path and its children; walking back three levels returns to the opening view.
