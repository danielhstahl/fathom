//! The Rust extractor.
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/rustlang.rs`) with
//! the four things a spike is allowed to skip: spans, complexity, supertypes and the "cannot
//! enumerate" marks. The *rules* are the spike's, because they were measured — Rust was the language
//! where the grammar helped most and misled most
//! ([`docs/concepts/structure-extraction.md`](../../../../docs/concepts/structure-extraction.md)).
//!
//! Two things here are load-bearing beyond Rust:
//!
//! * **A `mod x;` is an edge.** A Rust file's dependencies are not `use` statements: the module
//!   declarations are what make a directory a module tree, and `pub use crate::geo::point::Point;`
//!   in `geo/mod.rs` means the real declaration is in `geo/point.rs` — an edge to `geo/mod.rs` is an
//!   edge to a forwarder.
//! * **Visibility is a path property, not an item property.** An item is public API only if every
//!   module between it and the crate root is `pub`, or a `pub use` gives it a public path.

use crate::structure::engine::{
    self, CorpusView, Draft, Index, Lang, RawImport, follow, origin_key, publish_origin,
};
use crate::structure::model::{
    Complexity, Language, Supertype, SupertypeKind, Symbol, SymbolKind, Visibility, synthetic,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{self, base_type, field, kids, qualify, span_of, stem, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

/// Words in a macro's token tree that suggest the macro writes declarations.
const ITEM_KEYWORDS: &[&str] = &[
    "fn",
    "struct",
    "enum",
    "trait",
    "impl",
    "const",
    "static",
    "type",
    "pub",
    "macro_rules!",
];

/// Node kinds counted as branches: each one is a fork a reader has to hold.
const BRANCH_KINDS: &[&str] = &[
    "if_expr",
    "match_arm",
    "let_else",
    "if_condition",
    "else_clause",
    "while_condition",
];
/// Node kinds counted as iterations.
const LOOP_KINDS: &[&str] = &["for_expr", "for_loop", "while_loop", "loop_expression"];
/// Node kinds counted as an exit from the declaration.
const RETURN_KINDS: &[&str] = &[
    "return_expression",
    "break_expression",
    "continue_expression",
];
/// Kinds that nest: used for the depth proxy.
const NEST_KINDS: &[&str] = &[
    "if_expr",
    "match_block",
    "for_expr",
    "for_loop",
    "while_loop",
    "loop_expression",
    "unsafe_block",
];

pub fn grammar() -> TsLanguage {
    tree_sitter_rust::LANGUAGE.into()
}

pub const LANG: Lang = Lang {
    language: Language::Rust,
    grammar,
    scan,
    seed,
    resolve,
    publish,
    prepare_exports,
    export_names,
};

fn seed(_file: &str, _draft: &Draft, _index: &mut Index) {}

/// What a declaration sits inside, which decides whether a function is a `fn` or a `method`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Enc {
    File,
    Module,
    Type,
}

#[derive(Clone)]
struct Scope {
    /// Dotted name a member's name hangs off (`Rect` for `Rect.new`); empty at file top level.
    container: String,
    /// Module path the declaration sits in, crate-relative.
    module: Vec<String>,
    enc: Enc,
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let module_path = file_module(file);
    out.aux.insert("module".to_string(), module_path.join("::"));
    items(
        root,
        src,
        &Scope {
            container: String::new(),
            module: module_path,
            enc: Enc::File,
        },
        out,
        policy,
    );
}

/// `lib.rs`/`main.rs` is the crate root; `geo/mod.rs` is `geo`; `geo/point.rs` is `geo::point`.
fn file_module(file: &str) -> Vec<String> {
    let path = stem(file);
    let path = path.trim_end_matches("/mod").trim_end_matches("mod");
    if path.is_empty() || path == "lib" || path == "main" {
        return Vec::new();
    }
    path.split('/')
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect()
}

fn visibility(node: Node, src: &[u8]) -> Visibility {
    for kid in kids(&node) {
        if kid.kind() == "visibility_modifier" {
            let spelling = text(&kid, src);
            if spelling.contains("crate") || spelling.contains("self") {
                return Visibility::Crate;
            }
            if spelling.contains("super") || spelling.contains("in ") {
                return Visibility::Scoped;
            }
            return Visibility::Public;
        }
    }
    Visibility::Private
}

/// The attributes stacked in front of an item, concatenated.
fn attributes(node: Node, src: &[u8]) -> String {
    let mut attrs = String::new();
    let mut sibling = node.prev_named_sibling();
    while let Some(current) = sibling {
        if current.kind() != "attribute_item" {
            break;
        }
        attrs.push_str(text(&current, src));
        attrs.push(' ');
        sibling = current.prev_named_sibling();
    }
    attrs
}

/// A miniature cfg evaluator: "what does `cargo build` on this host compile?".
///
/// It knows four spellings, which is every spelling in the corpus, and it is wrong for any cfg a
/// build script invents — which is a stated limitation of the extractor rather than a bug to fix
/// here, and the reason a cfg-gated item is reported with its cfg rather than silently dropped.
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

fn name_of(node: Node, src: &[u8]) -> Option<String> {
    field(&node, "name").map(|n| text(&n, src).to_string())
}

/// Modifiers worth keeping on a symbol: the spellings that change what a reader expects.
fn modifiers(node: Node, src: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let spelling = text(&node, src);
    for word in ["async", "unsafe", "const", "extern", "default"] {
        if spelling.starts_with(word) || spelling.contains(&format!("{word} ")) {
            out.push(word.to_string());
        }
    }
    let attrs = attributes(node, src);
    if attrs.contains("test") {
        out.push("test".to_string());
    }
    if attrs.contains("deprecated") {
        out.push("deprecated".to_string());
    }
    if attrs.contains("doc(hidden)") {
        out.push("doc_hidden".to_string());
    }
    out.sort();
    out.dedup();
    out
}

fn complexity(node: Node, src: &[u8], policy: &ExtractPolicy) -> Complexity {
    let span = span_of(&node);
    let (branches, _) = util::count_kinds(&node, BRANCH_KINDS, policy.max_complexity_nodes);
    let (loops, _) = util::count_kinds(&node, LOOP_KINDS, policy.max_complexity_nodes);
    let (returns, _) = util::count_kinds(&node, RETURN_KINDS, policy.max_complexity_nodes);
    let (nesting, _) = util::max_depth_of(&node, NEST_KINDS, policy.max_complexity_nodes);
    let params = field(&node, "parameters")
        .map(|parameters| kids(&parameters).len())
        .unwrap_or(0);
    let members = field(&node, "body")
        .map(|body| {
            kids(&body)
                .iter()
                .filter(|kid| {
                    matches!(
                        kid.kind(),
                        "function_item"
                            | "function_signature_item"
                            | "const_item"
                            | "type_item"
                            | "associated_type"
                            // A struct's fields and an enum's variants are its members for a
                            // complexity proxy: `e3.5` reads "how much hangs off this type", and a
                            // 30-field config struct is a lot even though it declares no functions.
                            | "field_declaration"
                            | "variant"
                    )
                })
                .count()
        })
        .unwrap_or(0);
    let _ = src;
    Complexity {
        body_lines: span.lines(),
        branches,
        loops,
        params,
        returns,
        members,
        nesting,
    }
}

fn declare(
    out: &mut Draft,
    kid: Node,
    src: &[u8],
    scope: &Scope,
    kind: SymbolKind,
    policy: &ExtractPolicy,
) -> Option<String> {
    let name = name_of(kid, src)?;
    let full = qualify(&scope.container, &name);
    out.symbols.push(Symbol {
        kind,
        name: full.clone(),
        container: scope.container.clone(),
        span: span_of(&kid),
        visibility: visibility(kid, src),
        exported: false,
        complexity: complexity(kid, src, policy),
        modifiers: modifiers(kid, src),
    });
    record(out, &full, visibility(kid, src), kid, src, &scope.module);
    Some(full)
}

fn record(out: &mut Draft, name: &str, vis: Visibility, node: Node, src: &[u8], module: &[String]) {
    out.aux
        .insert(format!("vis:{name}"), vis.as_str().to_string());
    if cfg_excluded(&attributes(node, src)) {
        out.aux.insert(format!("cfgout:{name}"), "1".into());
    } else {
        // Two `#[cfg]` / `#[cfg(not)]` arms of one name: the second arm is the one the default
        // build compiles, and it has to clear the first arm's exclusion.
        out.aux.remove(&format!("cfgout:{name}"));
    }
    out.aux.insert(format!("mod:{name}"), module.join("::"));
}

fn mark_module(out: &mut Draft, module: &[String], name: &str, vis: Visibility) {
    let mut path = module.to_vec();
    path.push(name.to_string());
    out.aux.insert(
        format!("mpub:{}", path.join("::")),
        if vis.is_public() {
            "1".to_string()
        } else {
            "0".to_string()
        },
    );
}

fn items(node: Node, src: &[u8], scope: &Scope, out: &mut Draft, policy: &ExtractPolicy) {
    for kid in kids(&node) {
        let kind = kid.kind();
        match kind {
            "mod_item" => {
                let Some(name) = name_of(kid, src) else {
                    continue;
                };
                let vis = visibility(kid, src);
                let Some(full) = declare(out, kid, src, scope, SymbolKind::Module, policy) else {
                    continue;
                };
                mark_module(out, &scope.module, &name, vis);
                let inner = Scope {
                    container: full.clone(),
                    module: {
                        let mut module = scope.module.clone();
                        module.push(name.clone());
                        module
                    },
                    enc: Enc::Module,
                };
                match field(&kid, "body") {
                    Some(body) => items(body, src, &inner, out, policy),
                    None => {
                        // `mod x;` is a file inclusion: the crate's module tree lives here. This is
                        // the edge that makes a directory a module, and it is a resolved edge with a
                        // named symbol, not a wildcard.
                        let mut import = RawImport::new(
                            inner.module.join("::"),
                            &name,
                            "mod_decl",
                            span_of(&kid),
                        );
                        import.module = scope.module.clone();
                        import.reexport = vis.is_public();
                        out.imports.push(import);
                    }
                }
            }
            "trait_item" => {
                let Some(full) = declare(out, kid, src, scope, SymbolKind::Interface, policy)
                else {
                    continue;
                };
                // `trait Shape: fmt::Debug` — a supertrait is a contract this trait inherits, and
                // `e3.2`'s "who implements Shape?" walk starts from it.
                if let Some(bounds) = field(&kid, "bounds") {
                    for bound in kids(&bounds) {
                        let spelling = text(&bound, src).trim();
                        if spelling.is_empty() || spelling == "+" {
                            continue;
                        }
                        out.supertypes.push(Supertype {
                            of: full.clone(),
                            base: base_type(spelling),
                            kind: SupertypeKind::Trait,
                            resolved: None,
                            span: span_of(&bound),
                        });
                    }
                }
                items(
                    field(&kid, "body").unwrap_or(kid),
                    src,
                    &Scope {
                        container: full,
                        module: scope.module.clone(),
                        enc: Enc::Type,
                    },
                    out,
                    policy,
                );
            }
            "struct_item" | "union_item" => {
                declare(out, kid, src, scope, SymbolKind::Struct, policy);
            }
            "enum_item" => {
                declare(out, kid, src, scope, SymbolKind::Enum, policy);
            }
            "impl_item" => {
                let Some(spelling) = field(&kid, "type").map(|n| text(&n, src).to_string()) else {
                    continue;
                };
                let owner = base_type(&spelling);
                if let Some(trait_ref) = field(&kid, "trait") {
                    // `impl Shape for Rect` — a contract, not an ancestry.
                    let base = base_type(text(&trait_ref, src).trim());
                    out.supertypes.push(Supertype {
                        of: owner.clone(),
                        base,
                        kind: SupertypeKind::Trait,
                        resolved: None,
                        span: span_of(&trait_ref),
                    });
                }
                items(
                    field(&kid, "body").unwrap_or(kid),
                    src,
                    &Scope {
                        container: owner,
                        module: scope.module.clone(),
                        enc: Enc::Type,
                    },
                    out,
                    policy,
                );
            }
            "function_item" | "function_signature_item" => {
                let kind = if scope.enc == Enc::File || scope.enc == Enc::Module {
                    SymbolKind::Fn
                } else {
                    SymbolKind::Method
                };
                declare(out, kid, src, scope, kind, policy);
            }
            "const_item" | "static_item" => {
                declare(out, kid, src, scope, SymbolKind::Const, policy);
            }
            "associated_type" => {
                declare(out, kid, src, scope, SymbolKind::Type, policy);
            }
            "type_item" => {
                if scope.enc == Enc::Type && text(&kid, src).starts_with("type ") {
                    // `type Output = Point` inside `impl Add for Point` binds a requirement
                    // declared elsewhere; counting it would double-count the trait's.
                    let name = name_of(kid, src).unwrap_or_default();
                    out.note("associated_type_binding", &name);
                    continue;
                }
                declare(out, kid, src, scope, SymbolKind::Type, policy);
            }
            "use_declaration" => {
                let vis = visibility(kid, src);
                let mut sink: Vec<(String, String)> = Vec::new();
                if let Some(argument) = field(&kid, "argument") {
                    use_walk(argument, src, String::new(), &mut sink);
                }
                for (spec, symbol) in sink {
                    let mut import = RawImport::new(spec, symbol, "use", span_of(&kid));
                    import.module = scope.module.clone();
                    import.reexport = vis.is_public();
                    out.imports.push(import);
                }
            }
            "macro_definition" => {
                // The single most important line in this file, for s1 finding 1: a `macro_rules!`
                // in scope means the declaration list of this scope is *not* the set of declarations
                // the crate has. The scope is marked, not the absence asserted.
                let name = name_of(kid, src).unwrap_or_default();
                out.aux.insert(format!("macro:{name}"), "1".to_string());
                out.note("macro_definition", &name);
                out.mark_synthetic(&scope.container, synthetic::MACRO_DEFINITION);
            }
            "macro_invocation" => {
                let macro_name = text(&kid, src)
                    .split('!')
                    .next()
                    .unwrap_or("?")
                    .trim()
                    .to_string();
                // Which invocations can have written items? The extractor cannot expand a macro,
                // so it uses two signals it *can* see: the macro was defined in this file (so the
                // extractor watched its arm and can tell whether the arm declares items), or the
                // invocation's own token tree contains an item keyword (so it is very likely
                // declaring something). Everything else — `println!`, `vec![]`, `assert_eq!` — is
                // noted rather than marked, because marking every macro call would make
                // `synthetic_eligible` the answer to every question and therefore an answer to none.
                let declared_here = out.aux.contains_key(&format!("macro:{macro_name}"));
                let looks_like_items = ITEM_KEYWORDS.iter().any(|word| {
                    text(&kid, src)
                        .split_whitespace()
                        .any(|token| token == *word)
                });
                out.note("macro_invocation", &macro_name);
                if declared_here || looks_like_items {
                    out.mark_synthetic(&scope.container, synthetic::MACRO_INVOCATION);
                } else {
                    out.note("macro_invocation_ignored", &macro_name);
                }
            }
            "extern_block" => {
                out.note("extern_block", "foreign declarations are not symbols");
            }
            _ => {}
        }
    }
}

/// Flatten `use a::b::{c, d as e, *};` into (full specifier, named symbol) pairs.
fn use_walk(node: Node, src: &[u8], prefix: String, out: &mut Vec<(String, String)>) {
    let path = |prefix: &str, spelling: &str| -> String {
        if prefix.is_empty() {
            spelling.to_string()
        } else {
            format!("{prefix}::{spelling}")
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
    let join = |p: &str, spelling: &str| -> String {
        if p.is_empty() {
            spelling.to_string()
        } else {
            format!("{p}::{spelling}")
        }
    };
    let last = |spelling: &str| spelling.rsplit("::").next().unwrap_or("").to_string();
    for kid in kids(&list) {
        match kid.kind() {
            "use_wildcard" => out.push((prefix.clone(), "*".to_string())),
            "use_as_clause" => {
                let inner = field(&kid, "path").map(|p| text(&p, src)).unwrap_or("");
                let full = join(&prefix, inner);
                let symbol = last(&full);
                out.push((full, symbol));
            }
            "self" => out.push((join(&prefix, "self"), "self".to_string())),
            _ => {
                let spelling = text(&kid, src);
                let full = join(&prefix, spelling);
                let symbol = last(&full);
                out.push((full, symbol));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// resolution
// ---------------------------------------------------------------------------

/// Module path → the file that declares it.
fn module_file(index: &Index, module: &[String]) -> Option<String> {
    if module.is_empty() {
        for root in ["lib.rs", "main.rs", "mod.rs"] {
            if index.has(root) {
                return Some(root.to_string());
            }
        }
        return None;
    }
    let joined = module.join("/");
    [format!("{joined}.rs"), format!("{joined}/mod.rs")]
        .into_iter()
        .find(|candidate| index.has(candidate))
}

pub fn resolve(
    _file: &str,
    draft: &Draft,
    index: &mut Index,
    policy: &ExtractPolicy,
    truncated: &mut usize,
) -> Vec<engine::Edge> {
    let mut out: Vec<engine::Edge> = Vec::new();
    for import in &draft.imports {
        let mut segments: Vec<String> = import
            .spec
            .split("::")
            .filter(|s| !s.is_empty())
            .map(String::from)
            .collect();
        if segments.is_empty() {
            continue;
        }
        let mut base: Vec<String> = Vec::new();
        while let Some(head) = segments.first().map(String::as_str) {
            match head {
                "crate" => {
                    segments.remove(0);
                }
                "self" => {
                    base = import.module.clone();
                    segments.remove(0);
                }
                "super" => {
                    base = import.module.clone();
                    base.pop();
                    segments.remove(0);
                }
                _ => break,
            }
        }
        if import.form == "mod_decl" {
            if let Some(target) = module_file(index, &segments) {
                let name = segments.last().cloned().unwrap_or_default();
                let mut edge = engine::Edge::resolved(target, name);
                edge.form = import.form.to_string();
                edge.passes = 1;
                out.push(edge);
            } else {
                out.push(unresolved(import));
            }
            continue;
        }
        base.extend(segments.iter().cloned());
        // Longest prefix that is a module wins; what is left over is the symbol named.
        let mut resolved = false;
        for cut in (1..=base.len()).rev() {
            let Some(target) = module_file(index, &base[..cut]) else {
                continue;
            };
            let rest = &base[cut..];
            let symbol = if !rest.is_empty() {
                rest.join("::")
            } else if import.symbol == "*" {
                "*".to_string()
            } else {
                base[..cut].last().cloned().unwrap_or_default()
            };
            if symbol == "*" {
                // `use geo::*` — a real dependency on that module whose contents the parser cannot
                // name. Storing it as nothing answers "who depends on `point.rs`?" with "nobody".
                let mut edge = engine::Edge::wildcard(target);
                edge.form = import.form.to_string();
                edge.specifier = import.spec.clone();
                out.push(edge);
                resolved = true;
                break;
            }
            if let Some(chain) = follow(index, &target, &symbol, policy.max_resolution_hops) {
                let mut edge = engine::Edge::resolved(chain.target, chain.symbol);
                edge.via = chain.via;
                edge.passes = chain.hops;
                edge.form = import.form.to_string();
                if chain.truncated {
                    *truncated += 1;
                }
                out.push(edge);
                resolved = true;
                break;
            }
        }
        if !resolved {
            out.push(unresolved(import));
        }
    }
    out
}

fn unresolved(import: &RawImport) -> engine::Edge {
    engine::Edge {
        target: String::new(),
        symbol: import.symbol.clone(),
        kind: engine::EdgeKind::Unresolved,
        specifier: import.spec.clone(),
        via: None,
        form: import.form.to_string(),
        passes: 0,
    }
}

/// Record what this file makes available, so a later pass can follow a re-export hop:
/// `geo/mod.rs::Point` is declared in `geo/point.rs`.
pub fn publish(file: &str, draft: &Draft, edges: &[engine::Edge], index: &mut Index) {
    let _ = draft;
    for edge in edges {
        if edge.symbol != "*" && edge.kind == engine::EdgeKind::Resolved {
            publish_origin(index, file, &edge.symbol, &edge.target, &edge.symbol);
        }
    }
}

// ---------------------------------------------------------------------------
// exports
// ---------------------------------------------------------------------------

/// The export set is not a fact about one file: an item is public API only if every module on its
/// path from the crate root is `pub`, or a `pub use` gives it a public path. Both halves are
/// derived once here, into `index.extra`, so `export_names` is a lookup rather than a re-scan.
fn prepare_exports(view: &CorpusView) -> BTreeMap<String, String> {
    let mut derived: BTreeMap<String, String> = BTreeMap::new();
    let mut mod_public: BTreeSet<String> = BTreeSet::new();
    for draft in view.drafts.values() {
        for (key, value) in &draft.aux {
            if let Some(path) = key.strip_prefix("mpub:")
                && value == "1"
            {
                mod_public.insert(path.to_string());
            }
        }
    }
    for path in &mod_public {
        derived.insert(format!("mpub:{path}"), "1".to_string());
    }
    // Which (declaring file, symbol) pairs some publicly reachable file re-exports.
    for (file, draft) in view.drafts {
        let module = module_of(draft);
        if !path_is_public(&module, &mod_public) {
            continue;
        }
        if let Some(list) = view.edges.get(file) {
            for edge in list {
                if edge.symbol != "*" && edge.kind == engine::EdgeKind::Resolved {
                    derived.insert(
                        format!("reexported:{}", origin_key(&edge.target, &edge.symbol)),
                        "1".to_string(),
                    );
                }
            }
        }
    }
    derived
}

fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    let file_module = module_of(draft);
    let mod_public: BTreeSet<String> = view
        .index
        .extra
        .keys()
        .filter_map(|key| key.strip_prefix("mpub:").map(|p| p.to_string()))
        .collect();
    let mut names = BTreeSet::new();
    for symbol in &draft.symbols {
        // Members are out of scope for the export relation; see the normalisation rules.
        if symbol.name.contains('.') {
            continue;
        }
        let vis = draft
            .aux
            .get(&format!("vis:{}", symbol.name))
            .map(|s| s.as_str())
            .unwrap_or("private");
        if vis != "public" || draft.aux.contains_key(&format!("cfgout:{}", symbol.name)) {
            continue;
        }
        let mut module: Vec<String> = draft
            .aux
            .get(&format!("mod:{}", symbol.name))
            .map(|spelling| {
                spelling
                    .split("::")
                    .filter(|x| !x.is_empty())
                    .map(String::from)
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default();
        if module.is_empty() {
            module = file_module.clone();
        }
        let public = path_is_public(&module, &mod_public);
        let reexported = view
            .index
            .extra
            .contains_key(&format!("reexported:{}", origin_key(file, &symbol.name)));
        if public || reexported {
            names.insert(symbol.name.clone());
        }
    }
    names
}

fn module_of(draft: &Draft) -> Vec<String> {
    draft
        .aux
        .get("module")
        .map(|spelling| {
            spelling
                .split("::")
                .filter(|x| !x.is_empty())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

/// Every module on the path from the crate root must itself be `pub`.
fn path_is_public(module: &[String], mod_public: &BTreeSet<String>) -> bool {
    let mut prefix: Vec<String> = Vec::new();
    for part in module {
        prefix.push(part.clone());
        if !mod_public.contains(&prefix.join("::")) {
            return false;
        }
    }
    true
}
