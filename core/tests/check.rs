//! Each test breaks the example document in one way and expects the one
//! problem that names it. The example itself must be clean.

use draw_graph_core::check::check_text;
use draw_graph_core::rec::Doc;

const EXAMPLE: &str = include_str!("../../example/draw_graph_example.rec");

/// The example with `from` replaced by `to`, exactly once.
fn with(from: &str, to: &str) -> String {
    assert_eq!(EXAMPLE.matches(from).count(), 1, "fixture {from:?} must appear once in the example");
    EXAMPLE.replacen(from, to, 1)
}

/// Check the text and expect exactly one problem, containing `want`.
fn expect(text: &str, want: &str) {
    let problems = check_text(text);
    let lines: Vec<String> = problems.iter().map(|p| format!("{}: {p}", p.line)).collect();
    assert_eq!(
        problems.len(),
        1,
        "want one problem containing {want:?}, got:\n{}",
        lines.join("\n")
    );
    assert!(lines[0].contains(want), "want {want:?}, got {}", lines[0]);
}

#[test]
fn example_is_clean() {
    let problems = check_text(EXAMPLE);
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn example_round_trips_byte_for_byte() {
    assert_eq!(Doc::parse(EXAMPLE).unwrap().text(), EXAMPLE);
}

// Schema

#[test]
fn enum_value_outside_the_list() {
    expect(
        &with(
            "Name: Bash\nKind: tool\nLevel: 1\nTier: base",
            "Name: Bash\nKind: tool\nLevel: 1\nTier: maxx",
        ),
        "\"maxx\" is not one of base extra max",
    );
}

#[test]
fn range_value_out_of_range() {
    expect(
        &with("Name: Rust\nKind: tool\nLevel: 4", "Name: Rust\nKind: tool\nLevel: 9"),
        "\"9\" is not a whole number from 1 to 5",
    );
}

#[test]
fn regexp_value_that_does_not_match() {
    expect(&with("Pos: 600 0\n", "Pos: 600,0\n"), "does not match");
}

#[test]
fn mandatory_field_missing() {
    expect(&with("Name: HTML\nKind: tool\n", "Name: HTML\n"), "no Kind, which Node requires");
}

#[test]
fn field_not_allowed() {
    expect(
        &with("Name: HTML\nKind: tool\n", "Name: HTML\nKind: tool\nColour: #ffffff\n"),
        "Colour is not a Node field",
    );
}

#[test]
fn duplicate_name_ignoring_case() {
    expect(
        &with(
            "Name: Edit made\nKind: start\n",
            "Name: bash\nKind: tool\n\nName: Edit made\nKind: start\n",
        ),
        "\"bash\" is the same name as \"Bash\"",
    );
}

#[test]
fn foreign_key_to_nothing() {
    expect(&with("Root: Bash\n", "Root: Bashh\n"), "Root \"Bashh\" names no Node");
}

#[test]
fn unsupported_descriptor_is_reported() {
    expect(
        &with("%key: Name\n%type: Kind", "%key: Name\n%confidential: Note\n%type: Kind"),
        "does not check %confidential",
    );
}

#[test]
fn unknown_graph_setting() {
    expect(
        &with("%graph: pos Pos\n", "%graph: pos Pos\n%graph: glow Level\n"),
        "unknown %graph setting \"glow Level\"",
    );
}

// Graph

#[test]
fn edge_to_an_unknown_node() {
    expect(
        &with("From: Bash\nTo: Rust\n", "From: Bash\nTo: Rustt\n"),
        "no node or group named \"Rustt\"",
    );
}

#[test]
fn node_in_an_unknown_group() {
    expect(
        &with(
            "Name: Bash\nKind: tool\nLevel: 1\nTier: base\nTag: shell\nTag: cli\nGroup: Languages",
            "Name: Bash\nKind: tool\nLevel: 1\nTier: base\nTag: shell\nTag: cli\nGroup: Langauges",
        ),
        "Group \"Langauges\" names no Group",
    );
}

#[test]
fn group_parent_loop() {
    expect(
        &with(
            "Name: Learning\nLabel: Learning path\n",
            "Name: Learning\nLabel: Learning path\nParent: Languages\n",
        ),
        "Parent loops: Languages -> Learning -> Languages",
    );
}

#[test]
fn level_falls_along_a_requires_edge() {
    // Rust (L4) -> rec parser has no Level; Bash -> Rust is fine; make
    // Bash L5 so it sits above Rust (L4).
    let text = with("Name: Bash\nKind: tool\nLevel: 1", "Name: Bash\nKind: tool\nLevel: 5");
    let problems = check_text(&text);
    let above: Vec<_> = problems.iter().filter(|p| p.message.contains("is above")).collect();
    assert_eq!(above.len(), 2, "Bash -> Rust and Bash -> recutils: {problems:#?}");
    assert_eq!(problems.len(), 2);
}

#[test]
fn tier_falls_along_a_requires_edge() {
    expect(
        &with(
            "Name: Rust\nKind: tool\nLevel: 4\nTier: base",
            "Name: Rust\nKind: tool\nLevel: 4\nTier: max",
        ),
        "Rust (Tier max) is above Tauri (Tier base)",
    );
}

#[test]
fn requires_loop() {
    // Tasks carry no Level or Tier, so rise stays quiet and only the loop
    // is reported: rec parser -> Rule engine -> CLI check -> rec parser.
    expect(
        &with(
            "From: Rule engine\nTo: CLI check\nKind: requires\n",
            "From: Rule engine\nTo: CLI check\nKind: requires\n\nFrom: CLI check\nTo: rec parser\nKind: requires\n",
        ),
        "loop (Kind requires): rec parser -> Rule engine -> CLI check -> rec parser",
    );
}

#[test]
fn flow_loop_is_allowed() {
    assert!(EXAMPLE.contains("To: Edit made\nKind: flow"), "the example keeps its flow loop");
}

#[test]
fn implied_edge() {
    // Rust -> Tauri -> File watcher already; a direct Rust -> File watcher
    // edge says nothing new.
    expect(
        &with(
            "From: Tauri\nTo: File watcher\nKind: requires\n",
            "From: Tauri\nTo: File watcher\nKind: requires\n\nFrom: Rust\nTo: File watcher\nKind: requires\n",
        ),
        "implied through",
    );
}

#[test]
fn or_choice_met_by_an_and_edge() {
    expect(
        &with(
            "From: Rust\nTo: Tauri\nKind: requires\n",
            "From: Rust\nTo: Tauri\nKind: requires\n\nFrom: Rust\nTo: Tauri\nKind: requires\nAny: tauri-ui\n",
        ),
        "is met already: Rust is required on its own",
    );
}

#[test]
fn or_group_of_one() {
    expect(
        &with(
            "From: TypeScript\nTo: Tauri\nKind: requires\nAny: tauri-ui\n",
            "From: TypeScript\nTo: Tauri\nKind: requires\n",
        ),
        "is on one edge only",
    );
}

#[test]
fn or_group_into_two_nodes() {
    expect(
        &with(
            "From: Bash\nTo: recutils\nKind: requires\n",
            "From: Bash\nTo: recutils\nKind: requires\nAny: tauri-ui\n",
        ),
        "joins edges into different nodes",
    );
}

#[test]
fn edge_twice() {
    expect(
        &with(
            "From: Write file\nTo: Redraw\nKind: flow\n",
            "From: Write file\nTo: Redraw\nKind: flow\n\nFrom: Write file\nTo: Redraw\nKind: flow\n",
        ),
        "appears twice",
    );
}

#[test]
fn view_folds_an_unknown_node() {
    expect(
        &with("Collapse: Rule engine\n", "Collapse: Rule engin\n"),
        "no node or group named \"Rule engin\"",
    );
}

#[test]
fn view_names_an_unknown_edge_kind() {
    expect(&with("Edges: branch relates\n", "Edges: branch relate\n"), "Edges names \"relate\"");
}

#[test]
fn syntax_error_is_a_problem() {
    expect(&with("Name: Redraw\n", "Name: Redraw\nthis is not a field\n"), "not a field");
}

#[test]
fn real_file_round_trips_and_is_clean() {
    // agent1's tool list: a different schema (%rec Tool), 650 lines.
    // Skipped on a machine without it.
    let Some(home) = std::env::var_os("HOME") else { return };
    let path = std::path::Path::new(&home).join("repos/agent1/dev_tools.rec");
    let Ok(text) = std::fs::read_to_string(&path) else { return };
    assert_eq!(Doc::parse(&text).unwrap().text(), text, "{} changed on round trip", path.display());
    assert!(check_text(&text).is_empty(), "{:#?}", check_text(&text));
}
