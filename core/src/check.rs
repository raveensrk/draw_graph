//! Check a document: schema first, then the graph and its rules.
//!
//!   text ── parse ──> records ── schema ──> fields, types, keys
//!                                   └──── graph ──> names, boxes, OR, rules
//!
//! The same check gates every edit in the app: an edit that adds a problem
//! is refused. `draw_graph check` prints them.

use std::collections::HashMap;
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::graph::{self, Settings};
use crate::rec::Doc;
use crate::schema::{Schema, Type};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Problem {
    /// 1-based line of the record or descriptor field; 0 when unknown.
    pub line: usize,
    /// What it is about: `Node Bash`, `Edge Rust -> Tauri`, `%rec Node`.
    pub subject: String,
    pub message: String,
    pub fix: Option<String>,
}

impl Problem {
    pub fn new(line: usize, subject: String, message: &str, fix: Option<&str>) -> Problem {
        Problem {
            line,
            subject,
            message: message.to_string(),
            fix: fix.map(str::to_string),
        }
    }
}

impl fmt::Display for Problem {
    /// `Edge Bash -> Rustt: no node named "Rustt" - fix: correct To, or add the node`
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.subject, self.message)?;
        if let Some(fix) = &self.fix {
            write!(f, " - fix: {fix}")?;
        }
        Ok(())
    }
}

/// Every problem in the text, ordered by line.
pub fn check_text(text: &str) -> Vec<Problem> {
    match Doc::parse(text) {
        Ok(doc) => check(&doc),
        Err(errors) => errors
            .into_iter()
            .map(|e| Problem::new(e.line, "syntax".to_string(), &e.message, None))
            .collect(),
    }
}

pub fn check(doc: &Doc) -> Vec<Problem> {
    let mut out = Vec::new();
    let schema = Schema::read(doc, &mut out);
    let settings = Settings::read(&schema, &mut out);
    records(doc, &schema, &settings, &mut out);
    graph::check(doc, &schema, &settings, &mut out);
    out.sort_by_key(|p| p.line);
    out
}

/// Each record against its descriptor: fields allowed and present, values
/// of their type, keys unique, foreign keys pointing at real records.
fn records(doc: &Doc, schema: &Schema, s: &Settings, out: &mut Vec<Problem>) {
    let recs = doc.records();

    // Keys by type, for foreign keys and duplicates.
    let mut keys: HashMap<&str, Vec<(&str, usize)>> = HashMap::new();
    for r in recs.iter().filter(|r| !r.is_descriptor()) {
        let Some(t) = r.kind.as_deref().and_then(|k| schema.get(k)) else {
            continue;
        };
        if let Some(v) = t.key.as_deref().and_then(|k| r.get(k)) {
            keys.entry(t.name.as_str()).or_default().push((v, r.line));
        }
    }

    for r in recs.iter().filter(|r| !r.is_descriptor()) {
        let Some(t) = r.kind.as_deref().and_then(|k| schema.get(k)) else {
            continue;
        };
        let subject = graph::subject(r, schema, s);
        let mut say = |line: usize, message: String, fix: &str| out.push(Problem::new(line, subject.clone(), &message, Some(fix)));

        for m in &t.mandatory {
            if r.get(m).is_none() {
                say(r.line, format!("no {m}, which {} requires", t.name), &format!("add `{m}: ...`"));
            }
        }

        if let Some(k) = &t.key {
            let n = r.all(k).count();
            if n != 1 {
                say(
                    r.line,
                    format!("{n} {k} fields; the key appears exactly once"),
                    &format!("keep one {k}"),
                );
            }
        }

        for f in r.fields() {
            if !t.allows(&f.name) {
                let fix = match &t.allowed {
                    Some(a) => format!("use one of {}", a.join(" ")),
                    None => format!("remove {}; %prohibit bans it", f.name),
                };
                say(f.line, format!("{} is not a {} field", f.name, t.name), &fix);
                continue;
            }
            let Some(ty) = t.types.get(&f.name) else { continue };
            if let Some(misfit) = ty.misfit(&f.value) {
                say(f.line, format!("{}: {misfit}", f.name), &format!("fix the value of {}", f.name));
            }
            if let Type::Rec(target) = ty {
                let found = keys.get(target.as_str()).is_some_and(|ks| ks.iter().any(|(k, _)| *k == f.value));
                if !found {
                    say(
                        f.line,
                        format!("{} {:?} names no {target}", f.name, f.value),
                        &format!("correct it, or add the {target} record"),
                    );
                }
            }
        }
    }

    // Duplicate keys, also when they differ only in case or spacing:
    // "Vim", "vim" and "Vim " are one name to a reader.
    for (kind, ks) in &keys {
        let mut seen: HashMap<String, &str> = HashMap::new();
        for (k, line) in ks {
            let norm = k.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
            if let Some(first) = seen.get(&norm) {
                let msg = if first == k {
                    format!("{k:?} is used twice")
                } else {
                    format!("{k:?} is the same name as {first:?}")
                };
                out.push(Problem::new(
                    *line,
                    format!("{kind} {k}"),
                    &msg,
                    Some("rename one, or merge the two records"),
                ));
                continue;
            }
            seen.insert(norm, k);
        }
    }
}
