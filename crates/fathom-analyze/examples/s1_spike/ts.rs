//! Spike `s1` — the throwaway TypeScript extractor.

use crate::common::{dir_of, is_d_ts, join_dir, kids, qualify, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

pub fn language() -> Language {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Scan) {
    if is_d_ts(file) {
        out.aux.insert("ambient".into(), "1".into());
    }
    statements(root, src, "", out);
}

fn name_of(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
}

/// Named and anonymous children: `export default` and `export *` are marked by tokens the
/// grammar does not name, so a `named_children()` walk cannot see them at all.
fn all_kids<'a>(node: &tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

fn has_token(node: &tree_sitter::Node, kind: &str) -> bool {
    all_kids(node).iter().any(|k| k.kind() == kind)
}

fn statements(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    for kid in kids(&node) {
        match kid.kind() {
            "export_statement" => export(kid, src, container, out),
            "import_statement" | "import_require_clause" => import(kid, src, out),
            _ => declare(kid, src, container, false, false, out),
        }
    }
}

fn import(node: Node, src: &[u8], out: &mut Scan) {
    let source = node
        .child_by_field_name("source")
        .map(|s| literal(&s, src))
        .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for kid in kids(&node) {
        if kid.kind() != "import_clause" {
            continue;
        }
        for part in kids(&kid) {
            match part.kind() {
                // A bare identifier in the clause is the default import, and the name the
                // importer writes has nothing to do with the name it resolves to.
                "identifier" => names.push("default".to_string()),
                "named_imports" => {
                    for spec in kids(&part) {
                        if spec.kind() != "import_specifier" {
                            continue;
                        }
                        let origin = spec
                            .child_by_field_name("name")
                            .map(|n| text(&n, src).to_string())
                            .unwrap_or_default();
                        if !origin.is_empty() {
                            names.push(origin);
                        }
                    }
                }
                "namespace_import" => names.push("*".to_string()),
                _ => {}
            }
        }
    }
    if names.is_empty() {
        // `import './legacy';` — a side-effect dependency, with no symbol to name.
        names.push("*".to_string());
    }
    for name in names {
        out.imports
            .push(RawImport::new(source.clone(), name, "import"));
    }
}

/// One declaration site, reached either bare or through `export`.
fn declare(node: Node, src: &[u8], container: &str, exported: bool, default: bool, out: &mut Scan) {
    let name = match node.kind() {
        "function_declaration" | "generator_function_declaration" => "fn",
        "class_declaration" | "abstract_class_declaration" => "class",
        "interface_declaration" => "interface",
        "type_alias_declaration" => "type",
        "enum_declaration" => "enum",
        _ => "",
    };
    if !name.is_empty() {
        let Some(declared) = name_of(&node, src) else {
            return;
        };
        let stored = if default {
            "default".to_string()
        } else {
            qualify(container, &declared)
        };
        out.defs.push(Def::new(name, &stored));
        if container.is_empty() {
            out.aux.insert(format!("top:{stored}"), "1".into());
        }
        if default {
            out.aux.insert("has_default".into(), declared);
        }
        if (exported || default) && container.is_empty() {
            mark_export(out, &stored, default);
        }
        if (name == "class" || name == "interface")
            && let Some(body) = node.child_by_field_name("body")
        {
            members(body, src, &stored, out);
        }
        return;
    }
    match node.kind() {
        "ambient_declaration" => {
            // `declare function loadConfig(path: string): Config;` in a `.d.ts`: the file is a
            // stub by definition, and the stub is still the package's only declaration of it.
            for inner in kids(&node) {
                declare(inner, src, container, exported, default, out);
            }
        }
        "function_signature" => {
            // `export declare function loadConfig(path: string): Config;` in a `.d.ts` is the
            // only declaration of that symbol the package has.
            let Some(declared) = name_of(&node, src) else {
                return;
            };
            let stored = if default {
                "default".to_string()
            } else {
                qualify(container, &declared)
            };
            out.defs.push(Def::new("fn", &stored));
            if container.is_empty() {
                out.aux.insert(format!("top:{stored}"), "1".into());
                if exported || default {
                    mark_export(out, &stored, default);
                }
            }
        }
        "lexical_declaration" | "variable_declaration" => {
            for declarator in kids(&node) {
                if declarator.kind() != "variable_declarator" {
                    continue;
                }
                let Some(ident) = name_of(&declarator, src) else {
                    continue;
                };
                let value = declarator.child_by_field_name("value");
                let vt = value.map(|v| v.kind()).unwrap_or("");
                let kind = if matches!(
                    vt,
                    "arrow_function"
                        | "function_expression"
                        | "function"
                        | "generator_function"
                        | "generator_function_declaration"
                ) {
                    "fn"
                } else if node.kind() == "lexical_declaration"
                    && text(&node, src).starts_with("const")
                {
                    "const"
                } else {
                    "var"
                };
                let stored = qualify(container, &ident);
                out.defs.push(Def::new(kind, &stored));
                if container.is_empty() {
                    out.aux.insert(format!("top:{stored}"), "1".into());
                    if exported {
                        mark_export(out, &stored, false);
                    }
                }
            }
        }
        "module" | "internal_module" | "namespace_declaration" => {
            let Some(declared) = name_of(&node, src) else {
                out.note(
                    "ambient_module",
                    text(&node, src).split('{').next().unwrap_or("?"),
                );
                return;
            };
            let stored = qualify(container, &declared);
            out.defs.push(Def::new("module", &stored));
            if container.is_empty() {
                out.aux.insert(format!("top:{stored}"), "1".into());
                if exported {
                    mark_export(out, &stored, false);
                }
            }
            if let Some(body) = node.child_by_field_name("body") {
                statements(body, src, &stored, out);
            }
        }
        "expression_statement" => {
            for kid in kids(&node) {
                if kid.kind() == "call_expression" {
                    // `@Register(...)`-style decorators are decorators, not definitions; a bare
                    // `register(fn)` call that creates an export is invisible, and is reported.
                    out.note(
                        "side_effect_call",
                        text(&kid, src).split('(').next().unwrap_or("?"),
                    );
                }
            }
        }
        _ => {}
    }
}

fn mark_export(out: &mut Scan, name: &str, default: bool) {
    if default {
        out.aux.insert("export:default".into(), "1".into());
    } else {
        out.aux.insert(format!("export:{name}"), "1".into());
    }
}

fn members(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    for kid in kids(&node) {
        let kind = match kid.kind() {
            "method_definition" | "method_signature" | "abstract_method_signature" => "method",
            _ => continue,
        };
        let Some(name) = name_of(&kid, src) else {
            continue;
        };
        if name == "constructor" {
            continue;
        }
        out.defs.push(Def::new(kind, qualify(container, &name)));
    }
}

fn export(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    let is_default = has_token(&node, "default");
    let source = node.child_by_field_name("source").map(|s| literal(&s, src));
    let mut names: Vec<(String, String)> = Vec::new(); // (source name, exported name)
    let mut wildcard = false;
    for kid in all_kids(&node) {
        match kid.kind() {
            "declaration" => {}
            "export_clause" => {
                for spec in kids(&kid) {
                    if spec.kind() != "export_specifier" {
                        continue;
                    }
                    let local = spec
                        .child_by_field_name("name")
                        .map(|n| text(&n, src).to_string())
                        .unwrap_or_default();
                    let exported = spec
                        .child_by_field_name("alias")
                        .map(|n| text(&n, src).to_string())
                        .unwrap_or_else(|| local.clone());
                    names.push((local, exported));
                }
            }
            "namespace_export" => wildcard = true,
            "*" => wildcard = true,
            "type" | "default" => {}
            _ => {}
        }
    }
    if let Some(decl) = node.child_by_field_name("declaration") {
        declare(decl, src, container, true, is_default, out);
        return;
    }
    if is_default && node.child_by_field_name("declaration").is_none() {
        // `export default Point;` sells an existing symbol under the name `default`; the
        // declaring file keeps the definition and this file owns only the exported name.
        out.aux.insert("export:default".into(), "1".into());
        out.aux.insert("default_is_local".into(), "1".into());
        return;
    }
    match (source, wildcard) {
        (Some(spec), true) => out.imports.push(RawImport::new(spec, "*", "reexport_star")),
        (Some(spec), false) => {
            for (local, exported) in names {
                let mut imp = RawImport::new(spec.clone(), local, "reexport");
                imp.reexport = true;
                out.aux
                    .insert(format!("renamed:{exported}"), imp.symbol.clone());
                out.imports.push(imp);
            }
        }
        (None, false) => {
            for (local, exported) in names {
                out.aux.insert(format!("renamed:{exported}"), local);
                mark_export(out, &exported, false);
            }
        }
        (None, true) => {}
    }
}

fn literal(node: &Node, src: &[u8]) -> String {
    text(node, src)
        .trim_matches(|c| c == '\'' || c == '"' || c == '`')
        .to_string()
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let mut out = BTreeSet::new();
    let dir = dir_of(file);
    for imp in imports {
        let spec = imp.spec.clone();
        let Some(target) = resolve_relative(idx, &dir, &spec) else {
            idx.extra.insert(format!("unresolved:{file}"), spec.clone());
            continue;
        };
        if imp.symbol == "*" {
            out.insert(Edge::new(target, "*"));
            continue;
        }
        match follow(idx, &target, &imp.symbol, &mut Vec::new()) {
            Some(edge) => {
                out.insert(edge);
            }
            None => {
                idx.extra.insert(
                    format!("unresolved:{file}"),
                    format!("{spec}:{}", imp.symbol),
                );
            }
        }
    }
    out
}

fn follow(idx: &Index, file: &str, symbol: &str, seen: &mut Vec<String>) -> Option<Edge> {
    if symbol == "*" || idx.defines(file, symbol) {
        return Some(Edge::new(file, symbol));
    }
    let tag = format!("{file}\u{1}{symbol}");
    if seen.contains(&tag) {
        return None;
    }
    seen.push(tag.clone());
    let origin = idx.extra.get(&tag)?;
    let (ofile, osym) = origin.split_once('\u{1}')?;
    follow(idx, ofile, osym, seen)
}

/// `./geo`, `../shapes`, `./types` -> a file in the corpus. Bare specifiers are dependencies.
fn resolve_relative(idx: &Index, dir: &str, spec: &str) -> Option<String> {
    if !spec.starts_with('.') {
        return None;
    }
    let clean = spec.trim_start_matches('.').trim_start_matches('/');
    let base = if spec.starts_with("..") {
        dir_of(dir)
    } else {
        dir.to_string()
    };
    let root = if clean.is_empty() { "" } else { clean };
    let mut candidates = vec![
        join_dir(&base, &format!("{root}.ts")),
        join_dir(&base, &format!("{root}.d.ts")),
        join_dir(&base, &format!("{root}/index.ts")),
        join_dir(&base, &format!("{root}/index.d.ts")),
    ];
    if root.is_empty() {
        candidates.push(join_dir(&base, "index.ts"));
    }
    candidates.into_iter().find(|c| idx.has(c))
}

/// What each file makes available to an importer, following re-export chains.
pub fn surface(file: &str, scan: &Scan, edges: &BTreeSet<Edge>, idx: &mut Index) {
    for key in scan.aux.keys().filter(|k| k.starts_with("export:")) {
        let name = key.trim_start_matches("export:");
        idx.extra
            .insert(format!("{file}\u{1}{name}"), format!("{file}\u{1}{name}"));
    }
    for edge in edges {
        if edge.symbol == "*" {
            // `export * from './y'`: whatever y publishes, this file publishes too, under the
            // same name, attributed to y. Round by round the barrel fills in.
            let prefix = format!("{}\u{1}", edge.target);
            let mine = format!("{file}\u{1}");
            let copies: Vec<(String, String)> = idx
                .extra
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix))
                .map(|(k, v)| (format!("{mine}{}", &k[prefix.len()..]), v.clone()))
                .collect();
            for (k, v) in copies {
                idx.extra.insert(k, v);
            }
            continue;
        }
        // A re-export of `local as exported` publishes the origin under the exported name.
        let renamed: Vec<(String, String)> = scan
            .aux
            .iter()
            .filter_map(|(k, v)| {
                k.strip_prefix("renamed:")
                    .map(|exported| (exported.to_string(), v.clone()))
            })
            .collect();
        let mut published = false;
        for (exported, local) in &renamed {
            if local == &edge.symbol {
                idx.extra.insert(
                    format!("{file}\u{1}{exported}"),
                    format!("{}\u{1}{}", edge.target, edge.symbol),
                );
                published = true;
            }
        }
        if !published && !renamed.iter().any(|(_, local)| local == &edge.symbol) {
            idx.extra.insert(
                format!("{file}\u{1}{}", edge.symbol),
                format!("{}\u{1}{}", edge.target, edge.symbol),
            );
        }
    }
}

pub fn exports(
    scans: &BTreeMap<String, Scan>,
    _edges: &BTreeMap<String, BTreeSet<Edge>>,
    _idx: &Index,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut out = BTreeMap::new();
    for (file, scan) in scans {
        let mut names = BTreeSet::new();
        for key in scan.aux.keys() {
            if let Some(name) = key.strip_prefix("export:") {
                names.insert(name.to_string());
            }
        }
        out.insert(file.clone(), names);
    }
    out
}

pub fn same_package(_file: &str, _scan: &Scan, _idx: &Index) -> BTreeSet<Edge> {
    BTreeSet::new()
}
