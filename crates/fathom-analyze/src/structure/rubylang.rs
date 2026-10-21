//! The Ruby extractor (`e3.1b`, `reco-dlp.1`).
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/ruby.rs`) with the
//! spans, complexity, supertypes and cannot-enumerate marks a spike skips. Ruby is the language the
//! spike spent most of its unease on, and the two things it came back with are both implemented
//! here rather than explained away:
//!
//! * **The export relation is degenerate, and saying so is the feature.** Ruby has no per-symbol
//!   access modifier: 8 exports for 67 declarations, and "what this file exports" is nearly "what
//!   this file declares at top level". The extractor still computes it — a graph needs *a* answer —
//!   but every Ruby file that has one carries the note `export_surface_is_the_file`, and
//!   [`crate::structure::CorpusStructure::loads`] is the question the language can actually answer:
//!   *which files does this entry point pull in*. That is `e3.7`'s Ruby surface, per s1 finding 3.
//! * **A declaration list is not a method list.** `attr_accessor`, `define_method`, `class_eval`,
//!   `Struct.new` and `%i[a b].each { |n| define_method(n) }` all write methods. The ones a parser
//!   can name it names (a literal `define_method(:perimeter)`, every `attr_reader`); the ones whose
//!   names are computed at runtime mark the scope [`synthetic::DYNAMIC_METHOD`] and are recorded as
//!   the blind spot the corpus excuses. Silence is the one answer that is not allowed.
//!
//! And one thing the language did not ask for but the run needs: Ruby is the one grammar `s1`
//! measured an order of magnitude slower than its neighbours (under 1 MiB/s against 11–16), which is
//! why [`crate::structure::ExtractPolicy::scan_budget`] exists and why Ruby is the only language
//! with a per-run cap turned on by default. Files past the cap come back with
//! [`UnavailableReason::ScanBudget`](crate::structure::UnavailableReason::ScanBudget) and their
//! file-level numbers, which is a refusal somebody configured rather than a truncation nobody chose.

use crate::structure::engine::{CorpusView, Draft, Index, Lang, RawImport};
use crate::structure::model::{
    Complexity, Edge, EdgeKind, Granularity, Language, Supertype, SupertypeKind, Symbol,
    SymbolKind, Visibility, synthetic,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{self, dir_of, field, join_dir, kids, qualify, span_of, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

/// The macros that declare a method without a `def`. This list is the whole DSL support of the
/// extractor: everything it does not know about (`defdelegate` to a renamed target, a plugin's own
/// macro, `ActiveRecord`'s entire vocabulary) stays invisible, and the honest response to that is
/// the blind-spot note, not a longer list nobody verifies.
const MACROS: &[(&str, MethodKind)] = &[
    ("attr_reader", MethodKind::Reader),
    ("attr_writer", MethodKind::Writer),
    ("attr_accessor", MethodKind::Accessor),
    ("defdelegate", MethodKind::Reader),
    ("alias_method", MethodKind::Reader),
    ("module_function", MethodKind::Reader),
    // A literal `define_method(:perimeter) { }` is a definition in every sense a parser cares
    // about. The loop that computes names from a constant array is not, and stays a blind spot.
    ("define_method", MethodKind::Reader),
];

enum MethodKind {
    Reader,
    Writer,
    Accessor,
}

/// The methods a class gets from `include`/`prepend`/`extend` — Ruby's mixin mechanism, and the
/// closest thing the language has to an implemented interface.
const MIXIN_CALLS: &[&str] = &["include", "prepend", "extend"];

pub fn grammar() -> TsLanguage {
    tree_sitter_ruby::LANGUAGE.into()
}

pub const LANG: Lang = Lang {
    language: Language::Ruby,
    grammar,
    scan,
    seed,
    resolve,
    publish,
    prepare_exports,
    export_names,
};

// ---------------------------------------------------------------------------
// scan
// ---------------------------------------------------------------------------

/// The three load-path roots, in the order tried. Ruby resolves a bare `require` against a load
/// path that the file does not state; `lib/` is the convention every gem uses and the one the corpus
/// is written to, `<dir>/` catches a `require` inside the package's own tree, and the bare path
/// catches a corpus that *is* the load path. An extractor that guessed one and only one could not
/// resolve a single `require` in a repository laid out differently, and could not say which
/// convention it used if it did.
const LOAD_PATHS: &[LoadPath] = &[LoadPath::Root("lib"), LoadPath::Sibling, LoadPath::Root("")];

enum LoadPath {
    Root(&'static str),
    Sibling,
}

impl LoadPath {
    fn candidate(&self, dir: &str, spec: &str) -> String {
        match self {
            LoadPath::Root("") => format!("{spec}.rb"),
            LoadPath::Root(root) => join_dir(root, &format!("{spec}.rb")),
            LoadPath::Sibling => join_dir(dir, &format!("{spec}.rb")),
        }
    }
}

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    let _ = file;
    body(root, src, "", true, Scope::default(), out, policy);
    if out.aux.keys().any(|key| key.starts_with("top:")) {
        // Carried with the export list wherever there is one: "what this file exports" in Ruby is
        // "what this file declared at top level", and a surface that prints it under the heading
        // "public symbols" is telling a lie about the language (s1 finding 3).
        out.note(
            DEGENERATE_SURFACE_NOTE,
            "Ruby has no per-symbol access control: this list is every top-level name, not a public API",
        );
    }
}

#[derive(Clone, Copy, Default)]
struct Scope {
    /// Inside `class << self`, which moves nothing but explains the name.
    eigenclass: bool,
}

fn dotted(raw: &str) -> String {
    raw.replace("::", ".")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("")
}

#[allow(clippy::too_many_arguments)]
fn body(
    node: Node,
    src: &[u8],
    container: &str,
    top: bool,
    scope: Scope,
    out: &mut Draft,
    policy: &ExtractPolicy,
) {
    for kid in kids(&node) {
        match kid.kind() {
            "module" | "class" => {
                let kind = if kid.kind() == "module" {
                    SymbolKind::Module
                } else {
                    SymbolKind::Class
                };
                let Some(raw) = kid.child_by_field_name("name") else {
                    continue;
                };
                let name = dotted(text(&raw, src));
                let stored = if name.contains('.') {
                    // `module Demo::Core` declares `Demo.Core` where it is written; the `Demo` part
                    // is a reference to something declared elsewhere — possibly in another file.
                    name.clone()
                } else {
                    qualify(container, &name)
                };
                push(out, &stored, kind, kid, top, policy);
                if let Some(superclass) = kid.child_by_field_name("superclass") {
                    // tree-sitter-ruby puts the `<` inside the superclass node, so the base name has
                    // to drop it: a base of `<Shape` is a name no other file will ever match, and the
                    // scoring layer compares base names as strings, so the marker would surface as a
                    // wrong answer rather than a formatting wart.
                    let base = dotted(
                        superclass
                            .utf8_text(src)
                            .unwrap_or_default()
                            .trim_start_matches(|c: char| c == '<' || c.is_whitespace()),
                    );
                    if !base.is_empty() {
                        out.supertypes.push(Supertype {
                            of: stored.clone(),
                            base,
                            kind: SupertypeKind::Base,
                            resolved: None,
                            span: span_of(&superclass),
                        });
                    }
                }
                let inner = match kid.child_by_field_name("body") {
                    Some(inner) => inner,
                    None => kid,
                };
                body(inner, src, &stored, false, scope, out, policy);
            }
            "method" | "singleton_method" => {
                declare_method(kid, src, container, top, scope, out, policy);
            }
            "singleton_class" => {
                for part in kids(&kid) {
                    if matches!(part.kind(), "body_statement" | "statements") {
                        body(
                            part,
                            src,
                            container,
                            false,
                            Scope { eigenclass: true },
                            out,
                            policy,
                        );
                    }
                }
            }
            "assignment" => assign(kid, src, container, top, out, policy),
            "alias" => {
                let parts: Vec<String> = kids(&kid)
                    .iter()
                    .map(|k| text(k, src).trim_start_matches(':').to_string())
                    .collect();
                if let Some(first) = parts.first() {
                    let stored = qualify(container, first);
                    push(out, &stored, SymbolKind::Method, kid, false, policy);
                    out.note("alias", &stored);
                }
            }
            "call" => call(kid, src, container, top, scope, out, policy),
            "if" | "unless" | "then" | "else" | "elsif" | "begin" | "case" | "when" | "ensure"
            | "body_statement" | "statements" => {
                // Conditional `require`s and `if`-guarded class bodies are ordinary Ruby.
                body(kid, src, container, top, scope, out, policy);
            }
            _ => {}
        }
    }
}

fn push(
    out: &mut Draft,
    name: &str,
    kind: SymbolKind,
    node: Node,
    top: bool,
    policy: &ExtractPolicy,
) {
    if top {
        out.aux.insert(format!("top:{name}"), "1".to_string());
    }
    out.symbols.push(Symbol {
        kind,
        name: name.to_string(),
        container: match name.rfind('.') {
            Some(at) => name[..at].to_string(),
            None => String::new(),
        },
        span: span_of(&node),
        // Ruby's only visibility is `private`/`protected`, which is a runtime state of one object
        // rather than a name property the parser can settle: a later `public` in the same body
        // changes it, and `send` bypasses it entirely. So a member below a bare `private` is marked
        // `scoped` and never `public`: the *file* is the surface (s1 finding 3).
        visibility: match kind {
            SymbolKind::Module | SymbolKind::Class => Visibility::Public,
            _ => Visibility::Scoped,
        },
        exported: false,
        complexity: complexity(&node, policy),
        modifiers: Vec::new(),
    });
}

#[allow(clippy::too_many_arguments)]
fn declare_method(
    node: Node,
    src: &[u8],
    container: &str,
    top: bool,
    scope: Scope,
    out: &mut Draft,
    policy: &ExtractPolicy,
) {
    let Some(raw) = node.child_by_field_name("name") else {
        return;
    };
    let name = text(&raw, src).to_string();
    let stored = if node.kind() == "method" && container.is_empty() {
        name
    } else {
        qualify(container, &name)
    };
    let kind = if container.is_empty() {
        SymbolKind::Fn
    } else {
        SymbolKind::Method
    };
    push(out, &stored, kind, node, top, policy);
    if let Some(symbol) = out.symbols.last_mut() {
        let mut modifiers: Vec<String> = Vec::new();
        if node.kind() == "singleton_method" {
            modifiers.push("singleton_def".to_string());
        }
        if scope.eigenclass {
            modifiers.push("eigenclass".to_string());
        }
        symbol.modifiers = modifiers;
    }
}

fn assign(
    node: Node,
    src: &[u8],
    container: &str,
    top: bool,
    out: &mut Draft,
    policy: &ExtractPolicy,
) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let right = node
        .child_by_field_name("right")
        .map(|r| text(&r, src))
        .unwrap_or("");
    match left.kind() {
        "constant" | "scope_resolution" | "constant_reference" => {
            let name = dotted(text(&left, src));
            let stored = if name.contains('.') || container.is_empty() {
                name
            } else {
                qualify(container, &name)
            };
            let class_shaped = right.contains("Struct.new") || right.contains("Class.new");
            let kind = if class_shaped {
                SymbolKind::Class
            } else {
                SymbolKind::Const
            };
            push(out, &stored, kind, node, top, policy);
            if class_shaped {
                // `Pointy = Struct.new(:x, :y) do … end` declares a class the file *names* and a
                // set of members no file contains. Both halves get said: the class is a definition,
                // the generated members are a blind spot.
                out.note("struct_new", &stored);
                for block in blocks_under(node) {
                    body(block, src, &stored, false, Scope::default(), out, policy);
                }
            }
        }
        "attribute" | "element_reference" | "subscript_reference" => {
            // `Shape.pad = ..` declares a method on a class named elsewhere; `HANDLERS[:x] = ..` is
            // data. The parser cannot tell them apart by shape, and guessing would invent a method.
            out.note("monkeypatch_or_data", &dotted(text(&left, src)));
        }
        _ => {}
    }
}

/// Every block under a node, however the grammar nested it.
fn blocks_under(node: Node) -> Vec<Node> {
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

#[allow(clippy::too_many_arguments)]
fn call(
    node: Node,
    src: &[u8],
    container: &str,
    top: bool,
    scope: Scope,
    out: &mut Draft,
    policy: &ExtractPolicy,
) {
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
        // Literals only. An identifier argument is a *variable*: treating `define_method(name)` as
        // a definition of `name` invents a method that may not exist, which is the one error a graph
        // cannot tolerate. The dynamic case gets a blind-spot note instead.
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
            if let Some(spec) = args.first() {
                let form = if method == "require_relative" {
                    "require_relative"
                } else {
                    "load_path_require"
                };
                out.imports
                    .push(RawImport::new(spec.clone(), "*", form, span_of(&node)));
            }
        }
        "autoload" => {
            if let Some(spec) = args.get(1) {
                out.imports.push(RawImport::new(
                    spec.clone(),
                    "*",
                    "autoload",
                    span_of(&node),
                ));
            }
        }
        "class_eval" | "module_eval" | "instance_eval" if !receiver.is_empty() => {
            let name = dotted(receiver);
            // The reopened class is declared in another file, so *this* file cannot enumerate it —
            // the same state a macro is, arrived at from the other side.
            out.note("class_eval", &name);
            out.mark_synthetic(&name, synthetic::MONKEYPATCH);
            for block in kids(&node)
                .into_iter()
                .filter(|k| matches!(k.kind(), "do_block" | "block" | "body_statement"))
            {
                body(block, src, &name, false, scope, out, policy);
            }
        }
        _ => {
            for (macro_name, kind) in MACROS.iter() {
                if method != *macro_name {
                    continue;
                }
                for arg in &args {
                    let stored = qualify(container, arg);
                    push(out, &stored, SymbolKind::Method, node, false, policy);
                    out.note("macro_method", &stored);
                    if matches!(kind, MethodKind::Accessor | MethodKind::Writer) {
                        let setter = format!("{stored}=");
                        push(out, &setter, SymbolKind::Method, node, false, policy);
                        out.note("macro_method", &setter);
                    }
                }
            }
            // `define_method(n)` where `n` comes from a collection is a definition the parser
            // cannot see. Saying so is the difference between "4 methods missing" and "4 methods
            // missing because their names are computed at runtime".
            if method == "define_method" && args.is_empty() {
                out.note("define_method_dynamic", container);
                out.mark_synthetic(container, synthetic::DYNAMIC_METHOD);
            }
        }
    }
    if MIXIN_CALLS.contains(&method.as_str()) {
        let mixed: Vec<String> = kids(&node)
            .into_iter()
            .filter(|k| k.kind() == "argument_list")
            .flat_map(|list| kids(&list))
            .filter(|k| {
                matches!(
                    k.kind(),
                    "constant" | "scope_resolution" | "constant_reference"
                )
            })
            .map(|k| dotted(text(&k, src)))
            .collect();
        for arg in &mixed {
            out.supertypes.push(Supertype {
                of: container.to_string(),
                base: arg.clone(),
                kind: SupertypeKind::Base,
                resolved: None,
                span: span_of(&node),
            });
        }
    }
    for arg in kids(&node) {
        match arg.kind() {
            // `private def helper` passes a definition through as an argument; the definition is
            // real even though the call around it is a visibility macro.
            "argument_list" => {
                for inner in kids(&arg) {
                    if matches!(inner.kind(), "method" | "singleton_method") {
                        declare_method(inner, src, container, top, scope, out, policy);
                    }
                }
            }
            // `%i[a b].each { |n| define_method(n) { … } }`: the work happens inside the block, so
            // a walker that stops at the call never sees what the loop is driving. The reopen forms
            // are excluded because they were already walked above, under the container they reopen —
            // walking them twice would report `audit` both as a top-level function and as a method
            // of the reopened class, and only one of those is Ruby's meaning.
            "block" | "do_block"
                if !matches!(
                    method.as_str(),
                    "class_eval" | "module_eval" | "instance_eval"
                ) =>
            {
                body(arg, src, container, top, scope, out, policy);
            }
            _ => {}
        }
    }
}

fn complexity(node: &Node, policy: &ExtractPolicy) -> Complexity {
    let span = span_of(node);
    let (branches, _) = util::count_kinds(
        node,
        &[
            "if",
            "unless",
            "case",
            "when_clause",
            "rescue_clause",
            "ternary",
        ],
        policy.max_complexity_nodes,
    );
    let (loops, _) = util::count_kinds(
        node,
        &["while", "until", "for", "call_block"],
        policy.max_complexity_nodes,
    );
    let (returns, _) = util::count_kinds(
        node,
        &["return", "break", "next"],
        policy.max_complexity_nodes,
    );
    let (nesting, _) = util::max_depth_of(
        node,
        &["if", "unless", "case", "while", "until", "for", "begin"],
        policy.max_complexity_nodes,
    );
    let params = field(node, "parameters")
        .map(|parameters| kids(&parameters).len())
        .unwrap_or(0);
    let members = 0usize;
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

pub fn seed(_file: &str, _draft: &Draft, _index: &mut Index) {}

/// `require` is a whole-file dependency: Ruby has no symbol-level import edge at all, so every
/// resolved edge carries symbol `*` and the import relation can never say *which* name was used.
/// That is a property of the language, and the wildcard edge kind is what makes it visible instead
/// of merely absent.
pub fn resolve(
    file: &str,
    draft: &Draft,
    index: &mut Index,
    policy: &ExtractPolicy,
    truncated: &mut usize,
) -> Vec<Edge> {
    let _ = (policy, truncated);
    let dir = dir_of(file);
    let mut out: Vec<Edge> = Vec::new();
    for import in &draft.imports {
        let found = if import.form == "require_relative" {
            Some(join_dir(&dir, &format!("{}.rb", import.spec)))
                .filter(|candidate| index.has(candidate))
        } else {
            LOAD_PATHS
                .iter()
                .map(|root| root.candidate(&dir, &import.spec))
                .find(|candidate| index.has(candidate))
        };
        match found {
            Some(target) => {
                let mut edge = Edge::wildcard(target);
                edge.form = import.form.to_string();
                out.push(edge);
            }
            None => out.push(Edge {
                target: String::new(),
                symbol: import.symbol.clone(),
                kind: EdgeKind::Unresolved,
                specifier: import.spec.clone(),
                via: None,
                form: import.form.to_string(),
                passes: 0,
                granularity: Granularity::File,
                package: None,
            }),
        }
    }
    out
}

pub fn publish(_file: &str, _draft: &Draft, _edges: &[Edge], _index: &mut Index) {}

pub fn prepare_exports(_view: &CorpusView) -> BTreeMap<String, String> {
    BTreeMap::new()
}

/// The degenerate answer, computed honestly: every top-level constant and top-level method. See
/// [`export_names`]'s note in the module header — a consumer that reads this as "this file's public
/// symbols" is reading a Ruby file as if it had Ruby's rules about hiding things.
pub fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    draft
        .symbols
        .iter()
        .filter(|symbol| draft.aux.contains_key(&format!("top:{}", symbol.name)))
        .map(|symbol| symbol.name.clone())
        .collect()
}

/// The note every Ruby file with an export carries, so a surface cannot print the list without
/// carrying the caveat with it.
pub const DEGENERATE_SURFACE_NOTE: &str = "export_surface_is_the_file";
