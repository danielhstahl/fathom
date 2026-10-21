//! The TypeScript extractor.
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/ts.rs`).
//! TypeScript was the language that scored 100 % on all three relations, and it was not easy: the
//! grammar puts the two most important tokens in a module system into *anonymous* children —
//! `export default` and `export * from` are marked by `default` and `*` tokens that a
//! `named_children()` walk cannot see at all, so a barrel file silently exports nothing and a default
//! export silently becomes a nameless declaration.
//!
//! Barrels also force resolution to be iterative: `index.ts → geo/index.ts → geo/point.ts` closes
//! only once the middle hop is known, which is s1 finding 3 and the reason
//! [`crate::structure::ExtractPolicy::max_resolution_passes`] is a config key.

use crate::structure::engine::{
    self, CorpusView, Draft, Index, Lang, RawImport, SEP, follow, publish_origin,
};
use crate::structure::model::{
    Complexity, EdgeKind, Language, Supertype, SupertypeKind, Symbol, SymbolKind, Visibility,
    synthetic,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{
    self, all_kids, dir_of, field, is_d_ts, join_dir, kids, qualify, span_of, text,
};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "else_clause",
    "switch_case",
    "switch_default",
    "conditional_expression",
    "catch_clause",
];
const LOOP_KINDS: &[&str] = &[
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
];
const RETURN_KINDS: &[&str] = &["return_statement", "throw_statement"];
const NEST_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "for_in_statement",
    "while_statement",
    "do_statement",
    "switch_body",
    "try_statement",
    "class_body",
];
/// Member kinds that are behaviour. Fields and class attributes are members for the complexity
/// proxy and never symbols, per the normalisation rules.
const MEMBER_KINDS: &[&str] = &[
    "method_definition",
    "method_signature",
    "abstract_method_signature",
    "function_signature",
    "public_field_definition",
    "field_definition",
];

pub fn grammar() -> TsLanguage {
    tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into()
}

pub const LANG: Lang = Lang {
    language: Language::TypeScript,
    grammar,
    scan,
    seed,
    resolve,
    publish,
    prepare_exports,
    export_names,
};

fn seed(_file: &str, _draft: &Draft, _index: &mut Index) {}
fn prepare_exports(_view: &CorpusView) -> BTreeMap<String, String> {
    BTreeMap::new()
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    if is_d_ts(file) {
        // A `.d.ts` is a stub by definition, and the stub is still the package's only declaration of
        // everything in it.
        out.aux.insert("ambient".to_string(), "1".to_string());
        out.note("ambient_declaration_file", file);
    }
    statements(root, src, "", out, policy);
}

fn name_of(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
}

/// Whether any direct child satisfies the predicate (named children only).
fn any_kid(node: &Node, predicate: impl Fn(Node) -> bool) -> bool {
    kids(node).into_iter().any(predicate)
}

fn has_token(node: &Node, kind: &str) -> bool {
    all_kids(node).iter().any(|k| k.kind() == kind)
}

fn complexity(node: &Node, policy: &ExtractPolicy) -> Complexity {
    let span = span_of(node);
    let (branches, _) = util::count_kinds(node, BRANCH_KINDS, policy.max_complexity_nodes);
    let (loops, _) = util::count_kinds(node, LOOP_KINDS, policy.max_complexity_nodes);
    let (returns, _) = util::count_kinds(node, RETURN_KINDS, policy.max_complexity_nodes);
    let (nesting, _) = util::max_depth_of(node, NEST_KINDS, policy.max_complexity_nodes);
    let params = field(node, "parameters")
        .map(|parameters| kids(&parameters).len())
        .unwrap_or(0);
    let members = field(node, "body")
        .map(|body| {
            kids(&body)
                .iter()
                .filter(|kid| MEMBER_KINDS.contains(&kid.kind()))
                .count()
        })
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

fn statements(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    for kid in kids(&node) {
        match kid.kind() {
            "export_statement" => export(kid, src, container, out, policy),
            "import_statement" | "import_require_clause" => import(kid, src, out),
            _ => declare(kid, src, container, false, false, out, policy),
        }
    }
}

fn import(node: Node, src: &[u8], out: &mut Draft) {
    let source = node
        .child_by_field_name("source")
        .map(|source| literal(&source, src))
        .unwrap_or_default();
    let mut names: Vec<String> = Vec::new();
    for kid in kids(&node) {
        if kid.kind() != "import_clause" {
            continue;
        }
        for part in kids(&kid) {
            match part.kind() {
                // A bare identifier in the clause is the default import, and the name the importer
                // writes has nothing to do with the name it resolves to.
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
        // `import './legacy';` — a side-effect dependency, with no symbol to name. A real edge of
        // unknown contents, not nothing.
        names.push("*".to_string());
    }
    for name in names {
        out.imports.push(RawImport::new(
            source.clone(),
            name,
            "import",
            span_of(&node),
        ));
    }
    if source.is_empty() {
        out.note("import_without_source", text(&node, src).trim());
    }
}

/// One declaration site, reached either bare or through `export`.
fn declare(
    node: Node,
    src: &[u8],
    container: &str,
    exported: bool,
    default: bool,
    out: &mut Draft,
    policy: &ExtractPolicy,
) {
    let kind = match node.kind() {
        "function_declaration" | "generator_function_declaration" => Some(SymbolKind::Fn),
        "class_declaration" | "abstract_class_declaration" => Some(SymbolKind::Class),
        "interface_declaration" => Some(SymbolKind::Interface),
        "type_alias_declaration" => Some(SymbolKind::Type),
        "enum_declaration" => Some(SymbolKind::Enum),
        _ => None,
    };
    if let Some(kind) = kind {
        let Some(declared) = name_of(&node, src) else {
            return;
        };
        let stored = if default {
            "default".to_string()
        } else {
            qualify(container, &declared)
        };
        let mut modifiers = modifiers(&node, src);
        if default {
            modifiers.push("default_export".to_string());
        }
        out.symbols.push(Symbol {
            kind,
            name: stored.clone(),
            container: container.to_string(),
            span: span_of(&node),
            visibility: if exported || container.is_empty() && default {
                Visibility::Public
            } else if any_kid(&node, |kid| kid.kind() == "accessibility_modifier") {
                Visibility::Scoped
            } else {
                Visibility::Public
            },
            exported: false,
            complexity: complexity(&node, policy),
            modifiers,
        });
        if container.is_empty() {
            out.aux.insert(format!("top:{stored}"), "1".into());
        }
        if default {
            out.aux.insert("has_default".into(), declared);
        }
        if (exported || default) && container.is_empty() {
            mark_export(out, &stored, default);
        }
        if matches!(kind, SymbolKind::Class | SymbolKind::Interface) {
            heritage(&node, src, &stored, out);
        }
        if matches!(kind, SymbolKind::Class | SymbolKind::Interface)
            && let Some(body) = node.child_by_field_name("body")
        {
            members(body, src, &stored, out, policy);
        }
        return;
    }
    match node.kind() {
        "ambient_declaration" => {
            // `declare function loadConfig(path: string): Config;` in a `.d.ts`, and
            // `declare module 'virtual:thing' { … }` anywhere: names this file merges into a module
            // it does not declare.
            for inner in kids(&node) {
                declare(inner, src, container, exported, default, out, policy);
            }
            out.note(
                "ambient_declaration",
                text(&node, src).split('{').next().unwrap_or("?").trim(),
            );
            out.mark_file_synthetic(synthetic::AMBIENT_MERGE);
        }
        "function_signature" => {
            // `export declare function loadConfig(path: string): Config;` in a `.d.ts` is the only
            // declaration of that symbol the package has.
            let Some(declared) = name_of(&node, src) else {
                return;
            };
            let stored = if default {
                "default".to_string()
            } else {
                qualify(container, &declared)
            };
            out.symbols.push(Symbol {
                kind: SymbolKind::Fn,
                name: stored.clone(),
                container: container.to_string(),
                span: span_of(&node),
                visibility: Visibility::Public,
                exported: false,
                complexity: complexity(&node, policy),
                modifiers: vec!["signature".to_string()],
            });
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
                let value_kind = value_kind_of(&declarator);
                let kind = if matches!(
                    value_kind.as_str(),
                    "arrow_function"
                        | "function_expression"
                        | "function"
                        | "generator_function"
                        | "generator_function_declaration"
                ) {
                    SymbolKind::Fn
                } else if node.kind() == "lexical_declaration"
                    && text(&node, src).starts_with("const")
                {
                    SymbolKind::Const
                } else {
                    SymbolKind::Var
                };
                let stored = qualify(container, &ident);
                let mut modifiers = Vec::new();
                if kind == SymbolKind::Fn {
                    modifiers.push("arrow_function".to_string());
                }
                out.symbols.push(Symbol {
                    kind,
                    name: stored.clone(),
                    container: container.to_string(),
                    span: span_of(&declarator),
                    visibility: Visibility::Public,
                    exported: false,
                    complexity: complexity(&declarator, policy),
                    modifiers,
                });
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
                    text(&node, src).split('{').next().unwrap_or("?").trim(),
                );
                out.mark_file_synthetic(synthetic::AMBIENT_MERGE);
                return;
            };
            let stored = qualify(container, &declared);
            out.symbols.push(Symbol {
                kind: SymbolKind::Module,
                name: stored.clone(),
                container: container.to_string(),
                span: span_of(&node),
                visibility: Visibility::Public,
                exported: false,
                complexity: complexity(&node, policy),
                modifiers: vec!["namespace".to_string()],
            });
            if container.is_empty() {
                out.aux.insert(format!("top:{stored}"), "1".into());
                if exported {
                    mark_export(out, &stored, false);
                }
            }
            if let Some(body) = node.child_by_field_name("body") {
                statements(body, src, &stored, out, policy);
            }
        }
        "expression_statement" => {
            for kid in kids(&node) {
                if kid.kind() == "call_expression" {
                    // A bare `register(fn)` call that creates an export is invisible to any parser,
                    // and saying so is cheaper than being wrong about the export list.
                    out.note(
                        "side_effect_call",
                        text(&kid, src).split('(').next().unwrap_or("?").trim(),
                    );
                }
            }
        }
        _ => {}
    }
}

fn value_kind_of(declarator: &Node) -> String {
    declarator
        .child_by_field_name("value")
        .map(|value| value.kind().to_string())
        .unwrap_or_default()
}

fn modifiers(node: &Node, src: &[u8]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let spelling = text(node, src);
    if spelling.starts_with("async") || spelling.contains("async ") {
        out.push("async".to_string());
    }
    if spelling.starts_with("abstract") {
        out.push("abstract".to_string());
    }
    if spelling.starts_with("declare") {
        out.push("declare".to_string());
    }
    if spelling.starts_with("function*") || spelling.contains("function*") {
        out.push("generator".to_string());
    }
    let mut decorated = false;
    for kid in kids(node) {
        if kid.kind() == "decorator" {
            decorated = true;
            let name = text(&kid, src).trim_start_matches('@').to_string();
            let name = name.split('(').next().unwrap_or(name.as_str()).to_string();
            out.push(format!("decorated_by:{name}"));
        }
    }
    if decorated || parents_decorated(node) {
        out.push("decorated".to_string());
    }
    out.sort();
    out.dedup();
    out
}

/// `class Base implements Shape`, `class Circle extends Shape`, `interface A extends B`.
fn heritage(node: &Node, src: &[u8], of: &str, out: &mut Draft) {
    for kid in kids(node) {
        match kid.kind() {
            "class_heritage" => {
                for clause in kids(&kid) {
                    match clause.kind() {
                        "extends_clause" => {
                            for base in all_kids(&clause) {
                                if base.kind() == "type_arguments" {
                                    continue;
                                }
                                let spelling = text(&base, src).trim().to_string();
                                if spelling.is_empty() {
                                    continue;
                                }
                                out.supertypes.push(Supertype {
                                    of: of.to_string(),
                                    base: spelling,
                                    kind: SupertypeKind::Base,
                                    resolved: None,
                                    span: span_of(&base),
                                });
                            }
                        }
                        "implements_clause" => {
                            for base in kids(&clause) {
                                let spelling = text(&base, src).trim().to_string();
                                if spelling.is_empty() {
                                    continue;
                                }
                                out.supertypes.push(Supertype {
                                    of: of.to_string(),
                                    base: spelling,
                                    kind: SupertypeKind::Interface,
                                    resolved: None,
                                    span: span_of(&base),
                                });
                            }
                        }
                        _ => {}
                    }
                }
                if decorated(node, src) {
                    out.mark_synthetic(of, synthetic::DECORATED);
                }
            }
            "extends_type_clause" => {
                for base in all_kids(&kid) {
                    if base.kind() == "type_parameters" {
                        continue;
                    }
                    let spelling = text(&base, src).trim().to_string();
                    if spelling.is_empty() {
                        continue;
                    }
                    out.supertypes.push(Supertype {
                        of: of.to_string(),
                        base: spelling,
                        kind: SupertypeKind::Interface,
                        resolved: None,
                        span: span_of(&base),
                    });
                }
            }
            _ => {}
        }
    }
}

fn decorated(node: &Node, src: &[u8]) -> bool {
    field(node, "decorator").is_some()
        || any_kid(node, |kid| kid.kind() == "decorator")
        || text(node, src).starts_with('@')
        || parents_decorated(node)
}

/// `@sealed export class Circle` hangs the decorator off the `export_statement`, not the class.
fn parents_decorated(node: &Node) -> bool {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "export_statement" && text_is_decorated(&parent) {
            return true;
        }
        current = parent.parent();
    }
    false
}

fn text_is_decorated(node: &Node) -> bool {
    kids(node).iter().any(|kid| kid.kind() == "decorator")
}

fn mark_export(out: &mut Draft, name: &str, default: bool) {
    if default {
        out.aux.insert("export:default".into(), "1".into());
    } else {
        out.aux.insert(format!("export:{name}"), "1".into());
    }
}

fn members(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let mut names: BTreeMap<String, usize> = BTreeMap::new();
    for kid in kids(&node) {
        let kind = match kid.kind() {
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                SymbolKind::Method
            }
            _ => continue,
        };
        let Some(name) = name_of(&kid, src) else {
            continue;
        };
        if name == "constructor" {
            // A constructor is not an API surface in any of the six languages, and the corpus
            // annotates it that way.
            continue;
        }
        let stored = qualify(container, &name);
        *names.entry(stored.clone()).or_insert(0) += 1;
        let mut modifiers: Vec<String> = Vec::new();
        let spelling = text(&kid, src);
        if spelling.starts_with("static") {
            modifiers.push("static".to_string());
        }
        if spelling.starts_with("abstract") {
            modifiers.push("abstract".to_string());
        }
        if spelling.starts_with("get ") {
            modifiers.push("getter".to_string());
        }
        if spelling.starts_with("set ") {
            modifiers.push("setter".to_string());
        }
        for access in kids(&kid) {
            if access.kind() == "accessibility_modifier" {
                match text(&access, src) {
                    "private" | "protected" => modifiers.push("non_public".to_string()),
                    "readonly" => modifiers.push("readonly".to_string()),
                    _ => {}
                }
            }
            if access.kind() == "decorator" {
                modifiers.push("decorated".to_string());
                out.mark_synthetic(&stored, synthetic::DECORATED);
            }
        }
        modifiers.sort();
        modifiers.dedup();
        out.symbols.push(Symbol {
            kind,
            name: stored,
            container: container.to_string(),
            span: span_of(&kid),
            visibility: if modifiers.iter().any(|m| m == "non_public") {
                Visibility::Scoped
            } else {
                Visibility::Public
            },
            exported: false,
            complexity: complexity(&kid, policy),
            modifiers,
        });
    }
    // Declaration merging (`interface Config` twice in one file) is one symbol with two
    // declarations: the only reading that keeps "what uses `Config`?" a single answer.
    for (name, _) in names.iter().filter(|(_, count)| **count > 1) {
        out.note("declaration_merged", name);
    }
}

fn export(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let is_default = has_token(&node, "default");
    let source = node.child_by_field_name("source").map(|s| literal(&s, src));
    let mut names: Vec<(String, String)> = Vec::new(); // (source name, exported name)
    let mut wildcard = false;
    let decorated = text_is_decorated(&node);
    for kid in all_kids(&node) {
        match kid.kind() {
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
            "namespace_export" | "*" => wildcard = true,
            _ => {}
        }
    }
    if let Some(decl) = node.child_by_field_name("declaration") {
        if decorated {
            // A decorated class: the decorator may add members the file does not contain. Mark the
            // declared scope, which `declare` is about to create.
            if let Some(name) = name_of(&decl, src) {
                out.mark_synthetic(&qualify(container, &name), synthetic::DECORATED);
            }
        }
        declare(decl, src, container, true, is_default, out, policy);
        return;
    }
    if is_default && node.child_by_field_name("declaration").is_none() {
        // `export default Point;` sells an existing symbol under the name `default`; the declaring
        // file keeps the definition and this file owns only the exported name.
        out.aux.insert("export:default".into(), "1".into());
        out.aux.insert("default_is_local".into(), "1".into());
        return;
    }
    match (source, wildcard) {
        (Some(spec), true) => {
            // `export * from './y'`: this file's export list is now whatever y publishes, which the
            // extractor cannot enumerate. Marked, not silently partial.
            let mut import = RawImport::new(spec, "*", "reexport_star", span_of(&node));
            import.reexport = true;
            out.imports.push(import);
            out.mark_file_synthetic(synthetic::WILDCARD_REEXPORT);
        }
        (Some(spec), false) => {
            for (local, exported) in names {
                let mut import = RawImport::new(spec.clone(), local, "reexport", span_of(&node));
                import.reexport = true;
                out.aux
                    .insert(format!("renamed:{exported}"), import.symbol.clone());
                out.imports.push(import);
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

pub fn resolve(
    file: &str,
    draft: &Draft,
    index: &mut Index,
    policy: &ExtractPolicy,
    truncated: &mut usize,
) -> Vec<engine::Edge> {
    let mut out: Vec<engine::Edge> = Vec::new();
    let dir = dir_of(file);
    for import in &draft.imports {
        let spec = import.spec.clone();
        if spec.is_empty() {
            continue;
        }
        let Some(target) = resolve_relative(index, &dir, &spec) else {
            out.push(engine::Edge {
                target: String::new(),
                symbol: import.symbol.clone(),
                kind: EdgeKind::Unresolved,
                specifier: spec.clone(),
                via: None,
                form: import.form.to_string(),
                passes: 0,
            });
            continue;
        };
        if import.symbol == "*" {
            let mut edge = engine::Edge::wildcard(target);
            edge.form = import.form.to_string();
            edge.specifier = spec.clone();
            edge.passes = 1;
            out.push(edge);
            continue;
        }
        match follow(index, &target, &import.symbol, policy.max_resolution_hops) {
            Some(chain) => {
                let mut edge = engine::Edge::resolved(chain.target, chain.symbol);
                edge.via = chain.via;
                edge.passes = chain.hops;
                edge.form = import.form.to_string();
                edge.specifier = spec.clone();
                if chain.truncated {
                    *truncated += 1;
                }
                out.push(edge);
            }
            None => {
                out.push(engine::Edge {
                    target: String::new(),
                    symbol: import.symbol.clone(),
                    kind: EdgeKind::Unresolved,
                    specifier: format!("{spec}:{}", import.symbol),
                    via: None,
                    form: import.form.to_string(),
                    passes: 0,
                });
            }
        }
    }
    out
}

/// `./geo`, `../shapes`, `./types` → a file in the corpus. Bare specifiers are dependencies on
/// something outside the repository, which is a stated state, not a missing edge.
fn resolve_relative(index: &Index, dir: &str, spec: &str) -> Option<String> {
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
    let candidates = [
        join_dir(&base, &format!("{root}.ts")),
        join_dir(&base, &format!("{root}.d.ts")),
        join_dir(&base, &format!("{root}/index.ts")),
        join_dir(&base, &format!("{root}/index.d.ts")),
    ];
    candidates
        .into_iter()
        .find(|candidate| index.has(candidate))
}

/// What this file makes available to an importer, following re-export chains as they become known.
pub fn publish(file: &str, draft: &Draft, edges: &[engine::Edge], index: &mut Index) {
    for key in draft.aux.keys().filter(|key| key.starts_with("export:")) {
        let name = key.trim_start_matches("export:");
        publish_origin(index, file, name, file, name);
    }
    let renamed: Vec<(String, String)> = draft
        .aux
        .iter()
        .filter_map(|(key, value)| {
            key.strip_prefix("renamed:")
                .map(|exported| (exported.to_string(), value.clone()))
        })
        .collect();
    for edge in edges {
        if edge.kind == EdgeKind::Unresolved {
            continue;
        }
        if edge.symbol == "*" {
            // `export * from './y'`: whatever y publishes, this file publishes too, under the same
            // name, attributed to y. Round by round the barrel fills in.
            let prefix = format!("{}{SEP}", edge.target);
            let copies: Vec<(String, String)> = index
                .origin
                .iter()
                .filter(|(key, _)| key.starts_with(&prefix))
                .map(|(key, value)| {
                    (
                        format!("{file}{SEP}{}", &key[prefix.len()..]),
                        value.clone(),
                    )
                })
                .collect();
            for (key, value) in copies {
                index.origin.insert(key, value);
            }
            continue;
        }
        // A re-export of `local as exported` publishes the origin under the exported name.
        let mut published = false;
        for (exported, local) in &renamed {
            if local == &edge.symbol {
                publish_origin(index, file, exported, &edge.target, &edge.symbol);
                published = true;
            }
        }
        if !published && !renamed.iter().any(|(_, local)| local == &edge.symbol) {
            publish_origin(index, file, &edge.symbol, &edge.target, &edge.symbol);
        }
    }
}

fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    draft
        .aux
        .keys()
        .filter_map(|key| key.strip_prefix("export:"))
        .map(str::to_string)
        .collect()
}
