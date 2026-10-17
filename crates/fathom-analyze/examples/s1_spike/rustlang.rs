//! Spike `s1` — the throwaway Rust extractor.
//!
//! Nothing here is the shipping design; it is the thing that answers "can tree-sitter-rust tell
//! `e3.1` what a file defines and what it can name". It is written the way a first implementation
//! would be written in an afternoon, and every shortcut it takes is a number in the report.

use crate::common::{base_type, field, kids, qualify, stem, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

pub fn language() -> Language {
    tree_sitter_rust::LANGUAGE.into()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Enc {
    File,
    Module,
    Trait,
    Impl,
}

/// The nesting a declaration sits in.
#[derive(Clone)]
struct Scope {
    /// Dotted name a member's name hangs off (`Rect` for `Rect.new`), empty at file top level.
    container: String,
    /// Module path the declaration sits in, crate-relative.
    module: Vec<String>,
    enc: Enc,
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Scan) {
    let module_path = file_module(file);
    out.aux.insert("module".into(), module_path.join("::"));
    items(
        root,
        src,
        &Scope {
            container: String::new(),
            module: module_path,
            enc: Enc::File,
        },
        out,
    );
}

/// `lib.rs`/`main.rs` is the crate root; `geo/mod.rs` is `geo`; `geo/point.rs` is `geo::point`.
fn file_module(file: &str) -> Vec<String> {
    let stem = stem(file);
    if stem.is_empty() || stem == "lib" || stem == "main" {
        return Vec::new();
    }
    stem.split('/').map(|s| s.to_string()).collect()
}

fn visibility(node: Node, src: &[u8]) -> &'static str {
    for kid in kids(&node) {
        if kid.kind() == "visibility_modifier" {
            let t = text(&kid, src);
            if t.contains("crate") {
                return "pub_crate";
            }
            if t.contains("super") || t.contains("in ") {
                return "pub_scoped";
            }
            return "pub";
        }
    }
    "private"
}

/// The attributes stacked in front of an item, concatenated.
fn attributes(node: Node, src: &[u8]) -> String {
    let mut attrs = String::new();
    let mut sib = node.prev_named_sibling();
    while let Some(cur) = sib {
        if cur.kind() != "attribute_item" {
            break;
        }
        attrs.push_str(text(&cur, src));
        attrs.push(' ');
        sib = cur.prev_named_sibling();
    }
    attrs
}

/// A miniature cfg evaluator: "what does `cargo build` on this host compile?". It knows four
/// spellings, which is every spelling in the corpus, and it is wrong for any cfg a build script
/// invents — which is the dealbreaker this spike is reporting, not a bug to fix here.
fn cfg_excluded(attrs: &str) -> bool {
    let clean = attrs.replace("cfg_attr", "ignored");
    if clean.contains("cfg(test") {
        return true;
    }
    if clean.contains("cfg(not(") {
        return false;
    }
    clean.contains("cfg(feature")
}

fn name_of(kid: Node, src: &[u8]) -> Option<String> {
    field(&kid, "name").map(|n| text(&n, src).to_string())
}

fn items(node: Node, src: &[u8], scope: &Scope, out: &mut Scan) {
    for kid in kids(&node) {
        let kind = kid.kind();
        match kind {
            "mod_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let full = qualify(&scope.container, &name);
                let vis = visibility(kid, src);
                out.defs.push(Def::new("module", &full));
                record(out, &full, vis, kid, src, &scope.module);
                mark_module(out, &scope.module, &name, vis);
                let inner = Scope {
                    container: full,
                    module: {
                        let mut m = scope.module.clone();
                        m.push(name.clone());
                        m
                    },
                    enc: Enc::Module,
                };
                match field(&kid, "body") {
                    Some(body) => items(body, src, &inner, out),
                    None => {
                        // `mod x;` is a file inclusion: the crate's module tree lives here.
                        let mut imp = RawImport::new(inner.module.join("::"), &name, "mod_decl");
                        imp.module = scope.module.clone();
                        imp.reexport = vis == "pub";
                        out.imports.push(imp);
                    }
                }
            }
            "trait_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let full = qualify(&scope.container, &name);
                let vis = visibility(kid, src);
                out.defs.push(Def::new("interface", &full));
                record(out, &full, vis, kid, src, &scope.module);
                items(
                    field(&kid, "body").unwrap_or(kid),
                    src,
                    &Scope {
                        container: full,
                        module: scope.module.clone(),
                        enc: Enc::Trait,
                    },
                    out,
                );
            }
            "struct_item" | "union_item" => declare(out, kid, src, scope, "struct"),
            "enum_item" => declare(out, kid, src, scope, "enum"),
            "impl_item" => {
                let Some(ty) = field(&kid, "type").map(|n| text(&n, src).to_string()) else {
                    continue;
                };
                items(
                    field(&kid, "body").unwrap_or(kid),
                    src,
                    &Scope {
                        container: base_type(&ty),
                        module: scope.module.clone(),
                        enc: Enc::Impl,
                    },
                    out,
                );
            }
            "function_item" | "function_signature_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let full = qualify(&scope.container, &name);
                let def_kind = if scope.enc == Enc::File || scope.enc == Enc::Module {
                    "fn"
                } else {
                    "method"
                };
                out.defs.push(Def::new(def_kind, &full));
                record(out, &full, visibility(kid, src), kid, src, &scope.module);
            }
            "const_item" | "static_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let full = qualify(&scope.container, &name);
                out.defs.push(Def::new("const", &full));
                record(out, &full, visibility(kid, src), kid, src, &scope.module);
            }
            "associated_type" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let full = qualify(&scope.container, &name);
                out.defs.push(Def::new("type", &full));
                record(out, &full, visibility(kid, src), kid, src, &scope.module);
            }
            "type_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                if scope.enc == Enc::Impl {
                    // `type Output = Point` inside `impl Add for Point` binds a requirement that
                    // is declared elsewhere; counting it would double-count the trait's.
                    out.note("associated_type_binding", &name);
                    continue;
                }
                let full = qualify(&scope.container, &name);
                out.defs.push(Def::new("type", &full));
                record(out, &full, visibility(kid, src), kid, src, &scope.module);
            }
            "use_declaration" => {
                let vis = visibility(kid, src);
                let mut sink = Vec::new();
                if let Some(arg) = field(&kid, "argument") {
                    use_walk(arg, src, String::new(), &mut sink);
                }
                for (spec, symbol) in sink {
                    let mut imp = RawImport::new(spec, symbol, "use");
                    imp.module = scope.module.clone();
                    imp.reexport = vis == "pub";
                    out.imports.push(imp);
                }
            }
            "macro_definition" => out.note(
                "macro_definition",
                text(&kid, src).split('!').next().unwrap_or("?"),
            ),
            "macro_invocation" => out.note(
                "macro_invocation",
                text(&kid, src).split('!').next().unwrap_or("?"),
            ),
            _ => {}
        }
    }
}

fn declare(out: &mut Scan, kid: Node, src: &[u8], scope: &Scope, kind: &str) {
    let Some(name) = name_of(kid, src) else {
        return;
    };
    let full = qualify(&scope.container, &name);
    out.defs.push(Def::new(kind, &full));
    record(out, &full, visibility(kid, src), kid, src, &scope.module);
}

fn record(out: &mut Scan, name: &str, vis: &str, node: Node, src: &[u8], module: &[String]) {
    out.aux.insert(format!("vis:{name}"), vis.to_string());
    if cfg_excluded(&attributes(node, src)) {
        out.aux.insert(format!("cfgout:{name}"), "1".into());
    } else {
        // Two `#[cfg]`/`#[cfg(not)]` arms of one name: the second arm is the one the default
        // build compiles, and it must clear the first arm's exclusion.
        out.aux.remove(&format!("cfgout:{name}"));
    }
    out.aux.insert(format!("mod:{name}"), module.join("::"));
}

fn mark_module(out: &mut Scan, module: &[String], name: &str, vis: &str) {
    let mut path = module.to_vec();
    path.push(name.to_string());
    out.aux.insert(
        format!("mpub:{}", path.join("::")),
        if vis == "pub" {
            "1".to_string()
        } else {
            "0".to_string()
        },
    );
}

/// Flatten `use a::b::{c, d as e, *};` into (full specifier, named symbol) pairs.
fn use_walk(node: Node, src: &[u8], prefix: String, out: &mut Vec<(String, String)>) {
    let path = |prefix: &str, t: &str| -> String {
        if prefix.is_empty() {
            t.to_string()
        } else {
            format!("{prefix}::{t}")
        }
    };
    match node.kind() {
        "use_clause" => {
            let mut full = prefix;
            let mut list = None;
            let mut wildcard = false;
            let mut named = 0usize;
            let mut aliased = false;
            for kid in kids(&node) {
                match kid.kind() {
                    "identifier" | "scoped_identifier" | "crate" | "super" | "self"
                    | "metavariable" => {
                        full = path(&full, text(&kid, src));
                        named += 1;
                    }
                    "use_as_clause" => {
                        aliased = true;
                        let inner = field(&kid, "path").map(|p| text(&p, src)).unwrap_or("");
                        out.push((
                            path(&full, inner),
                            inner.rsplit("::").next().unwrap_or("").to_string(),
                        ));
                    }
                    "use_list" => list = Some(kid),
                    // `use a::b::{c, d}` keeps its path *inside* the group node, so the group has
                    // to be walked as a path, not treated as a list of names.
                    "scoped_use_list" => use_walk(kid, src, full.clone(), out),
                    "use_wildcard" => wildcard = true,
                    _ => {}
                }
            }
            if wildcard {
                out.push((full, "*".to_string()));
            } else if let Some(list) = list {
                for kid in kids(&list) {
                    use_walk(kid, src, full.clone(), out);
                }
            } else if named > 0 && !aliased {
                let symbol = full.rsplit("::").next().unwrap_or("").to_string();
                out.push((full, symbol));
            } else if named == 0 && !aliased {
                out.push((full, "*".to_string()));
            }
        }
        "use_as_clause" => {
            let inner = field(&node, "path").map(|p| text(&p, src)).unwrap_or("");
            let full = path(&prefix, inner);
            let symbol = full.rsplit("::").next().unwrap_or("").to_string();
            out.push((full, symbol));
        }
        "identifier" | "scoped_identifier" => {
            let full = path(&prefix, text(&node, src));
            out.push((
                full.clone(),
                full.rsplit("::").next().unwrap_or("").to_string(),
            ));
        }
        // `use a::b::{c, d as e, *}`: the group node holds both the path and the names, so the
        // names must not be appended to the path the way a plain `use a::b::c` would be.
        "scoped_use_list" => {
            let mut joined = prefix.clone();
            let mut list = None;
            for kid in kids(&node) {
                match kid.kind() {
                    "identifier" | "scoped_identifier" | "crate" | "super" | "self" => {
                        joined = path(&joined, text(&kid, src));
                    }
                    "use_list" | "scoped_use_list" => list = Some(kid),
                    _ => {}
                }
            }
            if let Some(list) = list {
                use_names(list, src, joined, out);
            }
        }
        "use_list" => use_names(node, src, prefix, out),
        // `use a::b::*` — the wildcard is the whole clause, with the path inside it.
        "use_wildcard" => {
            let mut joined = prefix;
            for kid in kids(&node) {
                if matches!(
                    kid.kind(),
                    "identifier" | "scoped_identifier" | "crate" | "super" | "self"
                ) {
                    joined = path(&joined, text(&kid, src));
                }
            }
            out.push((joined, "*".to_string()));
        }
        _ => {}
    }
}

/// The names inside `{ ... }`, each paired with the path the group was reached through.
fn use_names(list: Node, src: &[u8], prefix: String, out: &mut Vec<(String, String)>) {
    let join = |p: &str, t: &str| -> String {
        if p.is_empty() {
            t.to_string()
        } else {
            format!("{p}::{t}")
        }
    };
    let last = |t: &str| t.rsplit("::").next().unwrap_or("").to_string();
    for kid in kids(&list) {
        match kid.kind() {
            "use_wildcard" => out.push((prefix.clone(), "*".to_string())),
            "use_as_clause" => {
                let inner = field(&kid, "path").map(|p| text(&p, src)).unwrap_or("");
                let full = join(&prefix, inner);
                out.push((full.clone(), last(&full)));
            }
            "self" => out.push((join(&prefix, "self"), "self".to_string())),
            _ => {
                let t = text(&kid, src);
                let full = join(&prefix, t);
                out.push((full.clone(), last(&full)));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// resolution
// ---------------------------------------------------------------------------

/// Module path -> the file that declares it.
fn module_file(idx: &Index, module: &[String]) -> Option<String> {
    if module.is_empty() {
        for root in ["lib.rs", "main.rs", "mod.rs"] {
            if idx.has(root) {
                return Some(root.to_string());
            }
        }
        return None;
    }
    let joined = module.join("/");
    [format!("{joined}.rs"), format!("{joined}/mod.rs")]
        .into_iter()
        .find(|candidate| idx.has(candidate))
}

/// Resolve a (file, symbol) pair to where the symbol is *declared*, following re-exports:
/// `geo/mod.rs::Point` is declared in `geo/point.rs`.
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

/// Record what each file makes available, so a later pass can follow a re-export hop.
pub fn surface(file: &str, scan: &Scan, edges: &BTreeSet<Edge>, idx: &mut Index) {
    for edge in edges {
        if edge.symbol != "*" {
            idx.extra.insert(
                format!("{file}\u{1}{}", edge.symbol),
                format!("{}\u{1}{}", edge.target, edge.symbol),
            );
        }
    }
    let _ = scan;
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let mut out = BTreeSet::new();
    for imp in imports {
        let mut segs: Vec<String> = imp
            .spec
            .split("::")
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if segs.is_empty() {
            continue;
        }
        let mut base: Vec<String> = Vec::new();
        while let Some(head) = segs.first().map(|s| s.as_str()) {
            match head {
                "crate" => {
                    segs.remove(0);
                }
                "self" => {
                    base = imp.module.clone();
                    segs.remove(0);
                }
                "super" => {
                    base = imp.module.clone();
                    base.pop();
                    segs.remove(0);
                }
                _ => break,
            }
        }
        if imp.form == "mod_decl" {
            if let Some(target) = module_file(idx, &segs) {
                let name = segs.last().cloned().unwrap_or_default();
                out.insert(Edge::new(target, name));
            }
            continue;
        }
        base.extend(segs.iter().cloned());
        // Longest prefix that is a module wins; what is left over is the symbol named.
        let mut resolved = false;
        for cut in (1..=base.len()).rev() {
            let Some(target) = module_file(idx, &base[..cut]) else {
                continue;
            };
            let rest = &base[cut..];
            let symbol = if !rest.is_empty() {
                rest.join("::")
            } else if imp.symbol == "*" {
                "*".to_string()
            } else {
                base[..cut].last().cloned().unwrap_or_default()
            };
            let mut seen = Vec::new();
            if let Some(edge) = follow(idx, &target, &symbol, &mut seen) {
                out.insert(edge);
                resolved = true;
                break;
            }
        }
        if !resolved {
            idx.extra
                .insert(format!("unresolved:{file}"), imp.spec.clone());
        }
    }
    out
}

/// The export set, which for Rust is not a fact about one file: an item is public API only if
/// every module on its path from the crate root is `pub`, or a `pub use` gives it a public path.
pub fn same_package(_file: &str, _scan: &Scan, _idx: &Index) -> BTreeSet<Edge> {
    BTreeSet::new()
}

pub fn exports(
    scans: &BTreeMap<String, Scan>,
    edges: &BTreeMap<String, BTreeSet<Edge>>,
    _idx: &Index,
) -> BTreeMap<String, BTreeSet<String>> {
    let mut mod_pub: BTreeMap<String, bool> = BTreeMap::new();
    for scan in scans.values() {
        for (k, v) in &scan.aux {
            if let Some(path) = k.strip_prefix("mpub:") {
                mod_pub.insert(path.to_string(), v == "1");
            }
        }
    }
    // Which (declaring file, symbol) pairs some publicly reachable file re-exports.
    let mut reexported: BTreeSet<(String, String)> = BTreeSet::new();
    for (file, scan) in scans {
        let module = module_of(scan);
        if !path_is_public(&module, &mod_pub) {
            continue;
        }
        if let Some(list) = edges.get(file) {
            for edge in list {
                if edge.symbol != "*" {
                    reexported.insert((edge.target.clone(), edge.symbol.clone()));
                }
            }
        }
    }

    let mut out: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for (file, scan) in scans {
        let file_module = module_of(scan);
        let mut names = BTreeSet::new();
        for def in &scan.defs {
            // Members are out of scope for the export relation; see the spike's rules.
            if def.name.contains('.') {
                continue;
            }
            let vis = scan
                .aux
                .get(&format!("vis:{}", def.name))
                .map(|s| s.as_str())
                .unwrap_or("private");
            if vis != "pub" || scan.aux.contains_key(&format!("cfgout:{}", def.name)) {
                continue;
            }
            let mut module: Vec<String> = scan
                .aux
                .get(&format!("mod:{}", def.name))
                .map(|s| {
                    s.split("::")
                        .filter(|x| !x.is_empty())
                        .map(String::from)
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default();
            if module.is_empty() {
                module = file_module.clone();
            }
            let public = path_is_public(&module, &mod_pub);
            if public || reexported.contains(&(file.clone(), def.name.clone())) {
                names.insert(def.name.clone());
            }
        }
        out.insert(file.clone(), names);
    }
    out
}

fn module_of(scan: &Scan) -> Vec<String> {
    scan.aux
        .get("module")
        .map(|s| {
            s.split("::")
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Every module on the path from the crate root must itself be `pub`.
fn path_is_public(module: &[String], mod_pub: &BTreeMap<String, bool>) -> bool {
    let mut prefix: Vec<String> = Vec::new();
    for part in module {
        prefix.push(part.clone());
        if !mod_pub.get(&prefix.join("::")).copied().unwrap_or(false) {
            return false;
        }
    }
    true
}
