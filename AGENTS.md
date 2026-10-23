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
| `doc/` | Architecture and format docs, once there is code |
