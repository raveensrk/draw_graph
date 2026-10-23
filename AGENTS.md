# AGENTS

draw_graph: a native egui desktop app that edits a recfile as a graph.

- The spec and build order are in [PLAN.md](PLAN.md). Read it before changing
  anything; record every new decision there.
- Recfile is the only storage format; JSON only as an export. Do not add YAML.
- [draw_graph_example.rec](example/draw_graph_example.rec) is the
  reference document: every format feature appears in it. A format change
  updates it in the same commit, and it must pass `recfix --check`.

## Layout

| Path | Holds |
|---|---|
| `PLAN.md` | Spec, decisions, build order |
| `task/` | Open work, one file per task; written only by `task-router` (`task-router list --dir ~/repos/draw_graph/task`) |
| `example/` | Sample recfiles |
| `core/` | The shared library: `rec.rs` text, `schema.rs` descriptors, `graph.rs` %graph rules, `check.rs` every check, `edit.rs` edits behind the check, `model.rs` what the window draws, `tree.rs` the tree view |
| `core/tests/check.rs` | One test per problem: break the example once, expect that problem |
| `app/` | The `draw_graph` binary: `main.rs` the command line (one `VERBS` table drives all help) and the window launch; `app.rs` state and keys; `panel.rs` the side panel; `scene.rs` what is drawn; `draw.rs` painting and SVG; `file.rs` writes and undo; `icons/` from `icon.svg` |
| `script/bundle_macos.sh` | `draw_graph.app` and a `.dmg`; `--install` copies them into place |

## Commands

- Test: `timeout 300 cargo test -p draw_graph_core -p draw_graph` (about 5 s once built).
- Lint: `cargo clippy -p draw_graph_core -p draw_graph --all-targets` and `cargo fmt --all --check`; both clean.
- Run: `cargo run -p draw_graph -- FILE.rec`; install: `script/bundle_macos.sh --install`.
- Debug the window: its stderr is one JSON line per event (`open`, `apply`, `undo`, `redo`, `reload`, `prompt`, `submit`). Test on a copy of a file, never on the example in place.
- Driving the window from a test tool: background clicks do not reach egui (use foreground delivery), and a typed character may arrive with a stray key code (key code 0 is `A`).
- Usage: `draw_graph --help`; the help is the only usage doc.
