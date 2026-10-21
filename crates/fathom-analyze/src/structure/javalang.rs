//! The Java extractor (`e3.1b`, `reco-dlp.1`).
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/javalang.rs`) with
//! the spans, complexity, supertypes and cannot-enumerate marks a spike skips, and with the
//! mechanism `e0.12` attached to this ticket for it: **the same-package scan, scoped and measured at
//! repository scale**.
//!
//! Java is the language where the honest dependency has no syntax. `sealed interface Shape permits
//! Circle` in `Shape.java` is a real edge with no `import` to read, because both types are in the
//! same package and Java does not need one. `s1` found 14 of those 14 with zero false positives —
//! on a ten-file corpus, scanning every type reference it could see. Its finding 5 is the warning
//! that goes with that result: on a 4 000-file repository the same technique resolves `Builder`,
//! `Result` and `Config` to the wrong package, so `e3.2` was not allowed to draw same-package edges
//! until somebody re-measured on a corpus of at least 1 000 files and published both a false-positive
//! rate and a budget. That measurement is
//! [`benchmarks/reports/e3-1-structure-core.json`](../../../../benchmarks/reports/e3-1-structure-core.json)'s
//! `java.same_package` block, and the scope it measures is a configuration key
//! (`extract.java.same_package_scan`) with two settings:
//!
//! * [`ScanScope::ImportsPlusPackage`] (the shipped default) — a type reference this file does not
//!   already import by name, resolved against the package the file declares and nowhere else. The
//!   head of a qualified reference (`Outer.Inner`) counts only when it is type-shaped, because a
//!   lower-case head is a *package* name and resolving that against the file's own package is
//!   precisely the invention s1 warned about.
//! * [`ScanScope::AllTypeReferences`] — what the spike ran: every `type_identifier`, resolved
//!   against anything in the corpus that declares that simple name. Selectable so the cost of it
//!   stays a number in the report rather than a paragraph nobody re-runs.
//!
//! The other three Java rules the corpus commits to: **`public` is not the export authority inside
//! a module** (`module-info.java` is, so `com.acme.app.legacy` is nameable inside the module and not
//! outside it); **a record's accessors are not in the file** (the component list is, the accessors
//! are `synthesised_accessors`); and **a wildcard import is a dependency of unknown contents**, which
//! fans out per file of the package with symbol `*` and is exactly the case `EdgeKind::Wildcard`
//! exists for.

use crate::structure::engine::{CorpusView, Draft, Index, Lang, RawImport};
use crate::structure::model::{
    Complexity, Edge, EdgeKind, Granularity, Language, Supertype, SupertypeKind, Symbol,
    SymbolKind, Visibility, synthetic,
};
use crate::structure::policy::{ExtractPolicy, ScanScope};
use crate::structure::util::{self, field, kids, qualify, span_of, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

pub fn grammar() -> TsLanguage {
    tree_sitter_java::LANGUAGE.into()
}

pub const LANG: Lang = Lang {
    language: Language::Java,
    grammar,
    scan,
    seed,
    resolve,
    publish,
    prepare_exports,
    export_names,
};

const TYPE_KINDS: &[&str] = &[
    "class_declaration",
    "interface_declaration",
    "enum_declaration",
    "record_declaration",
    "annotation_type_declaration",
];

/// Index keys, namespaced as the engine's `Index::extra` contract asks.
const PACKAGE_PREFIX: &str = "java:pkg:";
const EXPORTS_KEY: &str = "java:module_exports";
const HAS_MODULE_KEY: &str = "java:has_module";
/// Draft (per-file) aux keys.
const REF_PREFIX: &str = "type:";
const QUALIFIED_REF_PREFIX: &str = "qtype:";
const IMPORTED_PREFIX: &str = "imported:";
const DECLARED_PREFIX: &str = "declared:";
const BOUND_PREFIX: &str = "bound:";

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

pub fn scan(root: Node, src: &[u8], _file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    statements(root, src, "", out, policy);
    // A same-package reference has no import statement to read, so the names this file mentions are
    // collected now and resolved later, against the corpus-wide package index. Two sets, because the
    // shipped scope is the narrow one and the spike's scope was the wide one, and the difference
    // between them is the number `e3.2` was told to wait for.
    let mut refs = BTreeSet::new();
    references(root, src, &mut refs, false);
    for (name, qualified) in refs {
        let key = if qualified {
            format!("{QUALIFIED_REF_PREFIX}{name}")
        } else {
            format!("{REF_PREFIX}{name}")
        };
        out.aux.insert(key, "1".to_string());
    }
    // Names bound as type parameters, or declared as local classes inside a method. Inside their
    // scope they *win* over a same-package type of the same spelling (JLS §6.5.5), so a mention of
    // one is not a dependency: `<Amount> int bound(Amount value)` names a `Amount.java` in the file's
    // own package to a scanner that only counts words, and there is no such file in the sense that
    // matters. Collected per file rather than per scope — a file that binds `<Result>` and also names
    // the package's `Result` elsewhere loses that edge, which is the cheap direction: a missing edge
    // is a hole, an invented one is a wrong answer, and `e3.2` can survive a hole.
    for name in bound_names(root, src) {
        out.aux
            .insert(format!("{BOUND_PREFIX}{name}"), "1".to_string());
    }
    for symbol in &out.symbols {
        if symbol.container.is_empty() {
            let simple = symbol
                .name
                .rsplit('.')
                .next()
                .unwrap_or(&symbol.name)
                .to_string();
            out.aux
                .insert(format!("{DECLARED_PREFIX}{simple}"), "1".to_string());
        }
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
                        out.mark_synthetic(&name, synthetic::SYNTHESISED_ACCESSORS);
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
    field(node, "name").map(|n| text(&n, src).to_string())
}

fn statements(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    for kid in kids(&node) {
        if TYPE_KINDS.contains(&kid.kind()) {
            declare_type(kid, src, container, out, policy);
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
                out.unit = Some(raw.clone());
                out.aux.insert("package".to_string(), raw);
            }
            "module_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    push(
                        out,
                        &name,
                        "",
                        SymbolKind::Module,
                        Visibility::Public,
                        kid,
                        src,
                        policy,
                    );
                }
                let body = field(&kid, "body").unwrap_or(kid);
                for directive in kids(&body) {
                    if directive.kind() != "exports_module_directive" {
                        continue;
                    }
                    if let Some(pkg) = kids(&directive)
                        .iter()
                        .find(|k| k.kind() == "scoped_identifier" || k.kind() == "identifier")
                        .map(|k| text(k, src).to_string())
                    {
                        let key = out
                            .aux
                            .get(EXPORTS_KEY)
                            .map(|old| format!("{old},{pkg}"))
                            .unwrap_or_else(|| pkg.clone());
                        out.aux.insert(EXPORTS_KEY.to_string(), key);
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
                let static_import = raw.starts_with("static ");
                let body = raw.strip_prefix("static ").unwrap_or(&raw);
                let (form, package, symbol, owner) = if let Some(wildcard) = body.strip_suffix(".*")
                {
                    (
                        "wildcard_import",
                        wildcard.to_string(),
                        "*".to_string(),
                        String::new(),
                    )
                } else {
                    let last = body.rsplit('.').next().unwrap_or("").to_string();
                    let mut package = body[..body.len() - last.len()]
                        .trim_end_matches('.')
                        .to_string();
                    let (symbol, owner) = if static_import {
                        // `import static a.b.Geom.area` names a member of a type: the type is the
                        // second-to-last segment, and the edge has to point at the member or every
                        // `area` in the corpus looks like the same dependency.
                        let ty = package.rsplit('.').next().unwrap_or("").to_string();
                        package = package[..package.len() - ty.len()]
                            .trim_end_matches('.')
                            .to_string();
                        (last, ty)
                    } else {
                        (last.clone(), String::new())
                    };
                    (
                        if static_import {
                            "static_import"
                        } else {
                            "single_import"
                        },
                        package,
                        symbol,
                        owner,
                    )
                };
                if !package.is_empty() {
                    out.aux
                        .insert(format!("{IMPORTED_PREFIX}{symbol}"), "1".to_string());
                    let mut import = RawImport::new(package.clone(), symbol, form, span_of(&kid));
                    // The second slot carries the type a static import reached through.
                    import.module = vec![package, owner];
                    out.imports.push(import);
                }
            }
            _ => {}
        }
    }
}

fn declare_type(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let Some(name) = name_of(&node, src) else {
        return;
    };
    let kind = match node.kind() {
        "interface_declaration" | "annotation_type_declaration" => SymbolKind::Interface,
        "enum_declaration" => SymbolKind::Enum,
        _ => SymbolKind::Class,
    };
    let stored = qualify(container, &name);
    let visibility = visibility(node, src);
    push(out, &name, container, kind, visibility, node, src, policy);
    if visibility == Visibility::Public {
        out.aux.insert(format!("public:{stored}"), "1".to_string());
    }
    for child in kids(&node) {
        let base_kind = match child.kind() {
            "superclass" => Some(SupertypeKind::Base),
            // `implements C` parses as `super_interfaces` (the field is called `interfaces`) and
            // `interface I extends J` as `extends_interfaces`; both are contracts, not ancestry.
            "super_interfaces" | "extends_interfaces" | "interfaces" => {
                Some(SupertypeKind::Interface)
            }
            // `permits` lists the *sub*types of a sealed type — the opposite edge, and an
            // `imports`-relation fact (the spike annotated those as edges), not a supertype row.
            _ => None,
        };
        if let Some(kind) = base_kind {
            let mut found = BTreeSet::new();
            references(child, src, &mut found, true);
            for (base, _) in found {
                out.supertypes.push(Supertype {
                    of: stored.clone(),
                    base,
                    kind,
                    resolved: None,
                    span: span_of(&child),
                });
            }
        }
    }
    if let Some(body) = field(&node, "body") {
        members(body, src, &stored, out, policy);
    }
}

fn members(node: Node, src: &[u8], container: &str, out: &mut Draft, policy: &ExtractPolicy) {
    for kid in kids(&node) {
        if TYPE_KINDS.contains(&kid.kind()) {
            declare_type(kid, src, container, out, policy);
            continue;
        }
        match kid.kind() {
            "annotation_type_element_declaration" | "method_declaration" => {
                if let Some(name) = name_of(&kid, src) {
                    push(
                        out,
                        &name,
                        container,
                        SymbolKind::Method,
                        visibility(kid, src),
                        kid,
                        src,
                        policy,
                    );
                }
            }
            "constructor_declaration" | "compact_constructor_declaration" => {
                // Constructors are not symbols; that is the rule, not a miss. Saying which ones were
                // skipped is what keeps the rule distinguishable from a gap in the extractor.
                out.note("constructor", container);
            }
            _ => {
                if matches!(
                    kid.kind(),
                    "class_body" | "interface_body" | "enum_body" | "enum_body_declarations"
                ) {
                    members(kid, src, container, out, policy);
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn push(
    out: &mut Draft,
    name: &str,
    container: &str,
    kind: SymbolKind,
    visibility: Visibility,
    node: Node,
    src: &[u8],
    policy: &ExtractPolicy,
) {
    out.symbols.push(Symbol {
        kind,
        name: qualify(container, name),
        container: container.to_string(),
        span: span_of(&node),
        visibility,
        exported: false,
        complexity: complexity(&node, policy),
        modifiers: modifiers(&node, src),
    });
}

/// The modifiers worth keeping: the spellings that change what a reader expects from a declaration.
fn modifiers(node: &Node, src: &[u8]) -> Vec<String> {
    let spelling: String = kids(node)
        .into_iter()
        .filter(|kid| kid.kind() == "modifiers")
        .map(|kid| text(&kid, src).to_string())
        .collect::<Vec<String>>()
        .join(" ");
    let mut out: Vec<String> = Vec::new();
    for word in [
        "public",
        "protected",
        "private",
        "static",
        "final",
        "abstract",
        "default",
        "sealed",
        "non-sealed",
        "synchronized",
        "native",
        "transient",
        "volatile",
        "strictfp",
    ] {
        if spelling
            .split_whitespace()
            .any(|token| trim_token(token) == word)
        {
            out.push(word.replace('-', "_"));
        }
    }
    if kids(node)
        .iter()
        .any(|kid| kid.kind() == "marker_annotation" || kid.kind() == "annotation")
    {
        out.push("annotated".to_string());
    }
    out.sort();
    out
}

fn trim_token(token: &str) -> &str {
    token.trim_end_matches(';')
}

fn visibility(node: Node, src: &[u8]) -> Visibility {
    let spelling: String = kids(&node)
        .into_iter()
        .filter(|kid| kid.kind() == "modifiers")
        .map(|kid| text(&kid, src).to_string())
        .collect::<Vec<String>>()
        .join(" ");
    let words: Vec<&str> = spelling.split_whitespace().map(trim_token).collect();
    if words.contains(&"public") {
        Visibility::Public
    } else if words.contains(&"protected") {
        // Nameable in the package and in every subclass: `Scoped`, which is the same answer
        // `pub(super)` gets in Rust.
        Visibility::Scoped
    } else if words.contains(&"private") {
        Visibility::Private
    } else {
        // Package-private — Java's default, and the reason "is this public?" cannot be answered by
        // looking for a keyword alone.
        Visibility::Scoped
    }
}

/// Type references, split by whether they were reached through a qualifier.
///
/// `scoped_identifier` and `scoped_type_identifier` are where Java puts `Outer.Inner` and
/// `sub.Builder`, and the two mean opposite things: the first is a real same-package dependency on
/// `Outer.java`, the second names a type the file can only reach through an import. They are told
/// apart the way Java itself tells them apart — a type name starts upper-case and a package name does
/// not — and the head of the chain decides for the whole chain. That rule is load-bearing, not
/// tidying: recording the *tail* of `pricing.Result` as a simple name is what made the same-package
/// scan claim an edge to a `Result.java` the source never mentioned, which is s1 finding 5 arriving
/// through a door nobody had shut.
fn references(node: Node, src: &[u8], out: &mut BTreeSet<(String, bool)>, qualified: bool) {
    for kid in kids(&node) {
        match kid.kind() {
            "scoped_identifier" | "scoped_type_identifier" => {
                let chain = text(&kid, src).replace(' ', "");
                let head = chain.split('.').next().unwrap_or_default().to_string();
                if name_is_type_shaped(&head) {
                    // `Outer.Inner` is a reference to `Outer`, in this file's own package.
                    out.insert((head, false));
                } else {
                    // `sub.Builder` is a reference to a type this file imported by name (or failed
                    // to); as a *chain* it is still a dependency worth reporting under the wide scan,
                    // and as nothing else under the narrow one.
                    out.insert((chain, true));
                }
                // The segments are accounted for; recursing would re-report the tail as a bare name.
            }
            "type_identifier" => {
                out.insert((text(&kid, src).to_string(), qualified));
                references(kid, src, out, qualified);
            }
            _ => references(kid, src, out, qualified),
        }
    }
}

/// Whether a reference head is a type name by Java's own convention: types are UpperCamelCase and
/// packages are lower-case, and the one place that convention is load-bearing is telling `Outer.Inner`
/// (a same-package dependency) from `sub.Builder` (a package the file has to import through).
/// Names this file binds that win over a same-package type of the same spelling: method and class
/// type parameters. Sorted out at file grain on purpose — see the note in [`scan`].
fn bound_names(node: Node, src: &[u8]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut stack = vec![node];
    while let Some(current) = stack.pop() {
        if current.kind() == "type_parameter"
            // tree-sitter-java gives a type parameter no `name` field: the identifier is a plain
            // `type_identifier` child, and the only one that is not a bound.
            && let Some(name) = kids(&current)
                .into_iter()
                .find(|kid| kid.kind() == "type_identifier")
                .map(|n| text(&n, src).trim().to_string())
                .filter(|name| name_is_type_shaped(name))
        {
            out.insert(name);
        }
        stack.extend(kids(&current));
    }
    out
}

fn name_is_type_shaped(name: &str) -> bool {
    name.chars().next().is_some_and(char::is_uppercase)
}

fn complexity(node: &Node, policy: &ExtractPolicy) -> Complexity {
    let span = span_of(node);
    let (branches, _) = util::count_kinds(
        node,
        &[
            "if_statement",
            "switch_block_statement_group",
            "switch_rule",
            "ternary_expression",
            "catch_clause",
        ],
        policy.max_complexity_nodes,
    );
    let (loops, _) = util::count_kinds(
        node,
        &[
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
        ],
        policy.max_complexity_nodes,
    );
    let (returns, _) = util::count_kinds(
        node,
        &[
            "return_statement",
            "break_statement",
            "continue_statement",
            "throw_statement",
        ],
        policy.max_complexity_nodes,
    );
    let (nesting, _) = util::max_depth_of(
        node,
        &[
            "if_statement",
            "for_statement",
            "enhanced_for_statement",
            "while_statement",
            "do_statement",
            "try_statement",
            "switch_expression",
        ],
        policy.max_complexity_nodes,
    );
    let params = field(node, "parameters")
        .or_else(|| field(node, "type_parameters"))
        .map(|parameters| kids(&parameters).len())
        .unwrap_or(0);
    let members = kids(node)
        .into_iter()
        .filter(|kid| {
            matches!(
                kid.kind(),
                "method_declaration" | "constructor_declaration" | "class_declaration"
            )
        })
        .count();
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

// ---------------------------------------------------------------------------
// resolution
// ---------------------------------------------------------------------------

/// Publish this file's package, which is the only fact the corpus-wide same-package rule needs.
pub fn seed(file: &str, draft: &Draft, index: &mut Index) {
    if let Some(package) = draft.aux.get("package") {
        index
            .extra
            .insert(format!("{PACKAGE_PREFIX}{file}"), package.clone());
    }
}

pub fn resolve(
    file: &str,
    draft: &Draft,
    index: &mut Index,
    policy: &ExtractPolicy,
    truncated: &mut usize,
) -> Vec<Edge> {
    let _ = truncated;
    let mut out: Vec<Edge> = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();

    // 1. The imports, as written.
    for import in &draft.imports {
        let package = import.module.first().cloned().unwrap_or_default();
        let owner = import.module.get(1).filter(|owner| !owner.is_empty());
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for (other, pkg) in &index.extra {
            let Some(path) = other.strip_prefix(PACKAGE_PREFIX) else {
                continue;
            };
            if pkg != &package || path == file {
                continue;
            }
            let names_it = if import.symbol == "*" {
                // A wildcard import is a dependency on the *package*, so it fans out to every file
                // of it — contents unnamed, which is what `Wildcard` means.
                true
            } else {
                index.defines(path, &import.symbol)
                    || owner.is_some_and(|owner| {
                        index.defines(path, &format!("{}.{owner}", import.symbol))
                            || index.defines(path, owner)
                    })
            };
            if names_it {
                hits.insert(path.to_string());
            }
        }
        if hits.is_empty() {
            out.push(Edge {
                target: String::new(),
                symbol: import.symbol.clone(),
                kind: EdgeKind::Unresolved,
                specifier: format!("{package}.{}", import.symbol),
                via: None,
                form: import.form.to_string(),
                passes: 0,
                granularity: Granularity::File,
                package: None,
            });
            continue;
        }
        for target in hits {
            if !seen.insert((target.clone(), import.symbol.clone())) {
                continue;
            }
            let mut edge = if import.symbol == "*" {
                Edge::wildcard(target)
            } else {
                let (origin, symbol) = index.origin_of(&target, &import.symbol);
                Edge::resolved(origin, symbol)
            };
            edge.form = import.form.to_string();
            edge.passes = 1;
            out.push(edge);
        }
    }

    // 2. The dependencies with no statement: same-package type references.
    let package = match draft.aux.get("package") {
        Some(package) => package.clone(),
        None => return out,
    };
    let wide = policy.java_same_package_scan == ScanScope::AllTypeReferences;
    let mut candidates: BTreeSet<String> = BTreeSet::new();
    for key in draft.aux.keys() {
        if let Some(name) = key.strip_prefix(REF_PREFIX) {
            candidates.insert(name.to_string());
        }
        if wide && let Some(name) = key.strip_prefix(QUALIFIED_REF_PREFIX) {
            candidates.insert(name.to_string());
        }
    }
    if wide {
        // The spike's scan: every reference in the tree, qualified heads included, and an import
        // does not shadow anything. Kept selectable so the false-positive cost of that reading is a
        // number in the report rather than a paragraph nobody re-runs.
        candidates.extend(
            draft
                .aux
                .keys()
                .filter_map(|key| key.strip_prefix(DECLARED_PREFIX))
                .map(|name| name.to_string()),
        );
    }
    for name in candidates {
        if !wide && !name_is_type_shaped(&name) {
            // A lower-case head is a package name (`sub.Builder`). It is still a *reference*, which
            // is why it is collected, but the same-package rule has nothing to say about it.
            continue;
        }
        if !wide
            && policy.java_bound_names
            && draft.aux.contains_key(&format!("{BOUND_PREFIX}{name}"))
        {
            // Bound by a type parameter or a local declaration in this file: a mention, not a use.
            continue;
        }
        if !wide && draft.aux.contains_key(&format!("{IMPORTED_PREFIX}{name}")) {
            // A single-type import shadows a same-package name of the same spelling (JLS §6.5.5),
            // so the import half above already owns this reference. Counting it twice would be a
            // false positive with a compile error behind it.
            continue;
        }
        let mut hits: BTreeSet<String> = BTreeSet::new();
        for (other, pkg) in &index.extra {
            let Some(path) = other.strip_prefix(PACKAGE_PREFIX) else {
                continue;
            };
            if path == file {
                continue;
            }
            let in_scope = if wide { true } else { pkg == &package };
            if in_scope && index.defines(path, &name) {
                hits.insert(path.to_string());
            }
        }
        for target in hits {
            if !seen.insert((target.clone(), name.clone())) {
                continue;
            }
            let mut edge = Edge::resolved(target, name.clone());
            edge.form = "same_package".to_string();
            out.push(edge);
        }
    }
    out
}

/// Java has no re-export chain; a package's surface is what its files declare.
pub fn publish(_file: &str, _draft: &Draft, _edges: &[Edge], _index: &mut Index) {}

/// Derive the module's exported packages once, from `module-info.java` if the corpus has one.
pub fn prepare_exports(view: &CorpusView) -> BTreeMap<String, String> {
    let mut derived: BTreeMap<String, String> = BTreeMap::new();
    let mut packages: BTreeSet<String> = BTreeSet::new();
    let mut has_module = false;
    for draft in view.drafts.values() {
        if let Some(list) = draft.aux.get(EXPORTS_KEY) {
            has_module = true;
            for package in list.split(',') {
                packages.insert(package.to_string());
            }
        }
    }
    if has_module {
        derived.insert(
            HAS_MODULE_KEY.to_string(),
            packages.into_iter().collect::<Vec<_>>().join(","),
        );
    }
    derived
}

/// Export is the module's *external* surface: a `public` top-level type, in a package the module
/// exports. `legacy` is the case this rule exists for — nameable inside the module, nameable
/// nowhere else, and therefore not API.
pub fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    let package = draft.aux.get("package").cloned().unwrap_or_default();
    let exported_packages = view.index.extra.get(HAS_MODULE_KEY).cloned();
    let package_ok = match &exported_packages {
        Some(list) => list.split(',').any(|candidate| candidate == package),
        None => true,
    };
    if !package_ok {
        return BTreeSet::new();
    }
    let mut out: BTreeSet<String> = BTreeSet::new();
    for symbol in &draft.symbols {
        if !symbol.container.is_empty() {
            continue;
        }
        if draft
            .aux
            .get(&format!("public:{}", symbol.name))
            .is_some_and(|value| value == "1")
        {
            out.insert(symbol.name.clone());
        }
    }
    out
}
