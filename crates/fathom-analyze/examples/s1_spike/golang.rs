//! Spike `s1` — the throwaway Go extractor.

use crate::common::{dir_of, field, kids, qualify, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

pub fn language() -> Language {
    tree_sitter_go::LANGUAGE.into()
}

fn name_of(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).trim_matches('"').to_string())
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Scan) {
    let _ = file;
    for kid in kids(&root) {
        match kid.kind() {
            "package_clause" => {
                if let Some(name) = field(&kid, "name").map(|n| text(&n, src).to_string()) {
                    out.aux.insert("package".into(), name);
                }
            }
            "import_declaration" => {
                for spec in descendants(&kid, "import_spec") {
                    let path = field(&spec, "path")
                        .map(|p| text(&p, src).trim_matches('"').to_string())
                        .unwrap_or_default();
                    let alias = field(&spec, "name")
                        .map(|n| text(&n, src).to_string())
                        .unwrap_or_default();
                    let form = match alias.as_str() {
                        "_" => "blank_import",
                        "." => "dot_import",
                        _ => "package_import",
                    };
                    out.imports.push(RawImport::new(path, "*", form));
                }
            }
            "const_declaration" => {
                for spec in descendants(&kid, "const_spec") {
                    for name in names(&spec, src) {
                        push(out, "const", &name);
                    }
                }
            }
            "var_declaration" => {
                for spec in descendants(&kid, "var_spec") {
                    for name in names(&spec, src) {
                        push(out, "var", &name);
                    }
                }
            }
            "type_declaration" => {
                for spec in descendants(&kid, "type_spec") {
                    let Some(name) = name_of(&spec, src) else {
                        continue;
                    };
                    let ty = field(&spec, "type");
                    let kind = match ty.map(|t| t.kind()).unwrap_or("") {
                        "struct_type" => "struct",
                        "interface_type" => "interface",
                        _ => "type",
                    };
                    push(out, kind, &name);
                    if let Some(body) = ty.filter(|t| t.kind() == "interface_type") {
                        for elem in kids(&body) {
                            match elem.kind() {
                                "method_elem" | "method_spec" => {
                                    if let Some(m) = name_of(&elem, src) {
                                        out.defs.push(Def::new("method", qualify(&name, &m)));
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
            "function_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    push(out, "fn", &name);
                }
            }
            "method_declaration" => {
                let Some(name) = name_of(&kid, src) else {
                    continue;
                };
                let recv = field(&kid, "receiver").map(|r| text(&r, src)).unwrap_or("");
                let ty = receiver_type(recv);
                out.defs.push(Def::new("method", qualify(&ty, &name)));
            }
            _ => {}
        }
    }
}

fn push(out: &mut Scan, kind: &str, name: &str) {
    out.defs.push(Def::new(kind, name));
    out.aux.insert(format!("top:{name}"), "1".into());
}

/// `func (r *Rect) Scale(..)` and `func (Rect) Area()` -> `Rect`.
fn receiver_type(recv: &str) -> String {
    let trimmed = recv
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim()
        .to_string();
    let tail = trimmed.rsplit([' ', ',']).next().unwrap_or("").trim();
    let tail = tail.trim_start_matches('*').trim();
    if tail.is_empty() {
        return String::new();
    }
    let base = tail.split(['[', '.']).next().unwrap_or(tail);
    base.to_string()
}

fn names(spec: &Node, src: &[u8]) -> Vec<String> {
    let Some(name) = spec.child_by_field_name("name") else {
        return Vec::new();
    };
    match name.kind() {
        "field_identifier" | "identifier" => vec![text(&name, src).to_string()],
        _ => kids(&name)
            .into_iter()
            .filter(|k| k.kind() == "field_identifier" || k.kind() == "identifier")
            .map(|k| text(&k, src).to_string())
            .collect(),
    }
}

fn descendants<'a>(node: &Node<'a>, kind: &str) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    let mut stack = vec![*node];
    while let Some(cur) = stack.pop() {
        for kid in kids(&cur) {
            if kid.kind() == kind {
                out.push(kid);
            } else {
                stack.push(kid);
            }
        }
    }
    out.reverse();
    out
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let module = idx.extra.get("module").cloned().unwrap_or_default();
    let mut out = BTreeSet::new();
    for imp in imports {
        let dir = if imp.spec == module {
            String::new()
        } else if !module.is_empty() && imp.spec.starts_with(&format!("{module}/")) {
            imp.spec[module.len() + 1..].to_string()
        } else {
            idx.extra
                .insert(format!("unresolved:{file}"), imp.spec.clone());
            continue;
        };
        // A Go import names a package, so it fans out to every non-test file of the directory.
        let mut found = false;
        for candidate in idx
            .files
            .iter()
            .filter(|f| f.ends_with(".go") && !f.ends_with("_test.go") && dir_of(f) == dir)
        {
            out.insert(Edge::new(candidate.clone(), "*"));
            found = true;
        }
        if !found {
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
            if def.name.contains('.') {
                continue;
            }
            // Go's export marker is the first rune of the name, which is a name property and
            // not a declaration marker — there is nothing for a parser to read.
            if def.name.starts_with(char::is_uppercase) {
                names.insert(def.name.clone());
            }
        }
        out.insert(file.clone(), names);
    }
    out
}
