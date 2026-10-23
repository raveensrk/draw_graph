//! The schema: what each `%rec` descriptor says about its records.
//!
//!   %rec: Node
//!   %key: Name
//!   %type: Level range 1 5        -> Type::Range(1, 5)
//!   %type: Tier enum base extra   -> Type::Enum(["base", "extra"])
//!   %mandatory: Name Kind
//!   %allowed: Name Kind Level Tier
//!   %graph: color Level           -> kept as words for graph.rs
//!
//! draw_graph checks these itself rather than calling recfix, so it needs
//! no recutils install (which Windows lacks). A descriptor field it cannot
//! check is a problem, never skipped in silence.

use std::collections::HashMap;

use regex::Regex;

use crate::check::Problem;
use crate::rec::{Doc, Record};

#[derive(Clone, Debug)]
pub enum Type {
    Int,
    Real,
    Bool,
    Line,
    Range(i64, i64),
    Regexp(Regex),
    /// Values in listed order: the order `rise` compares by.
    Enum(Vec<String>),
    /// A foreign key: the value names a record of that type.
    Rec(String),
}

impl Type {
    /// Why a value does not fit, if it does not.
    pub fn misfit(&self, value: &str) -> Option<String> {
        let fits = match self {
            Type::Int => value.parse::<i64>().is_ok(),
            Type::Real => value.parse::<f64>().is_ok(),
            Type::Bool => matches!(value, "yes" | "no" | "true" | "false" | "1" | "0"),
            Type::Line => !value.contains('\n'),
            Type::Range(lo, hi) => value.parse::<i64>().is_ok_and(|v| (*lo..=*hi).contains(&v)),
            Type::Regexp(re) => re.is_match(value),
            Type::Enum(values) => values.iter().any(|v| v == value),
            // Foreign keys are checked against the records, in check.rs.
            Type::Rec(_) => true,
        };
        if fits {
            return None;
        }
        Some(match self {
            Type::Int => format!("{value:?} is not a whole number"),
            Type::Real => format!("{value:?} is not a number"),
            Type::Bool => format!("{value:?} is not yes, no, true, false, 1 or 0"),
            Type::Line => "the value must be one line".to_string(),
            Type::Range(lo, hi) => format!("{value:?} is not a whole number from {lo} to {hi}"),
            Type::Regexp(re) => format!("{value:?} does not match /{}/", re.as_str()),
            Type::Enum(values) => format!("{value:?} is not one of {}", values.join(" ")),
            Type::Rec(_) => unreachable!("a Rec value always fits here"),
        })
    }

    /// Where a value sits in the type's order, for `rise`. None when the
    /// type has no order or the value is not of the type.
    pub fn rank(&self, value: &str) -> Option<f64> {
        if self.misfit(value).is_some() {
            return None;
        }
        match self {
            Type::Int | Type::Range(..) => value.parse::<i64>().ok().map(|v| v as f64),
            Type::Real => value.parse::<f64>().ok(),
            Type::Enum(values) => values.iter().position(|v| v == value).map(|i| i as f64),
            _ => None,
        }
    }
}

/// One `%graph:` line: its words, and where it is.
#[derive(Clone, Debug)]
pub struct Setting {
    pub words: Vec<String>,
    pub line: usize,
}

#[derive(Clone, Debug, Default)]
pub struct RecType {
    pub name: String,
    pub line: usize,
    pub key: Option<String>,
    pub types: HashMap<String, Type>,
    pub mandatory: Vec<String>,
    /// None: any field is allowed.
    pub allowed: Option<Vec<String>>,
    pub prohibit: Vec<String>,
    pub graph: Vec<Setting>,
}

impl RecType {
    /// Whether a field may appear: listed in %allowed, or in %key or
    /// %mandatory, which allow it too.
    pub fn allows(&self, field: &str) -> bool {
        let Some(allowed) = &self.allowed else {
            return !self.prohibit.iter().any(|p| p == field);
        };
        allowed.iter().any(|a| a == field) || self.key.as_deref() == Some(field) || self.mandatory.iter().any(|m| m == field)
    }
}

#[derive(Clone, Debug, Default)]
pub struct Schema {
    pub types: Vec<RecType>,
}

impl Schema {
    pub fn get(&self, name: &str) -> Option<&RecType> {
        self.types.iter().find(|t| t.name == name)
    }

    /// Read every descriptor in the document; problems go to `out`.
    pub fn read(doc: &Doc, out: &mut Vec<Problem>) -> Schema {
        let mut schema = Schema::default();
        for rec in doc.records().into_iter().filter(|r| r.is_descriptor()) {
            let t = read_type(rec, out);
            if schema.get(&t.name).is_some() {
                out.push(Problem::new(
                    rec.line,
                    format!("%rec {}", t.name),
                    "a second descriptor for this type",
                    Some("merge the two descriptors"),
                ));
                continue;
            }
            schema.types.push(t);
        }
        schema
    }
}

fn read_type(rec: &Record, out: &mut Vec<Problem>) -> RecType {
    let name = rec
        .get("%rec")
        .and_then(|v| v.split_whitespace().next())
        .unwrap_or_default()
        .to_string();
    let subject = format!("%rec {name}");
    let mut t = RecType {
        name: name.clone(),
        line: rec.line,
        ..RecType::default()
    };
    let mut typedefs: HashMap<String, Type> = HashMap::new();

    let words = |v: &str| v.split_whitespace().map(str::to_string).collect::<Vec<_>>();

    for f in rec.fields() {
        let mut problem = |message: String, fix: &str| out.push(Problem::new(f.line, subject.clone(), &message, Some(fix)));
        match f.name.as_str() {
            "%rec" | "%doc" | "%sort" => {}
            "%key" => t.key = words(&f.value).into_iter().next(),
            "%mandatory" => t.mandatory.extend(words(&f.value)),
            "%allowed" => t.allowed.get_or_insert_with(Vec::new).extend(words(&f.value)),
            "%prohibit" => t.prohibit.extend(words(&f.value)),
            "%graph" => t.graph.push(Setting {
                words: words(&f.value),
                line: f.line,
            }),
            "%typedef" => {
                let Some((alias, spec)) = f.value.trim().split_once(char::is_whitespace) else {
                    problem(format!("%typedef {:?} has no type", f.value), "write `%typedef: Name TYPE`");
                    continue;
                };
                match parse_type(spec.trim(), &typedefs) {
                    Ok(ty) => {
                        typedefs.insert(alias.to_string(), ty);
                    }
                    Err(message) => problem(message, "use int, real, bool, line, range, regexp, enum or rec"),
                }
            }
            "%type" => {
                let Some((names, spec)) = f.value.trim().split_once(char::is_whitespace) else {
                    problem(format!("%type {:?} has no type", f.value), "write `%type: Field TYPE`");
                    continue;
                };
                match parse_type(spec.trim(), &typedefs) {
                    Ok(ty) => {
                        for field in names.split(',') {
                            t.types.insert(field.to_string(), ty.clone());
                        }
                    }
                    Err(message) => problem(message, "use int, real, bool, line, range, regexp, enum or rec"),
                }
            }
            other => problem(
                format!("draw_graph does not check {other}"),
                "remove it, or ask for draw_graph to support it",
            ),
        }
    }
    t
}

/// `range 1 5`, `enum a b c`, `regexp /^x$/`, `rec Node`, or a %typedef name.
fn parse_type(spec: &str, typedefs: &HashMap<String, Type>) -> Result<Type, String> {
    let (head, rest) = spec.split_once(char::is_whitespace).unwrap_or((spec, ""));
    let rest = rest.trim();
    match head {
        "int" => Ok(Type::Int),
        "real" => Ok(Type::Real),
        "bool" => Ok(Type::Bool),
        "line" => Ok(Type::Line),
        "range" => {
            let nums: Result<Vec<i64>, _> = rest.split_whitespace().map(str::parse).collect();
            match nums.as_deref() {
                Ok([hi]) => Ok(Type::Range(0, *hi)),
                Ok([lo, hi]) if lo <= hi => Ok(Type::Range(*lo, *hi)),
                _ => Err(format!("bad range {rest:?}: want `range MAX` or `range MIN MAX`")),
            }
        }
        "enum" => {
            // recutils allows (comments) inside an enum's value list.
            let plain = Regex::new(r"\([^)]*\)").expect("static regex").replace_all(rest, " ");
            let values: Vec<String> = plain.split_whitespace().map(str::to_string).collect();
            if values.is_empty() {
                return Err("enum lists no values".to_string());
            }
            Ok(Type::Enum(values))
        }
        "regexp" => {
            // The first character is the delimiter: /^x$/ or |^x$|.
            let mut chars = rest.chars();
            let Some(delim) = chars.next() else {
                return Err("regexp has no pattern".to_string());
            };
            let body = chars.as_str();
            let Some(end) = body.rfind(delim) else {
                return Err(format!("regexp {rest:?} is not closed with {delim}"));
            };
            Regex::new(&body[..end])
                .map(Type::Regexp)
                .map_err(|e| format!("bad regexp {rest:?}: {e}"))
        }
        "rec" if !rest.is_empty() => Ok(Type::Rec(rest.split_whitespace().next().unwrap_or_default().to_string())),
        name if rest.is_empty() && typedefs.contains_key(name) => Ok(typedefs[name].clone()),
        other => Err(format!("draw_graph does not check the type {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn types() -> HashMap<String, Type> {
        HashMap::new()
    }

    #[test]
    fn types_parse_and_check() {
        let range = parse_type("range 1 5", &types()).unwrap();
        assert!(range.misfit("3").is_none());
        assert!(range.misfit("6").is_some());

        let tier = parse_type("enum base (core kit) extra max", &types()).unwrap();
        assert!(tier.misfit("extra").is_none());
        assert_eq!(tier.rank("max"), Some(2.0), "comments in parens are not values");

        let pos = parse_type("regexp /^-?[0-9]+ -?[0-9]+$/", &types()).unwrap();
        assert!(pos.misfit("12 -40").is_none());
        assert!(pos.misfit("12,40").is_some());

        assert!(parse_type("date", &types()).is_err(), "an unchecked type is a problem, not a pass");
    }

    #[test]
    fn typedef_names_a_type() {
        let mut defs = types();
        defs.insert("Level_t".to_string(), Type::Range(1, 5));
        assert!(matches!(parse_type("Level_t", &defs), Ok(Type::Range(1, 5))));
    }
}
