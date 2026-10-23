# draw_graph

A desktop app for drawing graphs - learning paths, mind maps, flowcharts and
concept maps - saved as one plain-text [recfile](https://www.gnu.org/software/recutils/).
Keyboard first, vim keys, works offline on macOS, Linux and Windows.

Status: the recfile library and `draw_graph check` work; the app window is
next. The spec is [PLAN.md](PLAN.md); a sample document is
[draw_graph_example.rec](example/draw_graph_example.rec).

## Install

From source, until release builds exist:

    cargo install --path .

Then run `draw_graph --help`.
