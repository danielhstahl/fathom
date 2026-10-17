//! Spike `s1` — the throwaway Java extractor.

use crate::common::{field, kids, qualify, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

pub fn language() -> Language {
    tree_sitter_java::LANGUAGE.into()
}

const TYPE_KINDS: [&str; 5] = [
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

pub fn scan(root: Node, src: &[u8], _file: &str, out: &mut Scan) {
    statements(root, src, "", out);
    // A same-package reference has no import statement to read, so the names this file mentions
    // are collected and resolved later against the corpus's package index.
    let mut refs = BTreeSet::new();
    collect_type_references(root, src, &mut refs);
    for name in refs {
        out.aux.insert(format!("type:{name}"), "1".into());
    }
    // A `record`'s components are parameters of an implicit constructor, and each one becomes an
    // accessor the compiler writes. The file does not contain them, so the honest record of a
    // component is a blind spot, not a definition — the same call `@dataclass` forces in Python.
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == "record_declaration" {
            let name = name_of(&node, src).unwrap_or_default();
            for params in kids(&node) {
                if params.kind() != "formal_parameters" {
                    continue;
                }
                for component in kids(&params) {
                    if matches!(component.kind(), "receiver_parameter" | "formal_parameter")
                        && let Some(member) = name_of(&component, src)
                    {
                        out.note("record_accessor", &format!("{name}.{member}"));
                    }
                }
            }
        }
        for kid in kids(&node) {
            stack.push(kid);
        }
    }
}

fn name_of(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
}

fn statements(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    for kid in kids(&node) {
        if TYPE_KINDS.contains(&kid.kind()) {
            declare_type(kid, src, container, out);
            continue;
        }
        match kid.kind() {
            "package_declaration" => {
                let raw = text(&kid, src)
                    .trim_start_matches("package")
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                out.aux.insert("package".into(), raw);
            }
            "module_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    out.defs.push(Def::new("module", &name));
                    out.aux.insert(format!("top:{name}"), "1".into());
                }
                let body = field(&kid, "body").unwrap_or(kid);
                for directive in kids(&body) {
                    if directive.kind() == "exports_module_directive"
                        && let Some(pkg) = kids(&directive)
                            .iter()
                            .find(|k| k.kind() == "scoped_identifier" || k.kind() == "identifier")
                            .map(|k| text(k, src).to_string())
                    {
                        let key = out
                            .aux
                            .get("module_exports")
                            .map(|old| format!("{old},{pkg}"))
                            .unwrap_or_else(|| pkg.clone());
                        out.aux.insert("module_exports".into(), key);
                    }
                }
            }
            "import_declaration" => {
                let raw = text(&kid, src)
                    .trim_start_matches("import")
                    .trim()
                    .trim_end_matches(';')
                    .trim()
                    .to_string();
                let (form, body) = match raw.strip_prefix("static ") {
                    Some(rest) => ("static_import", rest.to_string()),
                    None => (
                        if raw.ends_with(".*") {
                            "wildcard_import"
                        } else {
                            "single_import"
                        },
                        raw,
                    ),
                };
                let (package, symbol, owner) = if body.ends_with(".*") {
                    (
                        body[..body.len() - 2].to_string(),
                        "*".to_string(),
                        String::new(),
                    )
                } else {
                    let last = body.rsplit('.').next().unwrap_or("").to_string();
                    let mut package = body[..body.len() - last.len()]
                        .trim_end_matches('.')
                        .to_string();
                    let (symbol, owner) = if form == "static_import" {
                        // `import static a.b.Geom.area` names a member of a type: the type is the
                        // second-to-last segment, and the edge points at the member.
                        let ty = package.rsplit('.').next().unwrap_or("").to_string();
                        package = package[..package.len() - ty.len()]
                            .trim_end_matches('.')
                            .to_string();
                        (last, ty)
                    } else {
                        (last.clone(), String::new())
                    };
                    (package, symbol, owner)
                };
                let mut imp = RawImport::new(package.clone(), symbol, form);
                // The second slot carries the type a static import reached through, which is the
                // only way to tell `Geom.area` from some other `area`.
                imp.module = vec![package, owner];
                out.imports.push(imp);
            }
            _ => {}
        }
    }
}

fn declare_type(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    let Some(name) = name_of(&node, src) else {
        return;
    };
    let kind = match node.kind() {
        "interface_declaration" | "annotation_type_declaration" => "interface",
        "enum_declaration" => "enum",
        _ => "class",
    };
    let stored = qualify(container, &name);
    out.defs.push(Def::new(kind, &stored));
    if container.is_empty() {
        out.aux.insert(format!("top:{stored}"), "1".into());
        out.aux.insert(
            format!("public:{stored}"),
            if is_public(node, src) { "1" } else { "0" }.into(),
        );
    }
    if let Some(body) = field(&node, "body") {
        members(body, src, &stored, out);
    }
}

fn members(node: Node, src: &[u8], container: &str, out: &mut Scan) {
    for kid in kids(&node) {
        if TYPE_KINDS.contains(&kid.kind()) {
            declare_type(kid, src, container, out);
            continue;
        }
        match kid.kind() {
            "annotation_type_element_declaration" => {
                // `String value();` inside `@interface` is the type's method declaration itself.
                if let Some(name) = name_of(&kid, src) {
                    out.defs.push(Def::new("method", qualify(container, &name)));
                }
            }
            "method_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    out.defs.push(Def::new("method", qualify(container, &name)));
                }
            }
            "constructor_declaration" | "compact_constructor_declaration" => {
                out.note("constructor", container);
            }
            "field_declaration" => {}
            _ => {
                if matches!(
                    kid.kind(),
                    "class_body" | "interface_body" | "enum_body" | "enum_body_declarations"
                ) {
                    members(kid, src, container, out);
                }
            }
        }
    }
}

fn is_public(node: Node, src: &[u8]) -> bool {
    kids(&node)
        .iter()
        .any(|k| k.kind() == "modifiers" && text(k, src).split_whitespace().any(|w| w == "public"))
}

fn collect_type_references(node: Node, src: &[u8], out: &mut BTreeSet<String>) {
    for kid in kids(&node) {
        if kid.kind() == "type_identifier" {
            out.insert(text(&kid, src).to_string());
        }
        collect_type_references(kid, src, out);
    }
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let mut out = BTreeSet::new();
    for imp in imports {
        let package = imp.module.first().cloned().unwrap_or_default();
        let mut hits: Vec<String> = Vec::new();
        for (other, pkg) in &idx.extra {
            let Some(path) = other.strip_prefix("pkg:") else {
                continue;
            };
            if pkg != &package {
                continue;
            }
            let owner = imp.module.get(1).filter(|o| !o.is_empty());
            let names_it = imp.symbol == "*"
                || idx.defines(path, &imp.symbol)
                || owner.is_some_and(|owner| idx.defines(path, &format!("{}.{owner}", imp.symbol)))
                || owner.is_some_and(|owner| idx.defines(path, owner));
            if names_it {
                hits.push(path.to_string());
            }
        }
        if hits.is_empty() {
            idx.extra.insert(
                format!("unresolved:{file}"),
                format!("{package}.{}", imp.symbol),
            );
            continue;
        }
        for target in hits {
            if target == file {
                continue;
            }
            out.insert(Edge::new(target, imp.symbol.clone()));
        }
    }
    out
}

/// Same-package edges, computed where the corpus-wide package index exists.
pub fn same_package(file: &str, scan: &Scan, idx: &Index) -> BTreeSet<Edge> {
    let package = match scan.aux.get("package") {
        Some(p) => p.clone(),
        None => return BTreeSet::new(),
    };
    let mut out = BTreeSet::new();
    for key in scan.aux.keys() {
        let Some(name) = key.strip_prefix("type:") else {
            continue;
        };
        for (other, pkg) in &idx.extra {
            let Some(path) = other.strip_prefix("pkg:") else {
                continue;
            };
            if path == file || pkg != &package {
                continue;
            }
            if idx.defines(path, name) {
                out.insert(Edge::new(path.to_string(), name.to_string()));
            }
        }
    }
    out
}

pub fn surface(_file: &str, _scan: &Scan, _edges: &BTreeSet<Edge>, _idx: &mut Index) {}

pub fn exports(
    scans: &BTreeMap<String, Scan>,
    _edges: &BTreeMap<String, BTreeSet<Edge>>,
    _idx: &Index,
) -> BTreeMap<String, BTreeSet<String>> {
    let exported: Vec<String> = scans
        .values()
        .filter_map(|s| s.aux.get("module_exports").cloned())
        .flat_map(|s| s.split(',').map(|x| x.to_string()).collect::<Vec<String>>())
        .collect();
    let has_module = scans.values().any(|s| s.aux.contains_key("module_exports"));
    let mut out = BTreeMap::new();
    for (file, scan) in scans {
        let package = scan.aux.get("package").cloned().unwrap_or_default();
        let package_ok = !has_module || exported.contains(&package);
        let mut names = BTreeSet::new();
        if package_ok {
            for def in &scan.defs {
                if def.name.contains('.') {
                    continue;
                }
                if scan
                    .aux
                    .get(&format!("public:{}", def.name))
                    .map(|v| v == "1")
                    .unwrap_or(false)
                {
                    names.insert(def.name.clone());
                }
            }
        }
        out.insert(file.clone(), names);
    }
    out
}
