//! Recfile text: parse it into records, edit them, write it back.
//!
//! Writing keeps the text: a record nobody changed comes back byte for
//! byte, and a changed one is rewritten from its fields, its comment lines
//! kept where they were. Everything between records is kept as it was.
//!
//!   # Dev tools            <- Chunk::Text   (comments, blank lines)
//!
//!   %rec: Node             <- Chunk::Record (a descriptor)
//!   %key: Name
//!                          <- Chunk::Text
//!   Name: Bash             <- Chunk::Record, kind Node
//!   Note: first line
//!   + second line          <- continuation of Note

use std::fmt;

/// One `Name: value` field. A multi-line value holds `\n`.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub name: String,
    pub value: String,
    /// 1-based line in the parsed text; 0 for a field added since.
    pub line: usize,
}

#[derive(Clone, Debug)]
enum Item {
    Field(Field),
    Comment(String),
}

#[derive(Clone, Debug)]
pub struct Record {
    /// The `%rec` type this record belongs to; None before any descriptor.
    pub kind: Option<String>,
    /// 1-based line of the first field; 0 for a record added since.
    pub line: usize,
    items: Vec<Item>,
    raw: String,
    dirty: bool,
}

impl Record {
    pub fn fields(&self) -> impl Iterator<Item = &Field> {
        self.items.iter().filter_map(|item| match item {
            Item::Field(f) => Some(f),
            Item::Comment(_) => None,
        })
    }

    /// The first value of a field.
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields().find(|f| f.name == name).map(|f| f.value.as_str())
    }

    /// Every value of a field, in order.
    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a str> + 'a {
        self.fields().filter(move |f| f.name == name).map(|f| f.value.as_str())
    }

    /// A descriptor is a record whose first field is `%rec`.
    pub fn is_descriptor(&self) -> bool {
        self.fields().next().is_some_and(|f| f.name == "%rec")
    }

    fn render(&self) -> String {
        if !self.dirty {
            return self.raw.clone();
        }
        let mut out = String::new();
        for item in &self.items {
            match item {
                Item::Field(f) if f.value.is_empty() => out.push_str(&format!("{}:\n", f.name)),
                Item::Field(f) => out.push_str(&format!("{}: {}\n", f.name, f.value.replace('\n', "\n+ "))),
                Item::Comment(c) => out.push_str(&format!("{c}\n")),
            }
        }
        out
    }
}

#[derive(Clone, Debug)]
enum Chunk {
    Text(String),
    Record(Record),
}

/// A line the parser could not read.
#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

#[derive(Clone, Debug)]
pub struct Doc {
    chunks: Vec<Chunk>,
}

/// `Name` or `%name`: a letter, then letters, digits or underscores.
fn is_name(name: &str) -> bool {
    let name = name.strip_prefix('%').unwrap_or(name);
    let mut chars = name.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic()) && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Doc {
    pub fn parse(text: &str) -> Result<Doc, Vec<ParseError>> {
        let mut chunks = Vec::new();
        let mut errors = Vec::new();
        let mut text_buf = String::new();
        let mut rec: Option<Record> = None;
        let mut kind: Option<String> = None;

        // Close the open record: a descriptor sets the kind of what follows.
        let finish = |rec: &mut Option<Record>, kind: &mut Option<String>, chunks: &mut Vec<Chunk>| {
            let Some(mut r) = rec.take() else { return };
            if r.is_descriptor() {
                *kind = r.get("%rec").and_then(|v| v.split_whitespace().next()).map(str::to_string);
            }
            r.kind = kind.clone();
            chunks.push(Chunk::Record(r));
        };

        for (i, line) in text.split_inclusive('\n').enumerate() {
            let no = i + 1;
            let body = line.trim_end_matches(['\n', '\r']);

            // A blank line ends a record.
            if body.trim().is_empty() {
                finish(&mut rec, &mut kind, &mut chunks);
                text_buf.push_str(line);
                continue;
            }

            if body.starts_with('#') {
                match rec.as_mut() {
                    Some(r) => {
                        r.items.push(Item::Comment(body.to_string()));
                        r.raw.push_str(line);
                    }
                    None => text_buf.push_str(line),
                }
                continue;
            }

            // `+ more` continues the previous field's value on a new line.
            if let Some(rest) = body.strip_prefix('+') {
                let field = rec.as_mut().and_then(|r| {
                    r.raw.push_str(line);
                    r.items.iter_mut().rev().find_map(|item| match item {
                        Item::Field(f) => Some(f),
                        Item::Comment(_) => None,
                    })
                });
                match field {
                    Some(f) => {
                        f.value.push('\n');
                        f.value.push_str(rest.strip_prefix(' ').unwrap_or(rest));
                    }
                    None => errors.push(ParseError {
                        line: no,
                        message: "a `+` line must follow a field".into(),
                    }),
                }
                continue;
            }

            let Some((name, value)) = body.split_once(':').filter(|(name, _)| is_name(name)) else {
                errors.push(ParseError {
                    line: no,
                    message: format!("not a field: {body:?} - fix: write `Name: value`, `+ ` to continue, or `#` for a comment"),
                });
                continue;
            };

            // The first field opens a record; text read so far goes before it.
            let r = rec.get_or_insert_with(|| {
                if !text_buf.is_empty() {
                    chunks.push(Chunk::Text(std::mem::take(&mut text_buf)));
                }
                Record {
                    kind: None,
                    line: no,
                    items: Vec::new(),
                    raw: String::new(),
                    dirty: false,
                }
            });
            r.items.push(Item::Field(Field {
                name: name.to_string(),
                value: value.trim().to_string(),
                line: no,
            }));
            r.raw.push_str(line);
        }

        finish(&mut rec, &mut kind, &mut chunks);
        if !text_buf.is_empty() {
            chunks.push(Chunk::Text(text_buf));
        }
        if !errors.is_empty() {
            return Err(errors);
        }
        Ok(Doc { chunks })
    }

    /// The whole file, as it would be written.
    pub fn text(&self) -> String {
        self.chunks
            .iter()
            .map(|c| match c {
                Chunk::Text(t) => t.clone(),
                Chunk::Record(r) => r.render(),
            })
            .collect()
    }

    /// Every record, descriptors included, in file order. An edit takes the
    /// index into this list.
    pub fn records(&self) -> Vec<&Record> {
        self.chunks
            .iter()
            .filter_map(|c| match c {
                Chunk::Record(r) => Some(r),
                Chunk::Text(_) => None,
            })
            .collect()
    }

    fn at(&self, n: usize) -> Option<usize> {
        self.chunks
            .iter()
            .enumerate()
            .filter(|(_, c)| matches!(c, Chunk::Record(_)))
            .nth(n)
            .map(|(i, _)| i)
    }

    fn record_mut(&mut self, n: usize) -> Option<&mut Record> {
        let i = self.at(n)?;
        match &mut self.chunks[i] {
            Chunk::Record(r) => Some(r),
            Chunk::Text(_) => None,
        }
    }

    /// Set a field to one value: replace the first, drop any repeats, or
    /// append it when the record has none.
    pub fn set(&mut self, n: usize, name: &str, value: &str) -> Option<()> {
        let r = self.record_mut(n)?;
        let mut seen = false;
        r.items.retain_mut(|item| {
            let Item::Field(f) = item else { return true };
            if f.name != name {
                return true;
            }
            if seen {
                return false;
            }
            seen = true;
            f.value = value.to_string();
            true
        });
        if !seen {
            r.items.push(Item::Field(Field {
                name: name.to_string(),
                value: value.to_string(),
                line: 0,
            }));
        }
        r.dirty = true;
        Some(())
    }

    /// Add one more value: after the last field of that name, else at the end.
    pub fn add(&mut self, n: usize, name: &str, value: &str) -> Option<()> {
        let r = self.record_mut(n)?;
        let field = Item::Field(Field {
            name: name.to_string(),
            value: value.to_string(),
            line: 0,
        });
        let last = r.items.iter().rposition(|item| matches!(item, Item::Field(f) if f.name == name));
        match last {
            Some(i) => r.items.insert(i + 1, field),
            None => r.items.push(field),
        }
        r.dirty = true;
        Some(())
    }

    /// Drop every field of that name with that value (any value when None).
    /// Returns how many went.
    pub fn remove(&mut self, n: usize, name: &str, value: Option<&str>) -> Option<usize> {
        let r = self.record_mut(n)?;
        let before = r.items.len();
        r.items
            .retain(|item| !matches!(item, Item::Field(f) if f.name == name && value.is_none_or(|v| f.value == v)));
        let gone = before - r.items.len();
        if gone > 0 {
            r.dirty = true;
        }
        Some(gone)
    }

    /// Insert a record of `kind`: after the last record of that kind, else
    /// after its descriptor, else at the end. Returns its index in records().
    pub fn insert(&mut self, kind: &str, fields: &[(&str, &str)]) -> usize {
        let items = fields
            .iter()
            .map(|(name, value)| {
                Item::Field(Field {
                    name: name.to_string(),
                    value: value.to_string(),
                    line: 0,
                })
            })
            .collect();
        let rec = Record {
            kind: Some(kind.to_string()),
            line: 0,
            items,
            raw: String::new(),
            dirty: true,
        };

        let after = self
            .chunks
            .iter()
            .rposition(|c| matches!(c, Chunk::Record(r) if r.kind.as_deref() == Some(kind)));
        let at = after.map_or(self.chunks.len(), |i| i + 1);

        // A blank line goes between the new record and the one before it.
        let ends = self.chunks[..at].last().map(|c| match c {
            Chunk::Text(t) => t.clone(),
            Chunk::Record(r) => r.render(),
        });
        let gap = match ends {
            None => "",
            Some(t) if t.ends_with('\n') => "\n",
            Some(_) => "\n\n",
        };
        self.chunks.insert(at, Chunk::Record(rec));
        if !gap.is_empty() {
            self.chunks.insert(at, Chunk::Text(gap.to_string()));
        }
        self.chunks[..at].iter().filter(|c| matches!(c, Chunk::Record(_))).count()
    }

    /// Delete a record and one blank line next to it, keeping comments.
    pub fn delete(&mut self, n: usize) -> Option<()> {
        let i = self.at(n)?;
        self.chunks.remove(i);
        let blank = |c: &Chunk| matches!(c, Chunk::Text(t) if t.trim().is_empty());
        if i < self.chunks.len() && blank(&self.chunks[i]) {
            self.chunks.remove(i);
            return Some(());
        }
        if i > 0 && blank(&self.chunks[i - 1]) {
            self.chunks.remove(i - 1);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: &str = "# Tools\n\n%rec: Node\n%key: Name\n\nName: Bash\nLevel: 1\n\n# shells above, languages below\n\nName: Python\nLevel:2\nNote: one\n+ two\n# a comment inside\nTier: base\n";

    #[test]
    fn round_trip_is_byte_for_byte() {
        let doc = Doc::parse(TEXT).unwrap();
        assert_eq!(doc.text(), TEXT);
    }

    #[test]
    fn parse_reads_kinds_values_and_lines() {
        let doc = Doc::parse(TEXT).unwrap();
        let recs = doc.records();
        assert_eq!(recs.len(), 3);
        assert!(recs[0].is_descriptor());
        assert_eq!(recs[1].kind.as_deref(), Some("Node"));
        assert_eq!(recs[2].get("Note"), Some("one\ntwo"));
        assert_eq!(recs[2].get("Level"), Some("2"));
        assert_eq!(recs[2].line, 11);
    }

    #[test]
    fn edit_rewrites_only_that_record() {
        let mut doc = Doc::parse(TEXT).unwrap();
        doc.set(2, "Level", "3").unwrap();
        let want = TEXT.replace("Level:2\n", "Level: 3\n");
        assert_eq!(doc.text(), want, "the comment inside stays, other records untouched");
    }

    #[test]
    fn add_remove_insert_delete() {
        let mut doc = Doc::parse(TEXT).unwrap();
        doc.add(1, "Requires", "Computer").unwrap();
        assert_eq!(doc.records()[1].get("Requires"), Some("Computer"));
        assert_eq!(doc.remove(1, "Requires", Some("Computer")), Some(1));
        assert_eq!(doc.records()[1].get("Requires"), None);

        let n = doc.insert("Node", &[("Name", "Rust"), ("Level", "4")]);
        assert_eq!(doc.records()[n].get("Name"), Some("Rust"));
        assert!(doc.text().ends_with("Tier: base\n\nName: Rust\nLevel: 4\n"));

        doc.delete(n).unwrap();
        doc.delete(1).unwrap();
        assert_eq!(doc.records().len(), 2);
        assert!(
            doc.text().contains("# shells above, languages below"),
            "deleting a record keeps comments"
        );
    }

    #[test]
    fn bad_lines_are_reported() {
        let errors = Doc::parse("Name: a\nnot a field\n+ ok\n\n+ orphan\n").unwrap_err();
        assert_eq!(errors.iter().map(|e| e.line).collect::<Vec<_>>(), vec![2, 5]);
    }
}
