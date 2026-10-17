//! Spike `s1` — the throwaway Ruby extractor.

use crate::common::{dir_of, join_dir, kids, qualify, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

/// The macros that declare a method without a `def`. This list is the whole DSL support of the
/// spike: three of these four lines buys Ruby's most common blind spot, and everything the list
/// does not know about (`define_method` around a loop, `defdelegate`, a plugin's own macro) stays
/// invisible. That trade-off is the finding, not an oversight.
const MACROS: [(&str, MethodKind); 7] = [
    ("attr_reader", MethodKind::Reader),
    ("attr_writer", MethodKind::Writer),
    ("attr_accessor", MethodKind::Accessor),
    ("defdelegate", MethodKind::Reader),
    ("alias_method", MethodKind::Reader),
    ("module_function", MethodKind::Reader),
    // A literal `define_method(:perimeter) { }` is a definition in every sense a parser cares
    // about. The loop that computes names from a constant array is not, and stays in the report.
    ("define_method", MethodKind::Reader),
];

enum MethodKind {
    Reader,
    Writer,
    Accessor,
}

pub fn language() -> Language {
    tree_sitter_ruby::LANGUAGE.into()
}

fn dotted(raw: &str) -> String {
    raw.replace("::", ".")
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Scan) {
    let _ = file;
    out.aux.insert("loadpath".into(), "lib".into());
    body(root, src, "", true, out);
}

fn body(node: Node, src: &[u8], container: &str, top: bool, out: &mut Scan) {
    for kid in kids(&node) {
        match kid.kind() {
            "module" | "class" => {
                let kind = if kid.kind() == "module" {
                    "module"
                } else {
                    "class"
                };
                let Some(raw) = kid
                    .child_by_field_name("name")
                    .map(|n| text(&n, src).to_string())
                else {
                    continue;
                };
                let name = dotted(&raw);
                let stored = if name.contains('.') {
                    // `module Demo::Core` declares `Demo.Core` where it is written, and the
                    // `Demo` part is only a reference.
                    name.clone()
                } else {
                    qualify(container, &name)
                };
                out.defs.push(Def::new(kind, &stored));
                if top {
                    out.aux.insert(format!("top:{stored}"), "1".into());
                }
                if let Some(inner) = kid.child_by_field_name("body") {
                    body(inner, src, &stored, false, out);
                } else {
                    for part in kids(&kid) {
                        if matches!(part.kind(), "body_statement" | "statements") {
                            body(part, src, &stored, false, out);
                        }
                    }
                }
            }
            "method" | "singleton_method" => {
                let Some(raw) = kid
                    .child_by_field_name("name")
                    .map(|n| text(&n, src).to_string())
                else {
                    continue;
                };
                let stored = if kid.kind() == "method" && container.is_empty() {
                    raw
                } else {
                    qualify(container, &raw)
                };
                let kind = if container.is_empty() { "fn" } else { "method" };
                out.defs.push(Def::new(kind, &stored));
                if top {
                    out.aux.insert(format!("top:{stored}"), "1".into());
                }
            }
            "singleton_class" => {
                for part in kids(&kid) {
                    if matches!(part.kind(), "body_statement" | "statements") {
                        body(part, src, container, false, out);
                    }
                }
            }
            "assignment" => {
                assign(kid, src, container, top, out);
            }
            "alias" => {
                let parts: Vec<String> = kids(&kid)
                    .iter()
                    .map(|k| text(k, src).trim_start_matches(':').to_string())
                    .collect();
                if let Some(first) = parts.first() {
                    let stored = qualify(container, first);
                    out.defs.push(Def::new("method", &stored));
                    out.note("alias", &stored);
                }
            }
            "call" => {
                call(kid, src, container, top, out);
            }
            "if" | "unless" | "then" | "else" | "elsif" | "begin" | "case" | "body_statement"
            | "statements" => {
                // Conditional `require`s and `if`-guarded class bodies are ordinary Ruby.
                body(kid, src, container, top, out);
            }
            _ => {}
        }
    }
}

fn assign(node: Node, src: &[u8], container: &str, top: bool, out: &mut Scan) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let right = node
        .child_by_field_name("right")
        .map(|r| text(&r, src))
        .unwrap_or("");
    match left.kind() {
        "constant" | "scope_resolution" | "constant_reference" => {
            let name = dotted(&text(&left, src).replace(' ', ""));
            let stored = if name.contains('.') || container.is_empty() {
                name
            } else {
                qualify(container, &name)
            };
            let kind = if right.contains("Struct.new") || right.contains("Class.new") {
                "class"
            } else {
                "const"
            };
            out.defs.push(Def::new(kind, &stored));
            if top {
                out.aux.insert(format!("top:{stored}"), "1".into());
            }
            if kind == "class" {
                out.note("struct_new", &stored);
                for block in blocks_under(node) {
                    body(block, src, &stored, false, out);
                }
            }
        }
        "attribute" | "element_reference" => {
            // `Shape.pad = ..` / `HANDLERS[:x] = ..`: one of these declares a method on a class
            // named elsewhere, the other is data. The parser cannot tell them apart by shape alone.
            out.note("monkeypatch_or_data", &text(&left, src).replace(' ', ""));
        }
        _ => {}
    }
}

/// Every block under a node, however the grammar nested it.
fn blocks_under<'a>(node: tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut found = Vec::new();
    let mut stack = vec![node];
    while let Some(cur) = stack.pop() {
        for kid in kids(&cur) {
            if matches!(kid.kind(), "do_block" | "block" | "body_statement") {
                found.push(kid);
            }
            stack.push(kid);
        }
    }
    found
}

fn call(node: Node, src: &[u8], container: &str, top: bool, out: &mut Scan) {
    let method = node
        .child_by_field_name("method")
        .map(|m| text(&m, src).to_string())
        .unwrap_or_default();
    let receiver = node
        .child_by_field_name("receiver")
        .map(|r| text(&r, src))
        .unwrap_or("");
    let args: Vec<String> = kids(&node)
        .into_iter()
        .filter(|k| k.kind() == "argument_list")
        .flat_map(|list| kids(&list))
        // Literals only. An identifier argument is a *variable*: treating `define_method(name)`
        // as a definition of `name` invents a method that does not exist, which is the one error a
        // graph cannot tolerate. The dynamic case gets a blind-spot note instead.
        .filter(|k| matches!(k.kind(), "simple_symbol" | "string"))
        .map(|k| {
            text(&k, src)
                .trim_start_matches(':')
                .trim_matches(|c| c == '\'' || c == '"')
                .to_string()
        })
        .collect();

    match method.as_str() {
        "require" | "require_relative" | "load" => {
            let spec = args.first().cloned().unwrap_or_default();
            if spec.is_empty() {
                return;
            }
            let form = if method == "require_relative" {
                "require_relative"
            } else {
                "load_path_require"
            };
            out.imports.push(RawImport::new(spec, "*", form));
        }
        "autoload" => {
            if args.len() >= 2 {
                out.imports
                    .push(RawImport::new(args[1].clone(), "*", "autoload"));
            }
        }
        "class_eval" | "module_eval" if !receiver.is_empty() => {
            let name = dotted(receiver);
            out.note("class_eval", &name);
            for block in kids(&node)
                .into_iter()
                .filter(|k| matches!(k.kind(), "do_block" | "block" | "body_statement"))
            {
                body(block, src, &name, false, out);
            }
        }
        _ => {
            for (macro_name, kind) in MACROS.iter() {
                if method != *macro_name {
                    continue;
                }
                for arg in &args {
                    let stored = qualify(container, arg);
                    out.defs.push(Def::new("method", &stored));
                    out.note("macro_method", &stored);
                    if matches!(kind, MethodKind::Accessor | MethodKind::Writer) {
                        let setter = format!("{stored}=");
                        out.defs.push(Def::new("method", &setter));
                        out.note("macro_method", &setter);
                    }
                }
            }
            // `define_method(n)` where `n` comes from a collection is a definition the parser
            // cannot see. Saying so is the difference between "4 methods missing" and
            // "4 methods missing because their names are computed at runtime".
            if method == "define_method" && args.is_empty() {
                out.note("define_method_dynamic", container);
            }
        }
    }
    for arg in kids(&node) {
        match arg.kind() {
            // `private def helper` and `protected def shield` pass a definition through as an
            // argument; the definition is real even though the call around it is a visibility macro.
            "argument_list" => {
                for inner in kids(&arg) {
                    if matches!(inner.kind(), "method" | "singleton_method") {
                        declare_method(&inner, src, container, out);
                    }
                }
            }
            // `%i[a b].each { |n| define_method(n) { ... } }`: the work happens inside the block,
            // so a walker that stops at the call never sees what the loop is driving. The reopen
            // forms are excluded because they were already walked above, under the container they
            // reopen; walking them twice would report `audit` as a top-level function and as a
            // method of the reopened class, and only one of those is Ruby's meaning.
            "block" | "do_block"
                if !matches!(
                    method.as_str(),
                    "class_eval" | "module_eval" | "instance_eval"
                ) =>
            {
                body(arg, src, container, top, out)
            }
            _ => {}
        }
    }
}

fn declare_method<'a>(node: &tree_sitter::Node<'a>, src: &[u8], container: &str, out: &mut Scan) {
    let Some(raw) = node
        .child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
    else {
        return;
    };
    let stored = if node.kind() == "method" && container.is_empty() {
        raw
    } else {
        qualify(container, &raw)
    };
    let kind = if container.is_empty() { "fn" } else { "method" };
    out.defs.push(Def::new(kind, &stored));
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let mut out = BTreeSet::new();
    let dir = dir_of(file);
    for imp in imports {
        let candidate = if imp.form == "require_relative" {
            join_dir(&dir, &format!("{}.rb", imp.spec))
        } else {
            // A plain `require` resolves against a load path the file does not state. The corpus
            // keeps its root at `lib/`, and an extractor that is not told that cannot resolve a
            // single one of these edges.
            format!("lib/{}.rb", imp.spec)
        };
        if idx.has(&candidate) {
            out.insert(Edge::new(candidate, "*"));
        } else {
            idx.extra
                .insert(format!("unresolved:{file}"), imp.spec.clone());
        }
    }
    out
}

pub fn surface(_file: &str, _scan: &Scan, _edges: &BTreeSet<Edge>, _idx: &mut Index) {}

pub fn same_package(_file: &str, _scan: &Scan, _idx: &Index) -> BTreeSet<Edge> {
    BTreeSet::new()
}

pub fn exports(
    scans: &BTreeMap<String, Scan>,
    _edges: &BTreeMap<String, BTreeSet<Edge>>,
    _idx: &Index,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for (file, scan) in scans {
        let mut names = BTreeSet::new();
        for def in &scan.defs {
            // Ruby has no export declaration at all: a constant defined at the top of a file can
            // be named by anything that has loaded it. The relation is therefore degenerate, and
            // the report says so rather than pretending the number means something.
            if scan.aux.contains_key(&format!("top:{}", def.name)) {
                names.insert(def.name.clone());
            }
        }
        out.insert(file.clone(), names);
    }
    out
}
