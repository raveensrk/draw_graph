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
| `example/` | Sample recfiles |
| `src/rec.rs` | Recfile text: parse, edit, write back unchanged records byte for byte |
| `src/schema.rs` | What `%rec` descriptors say: types, keys, mandatory, allowed |
| `src/graph.rs` | `%graph` settings and the graph rules |
| `src/check.rs` | Runs every check; the gate for every edit |
| `src/main.rs` | The `draw_graph` command; one `VERBS` table drives all help |
| `tests/check.rs` | One test per problem: break the example once, expect that problem |

## Commands

- Test: `timeout 180 cargo test` (about 5 s once built).
- Lint: `timeout 120 cargo clippy --all-targets` and `cargo fmt --check`; both clean.
- Usage: `draw_graph --help`; the help is the only usage doc.
