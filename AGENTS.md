# AGENTS

draw_graph: a Tauri desktop app that edits a recfile as a graph.

- The spec and build order are in [PLAN.md](PLAN.md). Read it before changing
  anything; record every new decision there.
- Recfile is the only storage format. JSON is used only between the Rust side
  and the web view, and as an export. Do not add YAML.
- [draw_graph_example.rec](example/draw_graph_example.rec) is the
  reference document: every format feature appears in it. A format change
  updates it in the same commit, and it must pass `recfix --check`.

## Layout

| Path | Holds |
|---|---|
| `PLAN.md` | Spec, decisions, build order |
| `task/` | Open work, one file per task; written only by `task-router` (`task-router list --dir ~/repos/draw_graph/task`) |
| `example/` | Sample recfiles |
| `core/` | The shared library (also built for wasm): `rec.rs` text, `schema.rs` descriptors, `graph.rs` %graph rules, `check.rs` every check, `edit.rs` edits behind the check, `model.rs` what the UI draws |
| `core/tests/check.rs` | One test per problem: break the example once, expect that problem |
| `app/` | The `draw_graph` binary: the command line (`main.rs`, one `VERBS` table drives all help) and the Tauri window (`gui.rs`: commands, watcher, undo) |
| `ui/` | The Leptos web view (`src/main.rs`), `canvas.js` (Cytoscape glue), vendored JS in `vendor/` |

## Commands

- Test: `timeout 300 cargo test -p draw_graph_core -p draw_graph` (about 5 s once built).
- Lint: `cargo clippy -p draw_graph_core -p draw_graph --all-targets`, `cargo clippy -p draw_graph_ui --target wasm32-unknown-unknown`, `cargo fmt --all --check`; all clean.
- Build the app with the web view inside: `cd app && cargo tauri build --debug --no-bundle`, then `target/debug/draw_graph FILE.rec`. A plain `cargo build` loads the page from `trunk serve` on 127.0.0.1:1420 instead (`cargo tauri dev`).
- Debug the window: its stderr is one JSON line per event (`open`, `apply`, `undo`, `reload`, and from the web view `render`, `layout`, `ui_error`). Test on a copy of a file, never on the example in place.
- Usage: `draw_graph --help`; the help is the only usage doc.
- Needs: `rustup target add wasm32-unknown-unknown`, `cargo install --locked trunk --version '~0.21'`, `cargo install --locked tauri-cli --version '^2'`.
