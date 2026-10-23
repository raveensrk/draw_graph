//! The file the window edits: every edit checked, written at once, undoable.
//!
//!   edit ── core::edit::apply ── ok ──> write FILE.rec, push undo
//!                                └─ refused: the problems, file untouched
//!
//! The file is the truth: every edit reads it fresh, so a change made outside
//! the app (vim, git, an agent) is never overwritten by a stale copy.

use std::path::{Path, PathBuf};

use draw_graph_core::check::Problem;
use draw_graph_core::edit::{Edit, apply};

/// How many steps `u` can go back.
const UNDO_DEPTH: usize = 200;

pub struct File {
    pub path: PathBuf,
    /// The text last read or written, to tell our writes from others'.
    pub text: String,
    undo: Vec<String>,
    redo: Vec<String>,
}

/// One JSON-ish line on stderr per event, for whoever is debugging.
pub fn log(event: &str, detail: &str) {
    eprintln!("{{\"event\":\"{event}\",\"detail\":{detail:?}}}");
}

/// Write through a temp file and a rename, so a reader never sees half.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("draw_graph");
    let tmp = path.with_file_name(format!(".{name}.draw_graph.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

impl File {
    pub fn open(path: PathBuf) -> Result<File, String> {
        let text = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        log("open", &path.display().to_string());
        Ok(File {
            path,
            text,
            undo: Vec::new(),
            redo: Vec::new(),
        })
    }

    /// Read the file again after an outside change. True when it changed.
    pub fn reload(&mut self) -> bool {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return false;
        };
        if text == self.text {
            return false; // our own write, or no change
        }
        self.text = text;
        self.redo.clear();
        log("reload", &format!("{} bytes", self.text.len()));
        true
    }

    /// Apply an edit. Err holds the problems it would add, or why it failed.
    pub fn apply(&mut self, edit: &Edit) -> Result<(), Vec<Problem>> {
        let fail = |message: String| vec![Problem::new(0, "edit".into(), &message, None)];
        let now = std::fs::read_to_string(&self.path).map_err(|e| fail(e.to_string()))?;
        let new = apply(&now, edit)?;
        if new == now {
            return Ok(());
        }
        write(&self.path, &new).map_err(fail)?;
        self.undo.push(now);
        if self.undo.len() > UNDO_DEPTH {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.text = new;
        log("apply", &format!("{edit:?}"));
        Ok(())
    }

    /// Step back (undo) or forward (redo) through the texts this app wrote.
    pub fn step(&mut self, back: bool) -> Result<(), String> {
        let now = std::fs::read_to_string(&self.path).map_err(|e| e.to_string())?;
        let popped = if back { self.undo.pop() } else { self.redo.pop() };
        let Some(text) = popped else {
            return Err(format!("nothing to {}", if back { "undo" } else { "redo" }));
        };
        write(&self.path, &text)?;
        if back {
            self.redo.push(now);
        } else {
            self.undo.push(now);
        }
        self.text = text;
        log(if back { "undo" } else { "redo" }, "");
        Ok(())
    }
}
