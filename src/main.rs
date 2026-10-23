//! The draw_graph command. Today it checks files; the app comes next.

use std::process::ExitCode;

use draw_graph::check::check_text;

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
    let mut out =
        String::from("usage: draw_graph VERB [ARGS]\n\nDraw and edit a recfile as a graph. The app is not built yet; check is.\n\n");
    for v in VERBS {
        out.push_str(&format!("  {:<6} {:<12} {}\n", v.name, v.args, v.summary));
    }
    out.push_str("\n  -h, --help   this help\n\nexample: draw_graph check example/draw_graph_example.rec\n");
    out
}

fn long() -> String {
    format!(
        "{}
Files:     reads each FILE.rec; writes nothing.
Env:       none.
Exit:      {OK} no problems, {PROBLEMS} problems found, {USAGE} bad call or unreadable file.
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
        [other, ..] if other.ends_with(".rec") => {
            eprintln!("draw_graph: the app is not built yet - fix: draw_graph check {other}");
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
