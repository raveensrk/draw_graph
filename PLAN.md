# draw_graph plan

A desktop app for drawing graphs and seeing ideas as graphs: learning paths,
mind maps, flowcharts and concept maps. Every document is one plain-text
recfile that a person, git or an agent can edit while the app is open.

Status: planned. No code yet. The reference document is
[draw_graph_example.rec](example/draw_graph_example.rec).

## Decisions

| Area | Decision |
|---|---|
| Platforms | macOS, Linux and Windows; fully offline, no account, no server |
| Stack | [Tauri](https://tauri.app): Rust for files, parsing and rules; a web view for the UI |
| Drawing | [Cytoscape.js](https://js.cytoscape.org); compound nodes give nested boxes |
| Layout | [ELK.js](https://github.com/kieler/elkjs) layered, tree and radial, plus force; drag to adjust, positions saved in the file |
| Format | Recfile only. JSON only between the Rust side and the web view, and as an export. YAML dropped |
| Input | Keyboard first, vim keys; mouse optional |
| Export | SVG |
| Name | `draw_graph` for the app, the command and the repo (chosen 2026-10-23). A verb phrase as a folder name breaks agent1 naming.md's noun rule; chosen anyway, for one name everywhere. Free on crates.io, PyPI and npm |
| Repo | Public, on GitHub |
| Install | Release builds from CI for each tag: `.dmg`, `.msi`, `.AppImage` |

### Why recfile

Compared on the example graph (581 lines rec, 526 YAML, 951 JSON):

- Comments survive, and the app saves without losing them.
- One `Field: value` per line: nothing to escape, no indentation that
  carries meaning, no value silently turned into a boolean or number.
- `recfix` checks `%type`, `%mandatory` and `%allowed` for free.
- A changed record changes only its own lines, so git diffs and merges
  stay small.

Costs, accepted: a parser of our own, our own check that edge endpoints exist
(`recfix` does not check `%type: X rec Node`), and typing values from
`%type`.

## File format

One file holds four record types. Field names in `PascalCase`, as recutils
does.

| Record | Holds | Key |
|---|---|---|
| `Group` | A box on the canvas. `Parent` nests it in another box; `Collapsed` folds it to one node | `Name` |
| `Node` | One node. Its fields come from the schema | `Name` |
| `Edge` | One arrow: `From`, `To`, `Kind`, optional `Label`, `Any`, `Note` | none |
| `View` | A saved way of looking at the graph: layout, direction, root, folds | `Name` |

- **Node fields** come from the schema: `%type`, `%allowed` and `%mandatory`
  build the edit form; an `enum` or `range` becomes a dropdown.
- **Edges** are AND by default. Edges into one node that share an `Any`
  value are OR, drawn dashed: Tauri needs Rust, and TypeScript or
  JavaScript.
- **Edge kinds** in the example: `requires` (learning path), `flow`
  (flowchart, may loop), `branch` (mind map), `relates` (concept map,
  labelled, dotted).
- **`%graph:` lines** in a record descriptor are draw_graph settings.
  `recfix` ignores them (tested). They name the special fields, the styles
  and the rules:

```
%graph: label Label Name
%graph: color Level
%graph: shape Kind tool=round decision=diamond ...
%graph: pos Pos
%graph: group Group
%graph: filter Tier
%graph: acyclic Kind=requires
%graph: rise Level Tier when Kind=requires
%graph: no_implied when Kind=requires
```

### Rules

Checked on every edit; an edit that breaks one is refused and the problems
shown, the same gate `dev_tools.py` has today.

| Rule | Meaning |
|---|---|
| Schema | `recfix --check` |
| Endpoints | `From` and `To` name a real record |
| Duplicates | Names equal ignoring case and spaces |
| `acyclic` | No loop among the matching edges |
| `rise F...` | An edge never points from a higher value to a lower one of each field (an ordered enum counts in its listed order). A node without the field is skipped |
| `no_implied` | No edge already implied through another path; no OR line met by an AND edge |

## Views

### Graph view

Layered, radial, force or manual. A tier filter (`%graph: filter Tier`)
shows a tier and every tier before it.

### Tree view

Any node can be the root: focus it and press a key. `Follow: out` walks
what it unlocks; `Follow: in` walks what it needs.

A node with several parents appears under each one. `Repeat` decides how:

- `fold`, the default: the first copy shows its whole branch; every later
  copy starts folded with a "same as above" marker that jumps to the
  first. `za` still unfolds it. Copies never multiply down a chain.
- `full`: every copy shows its whole branch. For small trees only.

Folds are stored by node name in the view's `Collapse` lines, so folding a
node folds every copy of it.

### Styling

Colours and shapes come from fields (`%graph: color Level`,
`%graph: shape Kind`). A node's own `Color` or `Shape` overrides the
mapping. Dark theme by default.

## Keys

Vim keys on the canvas. First draft:

| Key | Does |
|---|---|
| `h` `j` `k` `l` | Move to the nearest node in that direction |
| `o`, `O` | Add a child, add a parent |
| `dd` | Delete the node and its edges |
| `u`, `Ctrl-r` | Undo, redo |
| `za` | Fold or unfold |
| `/`, `n`, `N` | Search, next, previous |
| `?` | Help page |

## Saving and live reload

- Saving rewrites only the records that changed, in schema field order;
  every other line, comment and blank line stays as it was.
- The app watches the file. An edit from vim, git or an agent shows at once.
  Unsaved edits in the app are merged; a clash is shown as a conflict, never
  overwritten.

## Command line

The same binary runs headless: `draw_graph check FILE.rec` prints
problems and exits 1 on any. agent1's
[dev_tools_rec.py](https://github.com/raveensrk/agent1/blob/main/harness/checks/dev_tools_rec.py)
calls it once `dev_tools.py` is retired.

## Build order

1. Recfile parse and save, rules, `draw_graph check`.
2. Canvas: Tauri window, Cytoscape.js, ELK layouts, vim keys, the edit form.
3. Tree view: root, follow, fold, `Repeat`.
4. Nested groups and SVG export.
5. Convert agent1's `dev_tools.rec` to Edge records; retire `dev_tools.py`
   (its editor, charts and checks).
6. CI release builds.

## Known risks

- **Code signing.** Without an Apple Developer ID (99 USD a year) macOS
  warns on the first launch of the `.dmg`; Windows SmartScreen warns too.
- **Edges onto groups.** The example has `To: Save flow`, a group, while the
  schema says `%type: From,To rec Node`. Decide whether an edge may end on a
  box.
- **Views in the data file.** Folding a node is a git change. A side file is
  the alternative.

## Origin

Grew out of agent1's
[dev_tools.py](https://github.com/raveensrk/agent1/blob/main/dev_tools.py):
validation, the text-preserving `edit()`, and a Cytoscape.js node editor
served on localhost. Searched for an existing app on 2026-10-23 (roadkit,
arrows.app, sane-graph-edit, draw.io CSV import, yEd, magjac's Graphviz
editor); none edits a recfile as a graph and writes it back.
