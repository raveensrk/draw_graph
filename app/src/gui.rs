//! The window: Tauri around the Leptos web view, and the file it edits.
//!
//!   web view ── apply(edit) ──> check ── ok ──> write file, push undo
//!            <── new text ────           └─ refused: problems, file untouched
//!   watcher  ── file changed on disk (vim, git, an agent) ──> "file" event
//!
//! The file is the truth: every command reads it fresh, so an edit made
//! outside the app is never overwritten by a stale copy.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use draw_graph_core::edit::{Edit, apply as apply_edit};
use notify::{EventKind, RecursiveMode, Watcher};
use serde_json::json;
use tauri::{Emitter, Manager, State};

/// How many steps `u` can go back.
const UNDO_DEPTH: usize = 200;

/// Let a writer finish before reading what it wrote.
const SETTLE: Duration = Duration::from_millis(40);

struct Shared {
    path: PathBuf,
    /// The text last read or written, to tell our writes from others'.
    last: String,
    undo: Vec<String>,
    redo: Vec<String>,
}

type Doc = Mutex<Shared>;

/// One JSON line on stderr per event, for whoever is debugging.
fn log(event: &str, fields: serde_json::Value) {
    let mut line = json!({ "event": event });
    if let (Some(l), Some(f)) = (line.as_object_mut(), fields.as_object()) {
        l.extend(f.clone());
    }
    eprintln!("{line}");
}

/// Write through a temp file and a rename, so a reader never sees half.
fn write(path: &Path, text: &str) -> Result<(), String> {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("draw_graph");
    let tmp = path.with_file_name(format!(".{name}.draw_graph.tmp"));
    std::fs::write(&tmp, text).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("cannot read {}: {e}", path.display()))
}

#[tauri::command]
fn open(doc: State<Doc>) -> Result<String, String> {
    let mut d = doc.lock().map_err(|e| e.to_string())?;
    let text = read(&d.path)?;
    d.last = text.clone();
    log("open", json!({ "path": d.path, "bytes": text.len() }));
    Ok(json!({ "path": d.path, "text": text }).to_string())
}

/// Apply an edit (JSON). Ok: the new text, written. Err: the problems (JSON).
#[tauri::command]
fn apply(doc: State<Doc>, edit: String) -> Result<String, String> {
    let mut d = doc.lock().map_err(|e| e.to_string())?;
    let parsed: Edit = serde_json::from_str(&edit)
        .map_err(|e| json!([{ "line": 0, "subject": "edit", "message": e.to_string(), "fix": null }]).to_string())?;
    let now = read(&d.path)?;
    match apply_edit(&now, &parsed) {
        Ok(new) if new == now => {
            log("apply", json!({ "edit": parsed, "ok": true, "changed": false }));
            Ok(new)
        }
        Ok(new) => {
            write(&d.path, &new)?;
            d.undo.push(now);
            if d.undo.len() > UNDO_DEPTH {
                d.undo.remove(0);
            }
            d.redo.clear();
            d.last = new.clone();
            log("apply", json!({ "edit": parsed, "ok": true }));
            Ok(new)
        }
        Err(problems) => {
            log("apply", json!({ "edit": parsed, "ok": false, "problems": problems }));
            Err(serde_json::to_string(&problems).unwrap_or_default())
        }
    }
}

/// Step back (undo) or forward (redo) through the texts this app wrote.
fn step(doc: &Doc, back: bool) -> Result<String, String> {
    let mut d = doc.lock().map_err(|e| e.to_string())?;
    let now = read(&d.path)?;
    let popped = if back { d.undo.pop() } else { d.redo.pop() };
    let Some(text) = popped else {
        return Err(format!("nothing to {}", if back { "undo" } else { "redo" }));
    };
    write(&d.path, &text)?;
    if back {
        d.redo.push(now);
    } else {
        d.undo.push(now);
    }
    d.last = text.clone();
    log(
        if back { "undo" } else { "redo" },
        json!({ "undo": d.undo.len(), "redo": d.redo.len() }),
    );
    Ok(text)
}

/// A log line from the web view: its errors, renders and layouts.
#[tauri::command]
fn ui_log(line: String) {
    eprintln!("{line}");
}

#[tauri::command]
fn undo(doc: State<Doc>) -> Result<String, String> {
    step(&doc, true)
}

#[tauri::command]
fn redo(doc: State<Doc>) -> Result<String, String> {
    step(&doc, false)
}

/// Watch the file's folder (editors often replace the file, so watching the
/// file itself would lose it) and send each outside change to the window.
fn watch(app: tauri::AppHandle, path: PathBuf) -> notify::Result<notify::RecommendedWatcher> {
    let dir = path.parent().map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from("."));
    let mut w = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, EventKind::Access(_)) || !ev.paths.iter().any(|p| p.file_name() == path.file_name()) {
            return;
        }
        std::thread::sleep(SETTLE);
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        let doc = app.state::<Doc>();
        let Ok(mut d) = doc.lock() else { return };
        if text == d.last {
            return; // our own write, or no change
        }
        d.last = text.clone();
        d.redo.clear();
        log("reload", json!({ "bytes": text.len() }));
        let _ = app.emit("file", text);
    })?;
    w.watch(&dir, RecursiveMode::NonRecursive)?;
    Ok(w)
}

pub fn run(path: PathBuf) -> Result<(), String> {
    let path = path.canonicalize().map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let title = format!("draw_graph - {}", path.file_name().and_then(|n| n.to_str()).unwrap_or_default());
    let shared = Shared {
        path: path.clone(),
        last: String::new(),
        undo: Vec::new(),
        redo: Vec::new(),
    };

    tauri::Builder::default()
        .manage(Mutex::new(shared))
        .invoke_handler(tauri::generate_handler![open, apply, undo, redo, ui_log])
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(&title);
            }
            let watcher = watch(app.handle().clone(), path.clone())?;
            app.manage(Mutex::new(watcher));
            log("watch", json!({ "path": path }));
            Ok(())
        })
        .run(tauri::generate_context!())
        .map_err(|e| format!("the window failed: {e}"))
}
