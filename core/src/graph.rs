//! The graph a document draws, and the rules its `%graph` lines declare.
//!
//! Four record types carry the graph; their field names come from the
//! settings, with these defaults:
//!
//!   Group  a box          parent Parent
//!   Node   a node         group Group, pos Pos, label Name
//!   Edge   an arrow       from From, to To, any Any
//!   View   a saved view   Root, Collapse, Edges
//!
//! Edges into one node that share an Any value are OR; the rest are AND.
//! An edge may start or end on a group: the arrow meets its box.

use std::collections::{HashMap, HashSet};

use crate::check::Problem;
use crate::rec::{Doc, Record};
use crate::schema::{RecType, Schema, Type};

pub const NODE: &str = "Node";
pub const EDGE: &str = "Edge";
pub const GROUP: &str = "Group";
pub const VIEW: &str = "View";

const SHAPES: &[&str] = &["box", "round", "diamond", "ellipse"];
const STYLES: &[&str] = &["solid", "dashed", "dotted"];

/// `when Kind=requires`: the edges a rule applies to. None: every edge.
#[derive(Clone, Debug, Default)]
pub struct Cond(Option<(String, String)>);

impl Cond {
    pub fn matches(&self, edge: &Record) -> bool {
        self.0.as_ref().is_none_or(|(field, value)| edge.get(field) == Some(value.as_str()))
    }

    fn describe(&self) -> String {
        self.0.as_ref().map(|(f, v)| format!(" ({f} {v})")).unwrap_or_default()
    }
}

#[derive(Clone, Debug)]
pub enum Rule {
    /// No loop among the matching edges.
    Acyclic(Cond),
    /// An edge never points from a higher value to a lower one.
    Rise(Vec<String>, Cond),
    /// No edge already implied through another path.
    NoImplied(Cond),
}

/// What the `%graph` lines say, resolved to field names.
#[derive(Clone, Debug)]
pub struct Settings {
    pub from: String,
    pub to: String,
    pub any: String,
    pub parent: String,
    pub group: String,
    pub pos: String,
    pub label: Vec<String>,
    pub color: Option<String>,
    pub filter: Option<String>,
    /// `%graph: tag Tag`: a repeated field of free labels.
    pub tag: Option<String>,
    pub shape: Option<(String, HashMap<String, String>)>,
    pub style: Option<(String, HashMap<String, String>)>,
    pub rules: Vec<Rule>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            from: "From".into(),
            to: "To".into(),
            any: "Any".into(),
            parent: "Parent".into(),
            group: "Group".into(),
            pos: "Pos".into(),
            label: vec!["Name".into()],
            color: None,
            filter: None,
            tag: None,
            shape: None,
            style: None,
            rules: Vec::new(),
        }
    }
}

impl Settings {
    /// Read every `%graph` line; problems go to `out`.
    pub fn read(schema: &Schema, out: &mut Vec<Problem>) -> Settings {
        let mut s = Settings::default();
        for t in &schema.types {
            for setting in &t.graph {
                let subject = format!("%rec {}", t.name);
                let mut problem = |message: String, fix: &str| out.push(Problem::new(setting.line, subject.clone(), &message, Some(fix)));
                let Some((verb, args)) = setting.words.split_first() else {
                    problem("an empty %graph line".into(), "remove it");
                    continue;
                };

                // Each field a setting names must exist in its record type.
                let unknown: Vec<&String> = field_args(verb, args).into_iter().filter(|f| !t.allows(f)).collect();
                if let Some(field) = unknown.first() {
                    problem(
                        format!("%graph {verb} names {field}, which {} does not allow", t.name),
                        "add it to %allowed, or fix the name",
                    );
                    continue;
                }

                match (t.name.as_str(), verb.as_str(), args) {
                    (GROUP, "parent", [f]) => s.parent = f.clone(),
                    (NODE, "group", [f]) => s.group = f.clone(),
                    (NODE, "pos", [f]) => s.pos = f.clone(),
                    (NODE, "label", fields) if !fields.is_empty() => s.label = fields.to_vec(),
                    (NODE, "color", [f]) => s.color = Some(f.clone()),
                    (NODE, "tag", [f]) => s.tag = Some(f.clone()),
                    (NODE, "filter", [f]) => match t.types.get(f) {
                        Some(Type::Enum(_)) => s.filter = Some(f.clone()),
                        _ => problem(
                            format!("%graph filter {f} needs an enum field"),
                            &format!("add `%type: {f} enum ...`"),
                        ),
                    },
                    (NODE, "shape", [f, pairs @ ..]) => match map(pairs, SHAPES, t.types.get(f)) {
                        Ok(m) => s.shape = Some((f.clone(), m)),
                        Err(message) => problem(message, &format!("shapes are {}", SHAPES.join(" "))),
                    },
                    (EDGE, "from", [f]) => s.from = f.clone(),
                    (EDGE, "to", [f]) => s.to = f.clone(),
                    (EDGE, "any", [f]) => s.any = f.clone(),
                    (EDGE, "style", [f, pairs @ ..]) => match map(pairs, STYLES, t.types.get(f)) {
                        Ok(m) => s.style = Some((f.clone(), m)),
                        Err(message) => problem(message, &format!("styles are {}", STYLES.join(" "))),
                    },
                    (EDGE, "acyclic" | "rise" | "no_implied", rest) => match rule(verb, rest, t, schema.get(NODE)) {
                        Ok(r) => s.rules.push(r),
                        Err(message) => problem(message, "see %graph rules in PLAN.md"),
                    },
                    _ => problem(
                        format!("unknown %graph setting {:?} for {}", setting.words.join(" "), t.name),
                        "see the settings in PLAN.md",
                    ),
                }
            }
        }
        s
    }
}

/// The fields a setting names in its own record type, to check they exist.
/// Rules check their own fields in rule(): rise names Node fields.
fn field_args<'a>(verb: &str, args: &'a [String]) -> Vec<&'a String> {
    match verb {
        "shape" | "style" => args.first().into_iter().collect(),
        "acyclic" | "rise" | "no_implied" => Vec::new(),
        _ => args.iter().collect(),
    }
}

/// `tool=round decision=diamond`: each key a value of the field's enum
/// (when it has one), each value one of `allowed`.
fn map(pairs: &[String], allowed: &[&str], ty: Option<&Type>) -> Result<HashMap<String, String>, String> {
    let mut m = HashMap::new();
    for pair in pairs {
        let Some((k, v)) = pair.split_once('=') else {
            return Err(format!("{pair:?} is not key=value"));
        };
        if !allowed.contains(&v) {
            return Err(format!("{v:?} is not one of {}", allowed.join(" ")));
        }
        if let Some(Type::Enum(values)) = ty
            && !values.iter().any(|x| x == k)
        {
            return Err(format!("{k:?} is not one of {}", values.join(" ")));
        }
        m.insert(k.to_string(), v.to_string());
    }
    Ok(m)
}

/// `acyclic when Kind=requires`, `rise Level Tier when Kind=requires`.
fn rule(verb: &str, args: &[String], edge: &RecType, node: Option<&RecType>) -> Result<Rule, String> {
    let (fields, cond) = match args.iter().position(|a| a == "when") {
        Some(i) => (&args[..i], &args[i + 1..]),
        None => (args, &[][..]),
    };

    let cond = match cond {
        [] => Cond(None),
        [c] => {
            let Some((f, v)) = c.split_once('=') else {
                return Err(format!("when {c:?} is not Field=value"));
            };
            if !edge.allows(f) {
                return Err(format!("when names {f}, which Edge does not allow"));
            }
            if let Some(misfit) = edge.types.get(f).and_then(|t| t.misfit(v)) {
                return Err(format!("when {f}={v}: {misfit}"));
            }
            Cond(Some((f.to_string(), v.to_string())))
        }
        _ => return Err("when takes one Field=value".to_string()),
    };

    match verb {
        "acyclic" if fields.is_empty() => Ok(Rule::Acyclic(cond)),
        "no_implied" if fields.is_empty() => Ok(Rule::NoImplied(cond)),
        "rise" if !fields.is_empty() => {
            let Some(node) = node else {
                return Err("rise needs a Node descriptor".to_string());
            };
            for f in fields {
                let ordered = node.types.get(f).is_some_and(|t| t.rank_order());
                if !ordered {
                    return Err(format!("rise {f}: Node needs `%type: {f}` as int, real, range or enum"));
                }
            }
            Ok(Rule::Rise(fields.to_vec(), cond))
        }
        "rise" => Err("rise names no field".to_string()),
        _ => Err(format!("{verb} takes no fields, only `when Field=value`")),
    }
}

impl Type {
    fn rank_order(&self) -> bool {
        matches!(self, Type::Int | Type::Real | Type::Range(..) | Type::Enum(_))
    }
}

/// The subject a problem names: `Edge Rust -> Tauri`, `Node Bash`.
pub fn subject(rec: &Record, schema: &Schema, s: &Settings) -> String {
    let kind = rec.kind.as_deref().unwrap_or("record");
    if kind == EDGE {
        return format!("Edge {} -> {}", rec.get(&s.from).unwrap_or("?"), rec.get(&s.to).unwrap_or("?"));
    }
    let key = schema.get(kind).and_then(|t| t.key.as_deref()).and_then(|k| rec.get(k));
    match key {
        Some(k) => format!("{kind} {k}"),
        None => format!("{kind} record"),
    }
}

/// Check the graph: names, boxes, OR groups, duplicates, then the rules.
pub fn check(doc: &Doc, schema: &Schema, s: &Settings, out: &mut Vec<Problem>) {
    let recs = doc.records();
    let of = |kind: &str| {
        recs.iter()
            .copied()
            .filter(move |r| !r.is_descriptor() && r.kind.as_deref() == Some(kind))
            .collect::<Vec<_>>()
    };
    let key = |kind: &str| schema.get(kind).and_then(|t| t.key.clone()).unwrap_or_else(|| "Name".to_string());
    let (nodes, edges, groups, views) = (of(NODE), of(EDGE), of(GROUP), of(VIEW));
    let (node_key, group_key) = (key(NODE), key(GROUP));
    let say = |rec: &Record, message: String, fix: &str| Problem::new(rec.line, subject(rec, schema, s), &message, Some(fix));

    let node_names: HashMap<&str, &Record> = nodes.iter().filter_map(|r| Some((r.get(&node_key)?, *r))).collect();
    let group_names: HashMap<&str, &Record> = groups.iter().filter_map(|r| Some((r.get(&group_key)?, *r))).collect();
    let known = |name: &str| node_names.contains_key(name) || group_names.contains_key(name);

    // A field typed `rec X` is checked as a foreign key already; checking
    // it here too would say the same thing twice.
    let typed = |kind: &str, field: &str| matches!(schema.get(kind).and_then(|t| t.types.get(field)), Some(Type::Rec(_)));

    // A name used by a node and a group would make an edge ambiguous.
    for (name, g) in &group_names {
        if node_names.contains_key(name) {
            out.push(say(g, format!("{name:?} names a node too"), "rename the group or the node"));
        }
    }

    // Boxes: a node's group and a group's parent name real groups, and
    // parents never loop.
    for n in nodes.iter().filter(|_| !typed(NODE, &s.group)) {
        if let Some(g) = n.get(&s.group).filter(|g| !group_names.contains_key(g)) {
            out.push(say(
                n,
                format!("{} {g:?} is not a group", s.group),
                "correct it, or add the Group record",
            ));
        }
    }
    for g in &groups {
        let Some(parent) = g.get(&s.parent) else { continue };
        if !group_names.contains_key(parent) {
            if !typed(GROUP, &s.parent) {
                out.push(say(
                    g,
                    format!("{} {parent:?} is not a group", s.parent),
                    "correct it, or add the Group record",
                ));
            }
            continue;
        }

        // Climb the parents; coming back to this group is a loop.
        let me = g.get(&group_key).unwrap_or_default();
        let mut trail = vec![me];
        let mut at = parent;
        loop {
            trail.push(at);

            // Every group on the loop sees it; the first name reports it.
            if at == me {
                if trail.iter().all(|n| me <= *n) {
                    out.push(say(
                        g,
                        format!("{} loops: {}", s.parent, trail.join(" -> ")),
                        "break the loop of Parent fields",
                    ));
                }
                break;
            }
            let Some(up) = group_names.get(at).and_then(|r| r.get(&s.parent)) else {
                break;
            };
            if trail[1..].contains(&up) {
                break; // a loop above this group; its own members report it
            }
            at = up;
        }
    }

    // Edges: both ends name a node or a group.
    for e in &edges {
        for field in [&s.from, &s.to] {
            match e.get(field) {
                None => out.push(say(e, format!("no {field}"), &format!("add `{field}: NAME`"))),
                Some(name) if !known(name) => out.push(say(
                    e,
                    format!("no node or group named {name:?}"),
                    &format!("correct {field}, or add the node"),
                )),
                Some(_) => {}
            }
        }
    }

    // OR groups: edges sharing an Any value, at least two, all into one node.
    let mut any: HashMap<&str, Vec<&Record>> = HashMap::new();
    for e in &edges {
        if let Some(a) = e.get(&s.any) {
            any.entry(a).or_default().push(e);
        }
    }
    for (a, members) in &any {
        let first = members[0];
        if members.len() < 2 {
            out.push(say(
                first,
                format!("{} {a:?} is on one edge only", s.any),
                &format!("add the other choices, or drop {}", s.any),
            ));
        }
        if members.iter().any(|m| m.get(&s.to) != first.get(&s.to)) {
            out.push(say(
                first,
                format!("{} {a:?} joins edges into different nodes", s.any),
                "give each target its own Any value",
            ));
        }
    }

    // The same arrow twice.
    let kind = s.style.as_ref().map(|(f, _)| f.as_str());
    type Arrow<'a> = (Option<&'a str>, Option<&'a str>, Option<&'a str>, Option<&'a str>);
    let mut arrows: HashMap<Arrow, &Record> = HashMap::new();
    for e in &edges {
        let arrow = (e.get(&s.from), e.get(&s.to), kind.and_then(|k| e.get(k)), e.get(&s.any));
        if arrows.insert(arrow, e).is_some() {
            out.push(say(e, "this edge appears twice".into(), "drop one"));
        }
    }

    // Views: their roots and folds name real nodes or groups, their edge
    // kinds real kinds.
    let kinds = kind.and_then(|k| match schema.get(EDGE)?.types.get(k)? {
        Type::Enum(values) => Some(values.clone()),
        _ => None,
    });
    for v in &views {
        let root = v.get("Root").filter(|_| !typed(VIEW, "Root"));
        for name in v.all("Collapse").chain(root) {
            if !known(name) {
                out.push(say(v, format!("no node or group named {name:?}"), "correct the name"));
            }
        }
        let bad = v
            .get("Edges")
            .into_iter()
            .flat_map(str::split_whitespace)
            .find(|k| kinds.as_ref().is_some_and(|ks| !ks.iter().any(|x| x == k)));
        if let Some(k) = bad {
            out.push(say(
                v,
                format!("Edges names {k:?}, not an edge kind"),
                "use a value of the edge kind enum",
            ));
        }
    }

    // Inside a loop every node is upstream of every other, so no_implied
    // would flag each edge on it; with a loop found, only the loop is told.
    let before = out.len();
    for r in &s.rules {
        match r {
            Rule::Acyclic(cond) => acyclic(&edges, cond, s, &say, out),
            Rule::Rise(fields, cond) => rise(&edges, fields, cond, &node_names, schema, s, &say, out),
            Rule::NoImplied(_) => {}
        }
    }
    let looped = out[before..].iter().any(|p| p.message.starts_with("loop"));
    for r in s.rules.iter().filter(|_| !looped) {
        if let Rule::NoImplied(cond) = r {
            no_implied(&edges, cond, s, &say, out);
        }
    }
}

type Say<'a> = dyn Fn(&Record, String, &str) -> Problem + 'a;

/// Each node's sources along the matching edges: node -> [edges into it].
fn into<'a>(edges: &[&'a Record], cond: &Cond, s: &Settings) -> HashMap<&'a str, Vec<&'a Record>> {
    let mut m: HashMap<&str, Vec<&Record>> = HashMap::new();
    for e in edges.iter().filter(|e| cond.matches(e)) {
        if let (Some(_), Some(to)) = (e.get(&s.from), e.get(&s.to)) {
            m.entry(to).or_default().push(e);
        }
    }
    m
}

fn acyclic(edges: &[&Record], cond: &Cond, s: &Settings, say: &Say, out: &mut Vec<Problem>) {
    // Walk out-edges depth first; meeting a node still open is a loop.
    let mut next: HashMap<&str, Vec<&Record>> = HashMap::new();
    for e in edges.iter().filter(|e| cond.matches(e)) {
        if let Some(from) = e.get(&s.from) {
            next.entry(from).or_default().push(e);
        }
    }
    let mut state: HashMap<&str, bool> = HashMap::new(); // false: open, true: done

    fn visit<'a>(
        at: &'a str,
        trail: &mut Vec<&'a str>,
        next: &HashMap<&'a str, Vec<&'a Record>>,
        state: &mut HashMap<&'a str, bool>,
        s: &Settings,
        found: &mut Vec<(Vec<&'a str>, &'a Record)>,
    ) {
        state.insert(at, false);
        trail.push(at);
        for e in next.get(at).into_iter().flatten() {
            let Some(to) = e.get(&s.to) else { continue };
            match state.get(to) {
                Some(false) => {
                    let start = trail.iter().position(|n| *n == to).unwrap_or(0);
                    let mut lp = trail[start..].to_vec();
                    lp.push(to);
                    found.push((lp, e));
                }
                Some(true) => {}
                None => visit(to, trail, next, state, s, found),
            }
        }
        trail.pop();
        state.insert(at, true);
    }

    let mut found = Vec::new();
    let mut starts: Vec<&str> = next.keys().copied().collect();
    starts.sort();
    for at in starts {
        if !state.contains_key(at) {
            visit(at, &mut Vec::new(), &next, &mut state, s, &mut found);
        }
    }
    for (lp, e) in found {
        out.push(say(
            e,
            format!("loop{}: {}", cond.describe(), lp.join(" -> ")),
            "drop one edge of the loop",
        ));
    }
}

#[allow(clippy::too_many_arguments)]
fn rise(
    edges: &[&Record],
    fields: &[String],
    cond: &Cond,
    nodes: &HashMap<&str, &Record>,
    schema: &Schema,
    s: &Settings,
    say: &Say,
    out: &mut Vec<Problem>,
) {
    let Some(node_type) = schema.get(NODE) else { return };
    for e in edges.iter().filter(|e| cond.matches(e)) {
        // An end that is a group, or lacks the field, is skipped.
        let (Some(from), Some(to)) = (e.get(&s.from).and_then(|n| nodes.get(n)), e.get(&s.to).and_then(|n| nodes.get(n))) else {
            continue;
        };
        for f in fields {
            let Some(ty) = node_type.types.get(f) else { continue };
            let (Some(a), Some(b)) = (from.get(f), to.get(f)) else { continue };
            let (Some(ra), Some(rb)) = (ty.rank(a), ty.rank(b)) else { continue };
            if ra > rb {
                let (fname, tname) = (e.get(&s.from).unwrap_or_default(), e.get(&s.to).unwrap_or_default());
                out.push(say(
                    e,
                    format!("{fname} ({f} {a}) is above {tname} ({f} {b})"),
                    &format!("lower {fname}'s {f}, raise {tname}'s, or drop the edge"),
                ));
            }
        }
    }
}

fn no_implied(edges: &[&Record], cond: &Cond, s: &Settings, say: &Say, out: &mut Vec<Problem>) {
    let into = into(edges, cond, s);

    // Everything upstream of a node along the matching edges.
    fn ancestors<'a>(at: &'a str, into: &HashMap<&'a str, Vec<&'a Record>>, s: &Settings, seen: &mut HashSet<&'a str>) {
        for e in into.get(at).into_iter().flatten() {
            if let Some(from) = e.get(&s.from)
                && seen.insert(from)
            {
                ancestors(from, into, s, seen);
            }
        }
    }

    let mut targets: Vec<&&str> = into.keys().collect();
    targets.sort();
    for to in targets {
        let ins = &into[*to];
        let and: Vec<&Record> = ins.iter().copied().filter(|e| e.get(&s.any).is_none()).collect();
        for e in &and {
            let Some(pre) = e.get(&s.from) else { continue };

            // An AND edge reached through another source is implied.
            let via = ins.iter().filter_map(|o| o.get(&s.from)).filter(|o| *o != pre).find(|o| {
                let mut seen = HashSet::new();
                ancestors(o, &into, s, &mut seen);
                seen.contains(pre)
            });
            if let Some(via) = via {
                out.push(say(e, format!("implied through {via}"), "drop this edge"));
            }

            // An OR choice that is also required outright says nothing.
            for o in ins.iter().filter(|o| o.get(&s.any).is_some() && o.get(&s.from) == Some(pre)) {
                out.push(say(
                    o,
                    format!(
                        "{} {:?} is met already: {pre} is required on its own",
                        s.any,
                        o.get(&s.any).unwrap_or_default()
                    ),
                    "drop the OR choice",
                ));
            }
        }
    }
}
