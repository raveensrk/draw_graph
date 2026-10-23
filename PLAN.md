# draw_graph plan

A desktop app for drawing graphs and seeing ideas as graphs: learning paths,
mind maps, flowcharts and concept maps. Every document is one plain-text
recfile that a person, git or an agent can edit while the app is open.

Status: all six steps of the build order done; on 2026-10-24 the window
moved from Tauri + Leptos + Cytoscape.js to egui (see "The egui switch"
below). The reference document is
[draw_graph_example.rec](example/draw_graph_example.rec).

## Decisions

| Area | Decision |
|---|---|
| Platforms | macOS today (the window is egui, which also runs on Linux and Windows); fully offline, no account, no server |
| Stack | Rust only. [egui](https://github.com/emilk/egui) 0.35 through eframe: one native window, no web view, no JavaScript. The window and `draw_graph check` share the `core` crate, so forms, colours and rules all come from the file's schema |
| Drawing | Painted by draw_graph each frame (`app/src/draw.rs`) from a plain-data scene (`app/src/scene.rs`); the same geometry writes the SVG export |
| Layout | Two: `manual` (the saved `Pos`; drag to move, written to the file) and `tree` (a tidy tree of the whole graph from `core/src/tree.rs`; it leaves group boxes out, since one box's members can land far apart). The tree view (`t`, `T`) has its own layout |
| Format | Recfile only. JSON only as an export. YAML dropped |
| Input | Keyboard first, vim keys; mouse optional |
| Saving | Every accepted edit is written at once (temp file, then rename). Undo and redo step through the texts the app wrote |
| Notes | A node's or edge's `Note` shows on the canvas (and in the SVG): its first line under the label, cut to 40 characters with an ellipsis. The full note is in the side panel |
| Links in the panel | A node's panel has an Incoming pane (what leads here) and an Outgoing pane (what it leads to); an edge's has a From pane and a To pane, each end with its tags and note. A name goes to that node; `link` opens the edge |
| Edit form | A side panel, built from the schema: an enum or small range is a dropdown, `Note` a text area. A repeated field (`Collapse`) is shown, not edited, since one input would flatten it |
| Export | SVG |
| Icon | [icon.svg](app/icons/icon.svg), dark: a graph branching left to right like the tree view, the root in the canvas's gold focus ring, nodes in the level colours, one dashed OR edge. Every file in `app/icons/` is rendered from it |
| Name | `draw_graph` for the app, the command and the repo (chosen 2026-10-23), one name everywhere. Free on crates.io, PyPI and npm |
| Repo | Public, on GitHub |
| Schema checks | Done by draw_graph itself, not by calling `recfix`: recutils is hard to get on Windows. A descriptor field or `%type` it cannot check is reported, never skipped |
| Edges onto groups | Allowed: `From` and `To` may name a node or a group; the arrow meets the group's box. draw_graph checks both ends itself |
| Install | `script/bundle_macos.sh --install` builds `draw_graph.app` into `~/Applications` and the command into `~/.local/bin`. Release builds from CI for each `v*` tag: a `.dmg` for Apple silicon and one for Intel |

### The egui switch

On 2026-10-24 the window moved from Tauri 2 + Leptos (wasm) + Cytoscape.js +
ELK.js to egui, after a prototype (task "Prototype an egui version of the
app"). With only the manual and tree layouts kept, ELK was no longer needed,
and every rule already lived in `core`. What changed:

- One language and one build tool: no wasm target, Trunk or tauri-cli.
- UI code went from 2,114 lines (Rust, JavaScript, CSS, HTML) to about 2,700
  lines of Rust that also do the drawing Cytoscape did.
- The bugs that came from the web layer (keys leaking between the page and
  Rust, a JavaScript error panicking a Rust task, a Cytoscape redraw error)
  have no place to happen.
- Lost for now: Linux and Windows release builds (eframe runs there; the
  bundle steps are not written).

### Why recfile

Compared on the example graph (581 lines rec, 526 YAML, 951 JSON):

- Comments survive, and the app saves without losing them.
- One `Field: value` per line: nothing to escape, no indentation that
  carries meaning, no value silently turned into a boolean or number.
- `%type`, `%mandatory` and `%allowed` describe the fields in the file
  itself, and `recfix` can check them on a machine that has recutils.
- A changed record changes only its own lines, so git diffs and merges
  stay small.

Costs, accepted: a parser of our own, our own schema check (built in step
1; `recfix` does not even check that `%type: X rec Node` points at a real
record), and typing values from `%type`.

## File format

One file holds four record types. Field names in `PascalCase`, as recutils
does.

| Record | Holds | Key |
|---|---|---|
| `Group` | A box on the canvas. `Parent` nests it in another box; `Collapsed` folds it to one node | `Name` |
| `Node` | One node. Its fields come from the schema | `Name` |
| `Edge` | One arrow: `From`, `To` (each a node or a group), `Kind`, optional `Label`, `Any`, `Note` | none |
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
%graph: acyclic when Kind=requires
%graph: rise Level Tier when Kind=requires
%graph: no_implied when Kind=requires
```

### Rules

Checked on every edit; an edit that breaks one is refused and the problems
shown, the same gate `dev_tools.py` has today.

| Rule | Meaning |
|---|---|
| Schema | `%type` (int, real, bool, line, range, regexp, enum, rec, `%typedef`), `%key`, `%mandatory`, `%allowed`, `%prohibit` |
| Foreign keys | A `rec Type` field names a real record of that type |
| Endpoints | `From` and `To` name a real node or group; `Group` and `Parent` name real groups, and parents never loop |
| OR groups | An `Any` value joins two or more edges, all into one node |
| Repeats | The same edge twice |
| Views | `Root` and `Collapse` name real nodes or groups; `Edges` names real edge kinds |
| Duplicates | Names equal ignoring case and spaces |
| `acyclic` | No loop among the matching edges |
| `rise F...` | An edge never points from a higher value to a lower one of each field (an ordered enum counts in its listed order). A node without the field is skipped |
| `no_implied` | No edge already implied through another path; no OR choice met by an AND edge. Skipped while a loop is reported, since inside a loop every edge looks implied |

A rule takes `when Field=value` to apply only to some edges. Problems print
as `FILE:LINE: SUBJECT: PROBLEM - fix: HOW`, the shape vim's `:cfile`
reads.

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

Folds are by node name, so folding a node folds every copy of it. Where a
fold lives depends on where the tree came from:

- A saved view (`V` steps through the View records): its `Collapse` lines,
  written at once like any edit.
- A quick tree (`t` what it unlocks, `T` what it needs): memory only, so
  folding never makes a git change by accident.

A quick tree follows the commonest edge kind around its root (`requires`
from a tool, `branch` from the mind map centre); `;` steps through the
kinds and back to all. A repeat opened with `za` is opened by its place in
the tree, in memory only.

The tree is laid out by draw_graph itself (`core/src/tree.rs`), not by
ELK: each leaf its own row, each parent centred on its children, elbow
edges. Groups are not drawn as boxes in a tree, since one node can show in
several places.

### Tags

`%graph: tag Tag` names a repeated field of free labels, one `Tag:` line
each. The side panel shows a node's tags as chips (× removes, the field adds
on Enter), and with nothing focused lists every tag with its count; a click
filters the graph to the nodes with that tag, their boxes and the edges
between them (click again or Esc to clear). `%graph: color` on a field with
no order, like Tag, colours by the first value. agent1's dev_tools.rec moved
from Level, Tier, Category and Subcategory to tags on 2026-10-24.

### Follow mode

`F` turns it on: every new selection (keys, click, search, `gd`, an edit)
zooms the view to 1.3x and glides the selected node to the middle. `zz`
does it once. Off at start; memory only.

### Folding a branch

In the graph view `za` on a node hides everything downstream of it, shown
as "Python ▸ 9"; a node another open path still reaches stays. Memory only,
like a quick tree. `zR` unfolds them.

### Group boxes

A group is a box; boxes nest through `Parent`. `Collapsed: yes` on a Group
record draws it as one node, "Name ▸ N" for the N nodes inside, placed at
the middle of their `Pos`. Edges to hidden members meet the box instead,
merged when several land on the same pair. Inside nested folds the outermost
collapsed box stands for all. In the graph view `za` folds the focused box,
`zR`/`zM` every box; the field is written like any edit.

`G` puts the focused node in a new group nested in its current one. `dd` on
a box deletes the box, not what is in it: members and child boxes move up to
its parent, edges onto the box go with it.

### SVG export

`E` writes what is on screen to `FILE.svg` next to `FILE.rec`: dark
background, boxes, edges (bezier, elbow, dashed, dotted) with labels,
nodes in their shapes. draw_graph writes the SVG itself, no export library.

### Styling

Colours and shapes come from fields (`%graph: color Level`,
`%graph: shape Kind`). A node's own `Color` or `Shape` overrides the
mapping. Dark theme by default.

## Keys

Vim keys on the canvas. The help page (`?`) lists them from the same table
the code uses (`KEYS` in `ui/src/main.rs`). Shipped in step 2:

`h j k l` or the arrow keys move to the nearest node by position;
Shift + arrows move along the drawn edges (→ what it unlocks, ← a
prerequisite, ↑ ↓ siblings under the parent you came from); `e` step through edges, `Enter`/`i` edit in the panel,
`Esc` back, `o`/`O` add after/before, `a` add alone, `c` connect, `dd`
delete, `u`/`Ctrl-r` undo/redo, `/` `n` `N` search, `L` next layout (or
leave a tree), `s` save positions, `f` fit, `?` help.

Tree view (step 3): `t`/`T` quick tree, `V` next saved view, `;` next edge
kind, `za` fold or open a repeat, `zR`/`zM` unfold or fold all, `gd` from a
repeat to its first copy.

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
calls it (since 2026-10-24), from `~/.local/bin/draw_graph`.

## Build order

1. Recfile parse and save, rules, `draw_graph check`. Done 2026-10-23.
2. Canvas: Tauri window, Cytoscape.js, ELK layouts, vim keys, the edit form,
   live reload. Done 2026-10-23.
3. Tree view: root, follow, fold, `Repeat`. Done 2026-10-23.
4. Nested groups and SVG export. Done 2026-10-23.
5. Convert agent1's `dev_tools.rec` to Edge records; retire `dev_tools.py`
   (its editor, charts and checks). Done 2026-10-24: 77 Node, 80 Edge and 3
   Group records, proven identical to the old file; the charts dropped;
   agent1's `dev_tools_rec.py` runs `recfix` and `draw_graph check`.
6. CI release builds: `.github/workflows/release.yml` builds each `v*` tag
   into a draft release; `ci.yml` tests every push. macOS only for now
   (Apple silicon, Intel), by choice on 2026-10-24; Linux and Windows are two
   matrix rows away.

## Known risks

- **Code signing.** Without an Apple Developer ID (99 USD a year) macOS
  warns on the first launch of the `.dmg`; Windows SmartScreen warns too.
- **ELK `tree` layout and boxes.** The graph layout named `tree` (ELK's
  `mrtree`) lets group boxes overlap. The tree view has its own layout;
  the ELK one may go.
- **Headless export.** SVG export needs the window, since the layout lives in
  Cytoscape. agent1's charts were dropped in step 5, so nothing needs it yet.
- **A Cytoscape redraw bug.** Swapping elements once threw inside
  Cytoscape's renderer after a box was removed while focused; headless
  Cytoscape did not reproduce it. Redraws now remove whole top-level
  elements, and a failed swap rebuilds the canvas (logged as `ui_error`).
- **`width: label` is deprecated** in Cytoscape 3.30; it still works.
- **View `Filter` is not applied.** `V` shows a view's layout, root and
  edge kinds, and says when its `Filter` expression is skipped.
- **Trunk pinned to 0.21.** Trunk 0.22 needs rustc 1.96.1; this machine has
  1.94.0. Move up when Rust is updated.
- **Views in the data file.** Folding a node is a git change. A side file is
  the alternative.

## Origin

Grew out of agent1's
[dev_tools.py](https://github.com/raveensrk/agent1/blob/main/dev_tools.py):
validation, the text-preserving `edit()`, and a Cytoscape.js node editor
served on localhost. Searched for an existing app on 2026-10-23 (roadkit,
arrows.app, sane-graph-edit, draw.io CSV import, yEd, magjac's Graphviz
editor); none edits a recfile as a graph and writes it back.
