# draw_graph

A desktop app for drawing graphs - learning paths, mind maps, flowcharts and
concept maps - saved as one plain-text [recfile](https://www.gnu.org/software/recutils/).
Keyboard first, vim keys, works offline. One native window (egui), no web view.

The spec is [PLAN.md](PLAN.md); a sample document is
[draw_graph_example.rec](example/draw_graph_example.rec).

## Install

A `.dmg` for each release is on the
[releases page](https://github.com/raveensrk/draw_graph/releases). From
source, on a Mac with Rust:

    script/bundle_macos.sh --install

That puts `draw_graph.app` in `~/Applications` and the `draw_graph` command
in `~/.local/bin`. Then run `draw_graph --help`.
