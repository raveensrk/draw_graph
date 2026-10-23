//! The draw_graph command: open a recfile in the window, or check it.
//! The window is egui: one native window, no web view.

mod app;
mod draw;
mod file;
mod panel;
mod scene;

use std::path::PathBuf;
use std::process::ExitCode;

use draw_graph_core::check::check_text;

/// Exit codes: no problems, problems found, the call itself was wrong.
const OK: u8 = 0;
const PROBLEMS: u8 = 1;
const USAGE: u8 = 2;

struct Verb {
    name: &'static str,
    args: &'static str,
    summary: &'static str,
    /// Extra lines for the verb's own help.
    detail: &'static str,
    example: &'static str,
}

/// One table drives the main help and every verb's help.
const VERBS: &[Verb] = &[
    Verb {
        name: "open",
        args: "FILE.rec",
        summary: "open the file in the window; `draw_graph FILE.rec` does the same",
        detail: "Every accepted edit is written to the file at once; an edit that adds
a problem is refused and the file is left as it was. Changes made by vim,
git or an agent show up in the window as soon as they land. Press ? in
the window for the keys.

Logs: one JSON line per event (open, apply, undo, redo, reload) on stderr.",
        example: "draw_graph example/draw_graph_example.rec",
    },
    Verb {
        name: "check",
        args: "FILE.rec...",
        summary: "print each file's problems, one per line; exit 1 on any",
        detail: "Checks the schema (%type, %key, %mandatory, %allowed, %prohibit),
foreign keys (%type: F rec Type), duplicate names (ignoring case and
spacing), then the graph: edge ends, group parents, OR groups (Any),
repeated edges, views, and the %graph rules acyclic, rise and no_implied.

Output: FILE:LINE: SUBJECT: PROBLEM - fix: HOW, sorted by line. vim's
:cfile reads it. A count goes to stderr.",
        example: "draw_graph check example/draw_graph_example.rec",
    },
    Verb {
        name: "help",
        args: "[VERB]",
        summary: "long help, or one verb's help",
        detail: "",
        example: "draw_graph help check",
    },
];

fn usage() -> String {
    let mut out = String::from("usage: draw_graph FILE.rec | VERB [ARGS]\n\nDraw and edit a recfile as a graph.\n\n");
    for v in VERBS {
        out.push_str(&format!("  {:<6} {:<12} {}\n", v.name, v.args, v.summary));
    }
    out.push_str("\n  -h, --help   this help\n\nexample: draw_graph check example/draw_graph_example.rec\n");
    out
}

fn long() -> String {
    format!(
        "{}
Files:     open reads and writes FILE.rec (through a .FILE.rec.draw_graph.tmp next
           to it); check reads each FILE.rec and writes nothing.
Env:       none.
Exit:      {OK} no problems (check) or window closed (open), {PROBLEMS} problems
           found, {USAGE} bad call, unreadable file or the window failed.
Spec:      PLAN.md in the draw_graph repo.

Run `draw_graph help VERB` for a verb's detail.
",
        usage()
    )
}

fn verb_help(v: &Verb) -> String {
    let mut out = format!("usage: draw_graph {} {}\n\n{}.\n", v.name, v.args, v.summary);
    if !v.detail.is_empty() {
        out.push_str(&format!("\n{}\n", v.detail));
    }
    out.push_str(&format!("\nexample: {}\n", v.example));
    out
}

fn find(name: &str) -> Option<&'static Verb> {
    VERBS.iter().find(|v| v.name == name)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let wants_help = |rest: &[&str]| rest.iter().any(|a| *a == "-h" || *a == "--help");

    // Opened as an app (Finder, Dock, Spotlight) there is no file: ask for one.
    if words.is_empty() && launched_as_app() {
        let Some(file) = rfd::FileDialog::new().set_title("Open a recfile").add_filter("recfile", &["rec"]).pick_file() else {
            return ExitCode::from(OK);
        };
        return match run(file) {
            Ok(()) => ExitCode::from(OK),
            Err(e) => {
                eprintln!("draw_graph: {e}");
                ExitCode::from(USAGE)
            }
        };
    }

    match words.as_slice() {
        [] | ["-h" | "--help", ..] => {
            print!("{}", usage());
            ExitCode::from(OK)
        }
        ["help"] => {
            print!("{}", long());
            ExitCode::from(OK)
        }
        ["help", name, ..] => match find(name) {
            Some(v) => {
                print!("{}", verb_help(v));
                ExitCode::from(OK)
            }
            None => {
                eprintln!("draw_graph: no verb {name:?} - fix: draw_graph help");
                ExitCode::from(USAGE)
            }
        },
        ["check", rest @ ..] if wants_help(rest) => {
            print!("{}", verb_help(find("check").expect("check is in VERBS")));
            ExitCode::from(OK)
        }
        ["check"] => {
            eprintln!("draw_graph: check needs a file - fix: draw_graph check FILE.rec");
            ExitCode::from(USAGE)
        }
        ["check", files @ ..] => check(files),
        ["open", rest @ ..] if wants_help(rest) => {
            print!("{}", verb_help(find("open").expect("open is in VERBS")));
            ExitCode::from(OK)
        }
        ["open", file] | [file] if file.ends_with(".rec") => match run(PathBuf::from(file)) {
            Ok(()) => ExitCode::from(OK),
            Err(e) => {
                eprintln!("draw_graph: {e}");
                ExitCode::from(USAGE)
            }
        },
        ["open", ..] => {
            eprintln!("draw_graph: open takes one FILE.rec - fix: draw_graph open FILE.rec");
            ExitCode::from(USAGE)
        }
        [other, ..] => {
            eprintln!("draw_graph: no verb {other:?} - fix: draw_graph --help");
            ExitCode::from(USAGE)
        }
    }
}

fn check(files: &[&str]) -> ExitCode {
    let mut total = 0;
    for file in files {
        let text = match std::fs::read_to_string(file) {
            Ok(t) => t,
            Err(err) => {
                eprintln!("draw_graph: cannot read {file}: {err}");
                return ExitCode::from(USAGE);
            }
        };
        let problems = check_text(&text);
        for p in &problems {
            println!("{file}:{}: {p}", p.line);
        }
        total += problems.len();
    }

    let noun = if total == 1 { "problem" } else { "problems" };
    eprintln!("{total} {noun}");
    ExitCode::from(if total == 0 { OK } else { PROBLEMS })
}

/// A system font with ▸, ↑, ↺ and arrows, added after egui's own fonts so
/// they fill only what those lack. Without one, plain ASCII marks.
const SYMBOLS: &[&str] = &[
    "/System/Library/Fonts/Apple Symbols.ttf",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
];

fn fonts(ctx: &eframe::egui::Context) -> &'static scene::Marks {
    use eframe::egui::{FontData, FontDefinitions, FontFamily};
    let Some(bytes) = SYMBOLS.iter().find_map(|p| std::fs::read(p).ok()) else {
        return &scene::PLAIN;
    };
    let mut defs = FontDefinitions::default();
    defs.font_data
        .insert("symbols".into(), std::sync::Arc::new(FontData::from_owned(bytes)));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        defs.families.entry(family).or_default().push("symbols".into());
    }
    ctx.set_fonts(defs);
    &scene::FANCY
}

/// Started from a macOS .app bundle rather than a terminal.
fn launched_as_app() -> bool {
    std::env::current_exe().is_ok_and(|p| p.to_string_lossy().contains(".app/Contents/MacOS/"))
}

/// Open the window on a file; returns when it closes.
fn run(path: PathBuf) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let file = file::File::open(path)?;
    let title = format!(
        "draw_graph - {}",
        file.path.file_name().and_then(|f| f.to_str()).unwrap_or_default()
    );
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1400.0, 900.0])
            .with_min_inner_size([700.0, 450.0])
            .with_title(&title),
        ..Default::default()
    };
    eframe::run_native(
        "draw_graph",
        options,
        Box::new(move |cc| {
            cc.egui_ctx.set_visuals(eframe::egui::Visuals::dark());
            let marks = fonts(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc, file, marks)))
        }),
    )
    .map_err(|e| format!("the window failed: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_verb_has_help_and_an_example() {
        for v in VERBS {
            assert!(usage().contains(v.name), "{} missing from the main help", v.name);
            assert!(
                !v.summary.is_empty() && v.example.starts_with("draw_graph "),
                "{} needs a summary and an example",
                v.name
            );
        }
    }
}
