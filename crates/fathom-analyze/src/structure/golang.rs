//! The Go extractor (`e3.1b`, `reco-dlp.1`).
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/golang.rs`) with
//! the four things a spike is allowed to skip — spans, complexity, supertypes, the cannot-enumerate
//! marks — and with the one mechanism the spike's findings demanded and `e3.1` could not ship
//! because it had no Go: **package granularity**.
//!
//! Go is the language where the grammar is the easiest and the *graph* is the most misleading. A Go
//! package is a directory; an import names the directory, never a file. A file-level rendering says
//! `main.go → shape/circle.go` where the language says `main → shape`, and s1's finding 4 is that
//! only one of those is "a dependency anyone can reason about"
//! ([`docs/concepts/structure-extraction.md`](../../../../docs/concepts/structure-extraction.md)).
//! The record stays per file — that is the granularity the store keys on and the granularity a delta
//! is computed on — but every edge a Go import produces is marked [`Granularity::Package`] and
//! carries the import path, so a pane can print the language's noun instead of inventing one.
//! [`Structure::package`](crate::structure::Structure::package) carries the file's own `package`
//! clause, which is the other half of that sentence.
//!
//! Three language rules the corpus commits to, all inherited from the spike:
//!
//! * **An import fans out to the package, with symbol `*`.** Not to one file and not to the
//!   directory path: every non-test file of that directory becomes an edge, contents unnamed,
//!   because the importer really can reach all of them.
//! * **`*_test.go` is a different package.** `shape_test.go` declares `package shape_test` in the
//!   same directory, so the directory-to-package mapping has to ignore test files as *targets*
//!   while still extracting them as files.
//! * **Export is a property of the name.** A first rune that is upper-case. There is no declaration
//!   marker for a parser to read, which is why the export rule here is a character test and not a
//!   visibility lookup — and why nothing downstream may infer "unexported" from the absence of a
//!   keyword.
//!
//! One thing the spike did with help this module refuses: the spike was *told* the module path (its
//! harness read `go.mod`). [`crate::structure::extract`] is a pure function of the files it is
//! handed, because that is what makes it a cache key (ADR-004 §5), so the module path is inferred
//! from the corpus instead — and when it cannot be, the imports come back `unresolved`, which is a
//! named state rather than a guess.

use crate::structure::engine::{CorpusView, Draft, Index, Lang, RawImport};
use crate::structure::model::{
    Complexity, Edge, EdgeKind, Granularity, Language, Supertype, SupertypeKind, Symbol,
    SymbolKind, Visibility, synthetic,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{self, dir_of, field, file_name, kids, qualify, span_of, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

pub fn grammar() -> TsLanguage {
    tree_sitter_go::LANGUAGE.into()
}

pub const LANG: Lang = Lang {
    language: Language::Go,
    grammar,
    scan,
    seed,
    resolve,
    publish,
    prepare_exports,
    export_names,
};

/// Index keys, namespaced as `Index::extra` asks: nothing outside this module reads them.
const SPEC_PREFIX: &str = "go:spec:";
const MODULE_KEY: &str = "go:module";

/// Node kinds counted as branches: each one is a fork a reader has to hold.
const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
];
/// Node kinds counted as iterations.
const LOOP_KINDS: &[&str] = &["for_statement"];
/// Node kinds counted as an exit from the declaration.
const RETURN_KINDS: &[&str] = &["return_statement", "break_statement", "continue_statement"];
/// Kinds that nest: used for the depth proxy.
const NEST_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "expression_switch_statement",
    "type_switch_statement",
    "select_statement",
];

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let mut build_constrained = false;
    // tree-sitter hangs a leading comment off the first real node rather than off the file, so the
    // build directives have to be looked for in the whole subtree and not among the top-level
    // children — a scan of `kids(&root)` finds none and the constraint silently disappears.
    for comment in descendants(&root, "comment") {
        let text = text(&comment, src);
        if text.contains("go:build") || text.contains("+build") {
            build_constrained = true;
            break;
        }
    }
    for kid in kids(&root) {
        match kid.kind() {
            "package_clause" => {
                // tree-sitter-go gives a package clause no `name` field: the name is a plain
                // `package_identifier` child. (Java's type parameter does the same thing, and the
                // second time this bites it is worth writing down: check `node-types.json` for
                // `"fields": {}` before trusting `child_by_field_name`.)
                let name = kids(&kid)
                    .into_iter()
                    .find(|child| child.kind() == "package_identifier")
                    .map(|n| text(&n, src).to_string());
                if let Some(name) = name {
                    out.unit = Some(name.clone());
                    out.aux.insert("package".to_string(), name);
                }
            }
            "import_declaration" => {
                for spec in descendants(&kid, "import_spec") {
                    let path = field(&spec, "path")
                        .map(|p| text(&p, src).trim_matches('"').to_string())
                        .unwrap_or_default();
                    if path.is_empty() {
                        continue;
                    }
                    let alias = field(&spec, "name")
                        .map(|n| text(&n, src).to_string())
                        .unwrap_or_default();
                    let form = match alias.as_str() {
                        "_" => "blank_import",
                        "." => "dot_import",
                        _ => "package_import",
                    };
                    out.imports
                        .push(RawImport::new(path, "*", form, span_of(&spec)));
                }
            }
            "const_declaration" => {
                for spec in descendants(&kid, "const_spec") {
                    for name in names(&spec, src) {
                        declare(out, &spec, src, "", &name, SymbolKind::Const, policy);
                    }
                }
            }
            "var_declaration" => {
                for spec in descendants(&kid, "var_spec") {
                    for name in names(&spec, src) {
                        declare(out, &spec, src, "", &name, SymbolKind::Var, policy);
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
                        "struct_type" => SymbolKind::Struct,
                        "interface_type" => SymbolKind::Interface,
                        _ => SymbolKind::Type,
                    };
                    let full = declare(out, &spec, src, "", &name, kind, policy);
                    if let Some(body) = ty.filter(|t| t.kind() == "interface_type") {
                        for elem in kids(&body) {
                            match elem.kind() {
                                "method_spec" | "method_elem" => {
                                    if let Some(member) = name_of(&elem, src) {
                                        declare(
                                            out,
                                            &elem,
                                            src,
                                            &full,
                                            &member,
                                            SymbolKind::Method,
                                            policy,
                                        );
                                    }
                                }
                                // An embedded interface is a relationship, not a member: `Repo`
                                // satisfies everything `Bucket` does.
                                _ => {
                                    if let Some(base) = embedded_name(&elem, src) {
                                        out.supertypes.push(Supertype {
                                            of: full.clone(),
                                            base,
                                            kind: SupertypeKind::Interface,
                                            resolved: None,
                                            span: span_of(&elem),
                                        });
                                    }
                                }
                            }
                        }
                    }
                    if let Some(body) = ty.filter(|t| t.kind() == "struct_type") {
                        for field_node in descendants(&body, "field_declaration") {
                            if let Some(base) = embedded_name(&field_node, src) {
                                out.supertypes.push(Supertype {
                                    of: full.clone(),
                                    base,
                                    kind: SupertypeKind::Base,
                                    resolved: None,
                                    span: span_of(&field_node),
                                });
                            }
                        }
                    }
                }
            }
            "function_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    declare(out, &kid, src, "", &name, SymbolKind::Fn, policy);
                }
            }
            "method_declaration" => {
                let Some(name) = name_of(&kid, src) else {
                    continue;
                };
                let receiver = field(&kid, "receiver").map(|r| text(&r, src)).unwrap_or("");
                let owner = receiver_type(receiver);
                declare(out, &kid, src, &owner, &name, SymbolKind::Method, policy);
            }
            _ => {}
        }
    }
    if build_constrained {
        // Which `Init` this package has depends on the build tags of the file that declares it, and
        // a parser does not run the build. That is a scope whose declaration list the file cannot
        // enumerate, which is exactly the state s1 finding 1 is about — and it is Go's only one.
        out.mark_file_synthetic(synthetic::BUILD_CONSTRAINT);
        out.note("build_constraint", file_name(file).as_str());
    }
}

/// Push one declaration. `container` is "" for a file-level name and the receiver or interface name
/// for a member; the stored name is the dotted one, per the corpus's identity rule.
fn declare(
    out: &mut Draft,
    node: &Node,
    src: &[u8],
    container: &str,
    name: &str,
    kind: SymbolKind,
    policy: &ExtractPolicy,
) -> String {
    let full = qualify(container, name);
    let public = container.is_empty() && exports_by_name(name);
    out.symbols.push(Symbol {
        kind,
        name: full.clone(),
        container: container.to_string(),
        span: span_of(node),
        visibility: if public {
            Visibility::Public
        } else if container.is_empty() {
            Visibility::Private
        } else {
            // An exported method of an unexported type is nameable through the interface it
            // satisfies; the *type* is the gate, so the member is scoped, not public.
            Visibility::Scoped
        },
        exported: false,
        complexity: complexity(*node, policy),
        modifiers: modifiers(*node, name, src),
    });
    full
}

fn name_of(node: &Node, src: &[u8]) -> Option<String> {
    field(node, "name").map(|n| text(&n, src).trim_matches('"').to_string())
}

/// Go's export rule: the first rune is upper-case.
fn exports_by_name(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn modifiers(node: Node, name: &str, src: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    if name == "init" {
        out.push("init_func".to_string());
    }
    if name.starts_with("Test") || name.starts_with("Benchmark") {
        out.push("test_only".to_string());
    }
    // `Map[T any, U any](…)` declares type parameters; nothing else in Go does.
    if field(&node, "type_parameters").is_some() {
        out.push("generic".to_string());
    }
    if node.kind() == "method_declaration" {
        out.push(
            if receiver_is_pointer(node, src) {
                "pointer_receiver"
            } else {
                "value_receiver"
            }
            .to_string(),
        );
    }
    out.sort();
    out
}

/// `func (r *Rect) Scale(…)` versus `func (r Rect) Area(…)`: the difference is whether a call
/// mutates the receiver, which is the kind of thing a reader holds and a graph may want to say.
fn receiver_is_pointer(node: Node, src: &[u8]) -> bool {
    field(&node, "receiver").is_some_and(|receiver| text(&receiver, src).contains('*'))
}

fn complexity(node: Node, policy: &ExtractPolicy) -> Complexity {
    let span = span_of(&node);
    let (branches, _) = util::count_kinds(&node, BRANCH_KINDS, policy.max_complexity_nodes);
    let (loops, _) = util::count_kinds(&node, LOOP_KINDS, policy.max_complexity_nodes);
    let (returns, _) = util::count_kinds(&node, RETURN_KINDS, policy.max_complexity_nodes);
    let (nesting, _) = util::max_depth_of(&node, NEST_KINDS, policy.max_complexity_nodes);
    let params = field(&node, "parameters")
        .or_else(|| field(&node, "receiver"))
        .map(|parameters| kids(&parameters).len())
        .unwrap_or(0);
    let members = field(&node, "type_parameters")
        .map(|ty| kids(&ty).len())
        .unwrap_or(0);
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

/// `func (r *Rect) Scale(..)` and `func (Rect) Area()` → `Rect`. A generic receiver
/// (`func (s Set[T]) Add(..)`) loses its type parameter, which is the normalisation the symbol name
/// would have to use anyway.
fn receiver_type(receiver: &str) -> String {
    let trimmed = receiver
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
    tail.split(['[', '.']).next().unwrap_or(tail).to_string()
}

/// An embedded field or embedded interface element: a node with a type and no name of its own.
fn embedded_name(node: &Node, src: &[u8]) -> Option<String> {
    if name_of(node, src).is_some() {
        return None;
    }
    // An embedded interface element (`type_elem`) carries its type as an unnamed child, while an
    // embedded struct field (`field_declaration`) uses the `type` field. Both mean the same thing.
    let ty = match field(node, "type") {
        Some(ty) => ty,
        None if node.kind() == "type_elem" => kids(node).into_iter().next()?,
        None => return None,
    };
    match ty.kind() {
        "type_identifier" | "qualified_type_name" | "generic_type" => {
            Some(text(&ty, src).rsplit('.').next().unwrap_or("").to_string())
        }
        _ => None,
    }
}

fn names(spec: &Node, src: &[u8]) -> Vec<String> {
    let Some(name) = spec.child_by_field_name("name") else {
        return Vec::new();
    };
    match name.kind() {
        "field_identifier" | "identifier" | "constant_identifier" | "type_identifier" => {
            vec![text(&name, src).to_string()]
        }
        _ => kids(&name)
            .into_iter()
            .filter(|k| {
                matches!(
                    k.kind(),
                    "field_identifier" | "identifier" | "constant_identifier" | "type_identifier"
                )
            })
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

// ---------------------------------------------------------------------------
// resolution
// ---------------------------------------------------------------------------

/// Every import spec in the corpus is published, because the module path can only be deduced from
/// the whole set of them (see [`module_path`]).
pub fn seed(_file: &str, draft: &Draft, index: &mut Index) {
    for import in &draft.imports {
        index
            .extra
            .insert(format!("{SPEC_PREFIX}{}", import.spec), "1".to_string());
    }
}

/// The corpus's module path, inferred once and cached in the index.
///
/// `go.mod` is not one of the files the extractor is handed, and opening it here would make
/// [`crate::structure::extract`] a function of the filesystem rather than of its input. So the
/// module path is *deduced*: an import path is the module path joined to a corpus directory, so the
/// candidate that explains the most imports is the module path. When nothing explains anything — a
/// corpus of one orphan file, a vendor tree — the answer is `""`, which means "specs are already
/// corpus-relative"; an import that fits neither reading comes back `unresolved` rather than
/// invented.
fn module_path(index: &mut Index) -> String {
    if let Some(known) = index.extra.get(MODULE_KEY) {
        return known.clone();
    }
    let dirs: BTreeSet<String> = index
        .files
        .iter()
        .filter(|path| path.ends_with(".go") && !path.ends_with("_test.go"))
        .map(|path| dir_of(path))
        .collect();
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let specs: Vec<String> = index
        .extra
        .keys()
        .filter_map(|key| key.strip_prefix(SPEC_PREFIX))
        .map(|spec| spec.to_string())
        .collect();
    for spec in specs {
        for dir in &dirs {
            let matched = if dir.is_empty() {
                spec.is_empty()
            } else {
                spec == *dir || spec.ends_with(&format!("/{dir}"))
            };
            if !matched {
                continue;
            }
            let candidate = if dir.is_empty() || spec == *dir {
                String::new()
            } else {
                spec[..spec.len() - dir.len() - 1].to_string()
            };
            *counts.entry(candidate).or_default() += 1;
        }
    }
    let best = counts
        .into_iter()
        .max_by(|(left, left_count), (right, right_count)| {
            left_count
                .cmp(right_count)
                .then_with(|| left.len().cmp(&right.len()))
                .then_with(|| right.cmp(left))
        })
        .map(|(candidate, _)| candidate)
        .unwrap_or_default();
    index.extra.insert(MODULE_KEY.to_string(), best.clone());
    best
}

pub fn resolve(
    _file: &str,
    draft: &Draft,
    index: &mut Index,
    policy: &ExtractPolicy,
    truncated: &mut usize,
) -> Vec<Edge> {
    // Go has no re-export chain to follow, so the hop and pass bounds cannot bite here; the two
    // parameters stay in the signature because the engine hands every language the same five.
    let _ = (policy, truncated);
    let module = module_path(index);
    let mut out: Vec<Edge> = Vec::new();
    let mut seen: BTreeSet<(String, String, String)> = BTreeSet::new();
    for import in &draft.imports {
        let mut hit = false;
        if let Some(dir) = package_dir(&module, &import.spec) {
            for candidate in index.files.iter().filter(|path| {
                path.ends_with(".go") && !path.ends_with("_test.go") && dir_of(path) == dir
            }) {
                // The package, not the file: `Granularity::Package` is the label that keeps this
                // honest, and `package` carries the import path the source actually named.
                let mut edge = Edge::package(candidate.clone(), import.spec.clone());
                edge.form = import.form.to_string();
                if seen.insert((edge.target.clone(), edge.symbol.clone(), edge.form.clone())) {
                    out.push(edge);
                }
                hit = true;
            }
        }
        if !hit {
            out.push(Edge {
                target: String::new(),
                symbol: import.symbol.clone(),
                kind: EdgeKind::Unresolved,
                specifier: import.spec.clone(),
                via: None,
                form: import.form.to_string(),
                passes: 0,
                granularity: Granularity::File,
                package: None,
            });
        }
    }
    out.sort_by(|a, b| {
        (&a.target, &a.symbol, a.kind, &a.specifier).cmp(&(
            &b.target,
            &b.symbol,
            b.kind,
            &b.specifier,
        ))
    });
    out
}

/// Strip the module prefix from an import path. `None` means "not this repository's".
fn package_dir(module: &str, spec: &str) -> Option<String> {
    if module.is_empty() {
        return Some(spec.to_string());
    }
    if spec == module {
        return Some(String::new());
    }
    spec.strip_prefix(&format!("{module}/"))
        .map(|rest| rest.to_string())
}

/// Go has no re-export chain: a package's surface is what its files declare.
pub fn publish(_file: &str, _draft: &Draft, _edges: &[Edge], _index: &mut Index) {}

pub fn prepare_exports(_view: &CorpusView) -> BTreeMap<String, String> {
    BTreeMap::new()
}

/// Export = first rune upper-case, members excluded: a method is nameable only through its
/// receiver, so it is not API in the sense `e3.7` means.
pub fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    draft
        .symbols
        .iter()
        .filter(|symbol| symbol.container.is_empty() && exports_by_name(&symbol.name))
        .map(|symbol| symbol.name.clone())
        .collect()
}
