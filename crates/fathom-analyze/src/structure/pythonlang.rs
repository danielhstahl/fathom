//! The Python extractor.
//!
//! Ported from spike `s1`'s throwaway (`crates/fathom-analyze/examples/s1_spike/python.rs`).
//! Python is where a naive walker loses the API: the spike's first version reported ~85 % on
//! definitions and looked fine, because `@property`, `@classmethod`, `@staticmethod` and every
//! method of every `@dataclass` sit inside a `decorated_definition` node the walker did not match.
//! A decorator is not decoration in a graph sense — it is the definition site of half of a real
//! module's surface.
//!
//! Three rules here are annotations the spike committed to rather than facts the grammar hands over:
//!
//! * **`__all__` is the export authority.** A name absent from it is not exported, however public it
//!   looks; with no `__all__`, a leading underscore is.
//! * **Assignment-as-definition.** `MAX_SHAPES: int = 10` and `Record = namedtuple(...)` are
//!   declarations; the kind is the *shape of the value*, because Python has no `const`.
//! * **A star import is a whole-file dependency**, never a set of symbols, because no parser can
//!   enumerate what `from models import *` binds.

use crate::structure::engine::{self, CorpusView, Draft, Index, Lang, RawImport};
use crate::structure::model::{
    Complexity, Language, Supertype, SupertypeKind, Symbol, SymbolKind, Visibility, synthetic,
};
use crate::structure::policy::ExtractPolicy;
use crate::structure::util::{self, dir_of, field, join_dir, kids, qualify, span_of, text};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language as TsLanguage, Node};

const BRANCH_KINDS: &[&str] = &[
    "if_statement",
    "elif_clause",
    "else_clause",
    "if_expression",
    "try_statement",
    "except_clause",
    "case_clause",
];
const LOOP_KINDS: &[&str] = &["for_statement", "while_statement", "for_clause"];
const RETURN_KINDS: &[&str] = &["return_statement", "raise_statement"];
const NEST_KINDS: &[&str] = &[
    "if_statement",
    "for_statement",
    "while_statement",
    "try_statement",
    "with_statement",
    "class_definition",
    "function_definition",
];
/// Bases whose instances get members the file does not contain.
const GENERATED_BASES: &[&str] = &[
    "NamedTuple",
    "TypedDict",
    "Enum",
    "IntEnum",
    "StrEnum",
    "Protocol",
];

pub fn grammar() -> TsLanguage {
    tree_sitter_python::LANGUAGE.into()
}

pub const LANG: Lang = Lang {
    language: Language::Python,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Module,
    Class,
    Function,
}

#[derive(Clone)]
struct Stack {
    container: String,
    scope: Scope,
}

pub fn scan(root: Node, src: &[u8], _file: &str, out: &mut Draft, policy: &ExtractPolicy) {
    walk(
        root,
        src,
        &Stack {
            container: String::new(),
            scope: Scope::Module,
        },
        out,
        policy,
    );
}

fn walk(node: Node, src: &[u8], stack: &Stack, out: &mut Draft, policy: &ExtractPolicy) {
    for kid in kids(&node) {
        match kid.kind() {
            "class_definition" => {
                let Some(name) = field_name(&kid, src) else {
                    continue;
                };
                let full = qualify(&stack.container, &name);
                let decorated = decorated(&kid);
                out.symbols.push(Symbol {
                    kind: SymbolKind::Class,
                    name: full.clone(),
                    container: stack.container.clone(),
                    span: span_of(&kid),
                    visibility: python_visibility(&full),
                    exported: false,
                    complexity: complexity(&kid, policy),
                    modifiers: if decorated {
                        vec!["decorated".to_string()]
                    } else {
                        Vec::new()
                    },
                });
                mark(out, &full, stack);
                if decorated {
                    // `@dataclass` writes `__init__`, `__eq__` and `__repr__`; `@property` changes
                    // how a member is reached. Either way the member list in the file is not the
                    // member list of the class.
                    out.note("decorated_class", &full);
                    out.mark_synthetic(&full, synthetic::DECORATED);
                    // The stronger statement, where the decorator is one this extractor knows: a
                    // dataclass writes accessors, so the members are not merely unknown, they are
                    // known to be *added*. `e3.7`'s API-surface pane reads the difference.
                    if decorators(&kid, src).iter().any(|decorator| {
                        matches!(decorator.as_str(), "dataclass" | "attr.s" | "attrs")
                    }) {
                        out.mark_synthetic(&full, synthetic::SYNTHESISED_ACCESSORS);
                    }
                }
                if let Some(superclasses) = field(&kid, "superclasses") {
                    supertypes(superclasses, src, &full, out);
                }
                if let Some(body) = kid.child_by_field_name("body") {
                    walk(
                        body,
                        src,
                        &Stack {
                            container: full,
                            scope: Scope::Class,
                        },
                        out,
                        policy,
                    );
                }
            }
            "function_definition" => {
                if stack.scope == Scope::Function {
                    // A declaration inside a function body is not a symbol of the file. This is the
                    // rule the normalisation section commits to, and the largest category of noise
                    // Python would otherwise produce.
                    let name = field_name(&kid, src).unwrap_or_default();
                    out.note("nested_function", &name);
                    continue;
                }
                let Some(name) = field_name(&kid, src) else {
                    continue;
                };
                let full = qualify(&stack.container, &name);
                let kind = if stack.scope == Scope::Class {
                    SymbolKind::Method
                } else {
                    SymbolKind::Fn
                };
                let mut modifiers: Vec<String> = Vec::new();
                if decorated(&kid) {
                    modifiers.push("decorated".to_string());
                    for decorator in decorators(&kid, src) {
                        match decorator.as_str() {
                            "property" => modifiers.push("property".to_string()),
                            "classmethod" => modifiers.push("classmethod".to_string()),
                            "staticmethod" => modifiers.push("staticmethod".to_string()),
                            "setter" | "getter" | "deleter" => {
                                modifiers.push(decorator.trim_end_matches('s').to_string())
                            }
                            "override" => modifiers.push("override".to_string()),
                            _ => modifiers.push(format!("decorated_by:{decorator}")),
                        }
                    }
                    // A decorated *function* is not marked synthetic, on purpose: what a decorator
                    // threatens is a member list, and a function has none. It is a class whose
                    // members a decorator can write that cannot be enumerated, which is the rule the
                    // class branch above follows.
                    modifiers.sort();
                    modifiers.dedup();
                }
                let is_async = text(&kid, src).starts_with("async");
                if is_async {
                    modifiers.push("async".to_string());
                }
                out.symbols.push(Symbol {
                    kind,
                    name: full.clone(),
                    container: stack.container.clone(),
                    span: span_of(&kid),
                    visibility: python_visibility(&full),
                    exported: false,
                    complexity: complexity(&kid, policy),
                    modifiers,
                });
                mark(out, &full, stack);
                if stack.container.is_empty() && name == "__getattr__" {
                    // A module-level `__getattr__` means `module.Thing` can resolve to something no
                    // file declares. Nothing in the file can be asserted complete after that.
                    out.note("module_getattr", &full);
                    out.mark_file_synthetic(synthetic::DYNAMIC_METHOD);
                }
                if let Some(body) = kid.child_by_field_name("body") {
                    walk(
                        body,
                        src,
                        &Stack {
                            container: String::new(),
                            scope: Scope::Function,
                        },
                        out,
                        policy,
                    );
                }
            }
            "expression_statement" => {
                if let Some(assign) = kid
                    .child_by_field_name("expression")
                    .or_else(|| kids(&kid).into_iter().find(|k| k.kind() == "assignment"))
                    && assign.kind() == "assignment"
                {
                    assignment(assign, src, stack, out);
                }
                import_nodes(kid, src, stack, out);
            }
            "import_statement" | "import_from_statement" => {
                import_nodes(kid, src, stack, out);
            }
            "decorated_definition" => {
                // `@dataclass`, `@property` and `@classmethod` wrap the definition they decorate.
                // A walker that only looks for `def` loses every decorated member in the file, which
                // is most of what Python's API surface is made of.
                walk(kid, src, stack, out, policy);
            }
            "if_statement" | "try_statement" | "for_statement" | "with_statement" => {
                // Conditional imports and conditional definitions are real: `if TYPE_CHECKING:` and
                // `try: import ujson` are how Python codebases spell optional.
                walk(kid, src, stack, out, policy);
            }
            _ => {
                if stack.scope == Scope::Module {
                    walk(kid, src, stack, out, policy);
                }
            }
        }
    }
}

/// Python has no per-symbol access modifier. The one convention the language actually has is the
/// leading underscore, which is exactly `Visibility::Crate` in shape: nameable inside the package,
/// not advertised outside it.
fn python_visibility(name: &str) -> Visibility {
    let last = name.rsplit('.').next().unwrap_or(name);
    if last.starts_with("__") && !last.ends_with("__") {
        Visibility::Scoped
    } else if last.starts_with('_') {
        Visibility::Crate
    } else {
        Visibility::Public
    }
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
                .filter(|kid| {
                    matches!(
                        kid.kind(),
                        "function_definition" | "class_definition" | "decorated_definition"
                    )
                })
                .count()
                + kids(&body)
                    .iter()
                    .filter(|kid| kid.kind() == "expression_statement")
                    .filter(|kid| kids(kid).iter().any(|inner| inner.kind() == "assignment"))
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

/// `class Circle(Shape)`, `class Box(NamedTuple)`, `class Meta(metaclass=ABCMeta)`.
fn supertypes(superclasses: Node, src: &[u8], of: &str, out: &mut Draft) {
    for kid in kids(&superclasses) {
        match kid.kind() {
            "keyword_argument" => {
                let name = field(&kid, "name").map(|n| text(&n, src)).unwrap_or("");
                if name == "metaclass" {
                    let base = field(&kid, "value").map(|v| text(&v, src)).unwrap_or("");
                    out.supertypes.push(Supertype {
                        of: of.to_string(),
                        base: base.to_string(),
                        kind: SupertypeKind::MetaClass,
                        resolved: None,
                        span: span_of(&kid),
                    });
                } else {
                    out.note("class_keyword_argument", name);
                }
            }
            "comment" => {}
            _ => {
                let base = text(&kid, src).trim().to_string();
                if base.is_empty() {
                    continue;
                }
                let base_name = base.split('[').next().unwrap_or(&base).to_string();
                out.supertypes.push(Supertype {
                    of: of.to_string(),
                    base: base_name.clone(),
                    kind: SupertypeKind::Base,
                    resolved: None,
                    span: span_of(&kid),
                });
                if GENERATED_BASES
                    .iter()
                    .any(|generated| base_name.ends_with(generated))
                {
                    // Inheriting `NamedTuple` or `TypedDict` gives the class members the file does
                    // not contain, which is the same state a decorator writes.
                    out.note("generated_base", &base_name);
                    out.mark_synthetic(of, synthetic::SYNTHESISED_ACCESSORS);
                }
            }
        }
    }
}

fn decorated(node: &Node) -> bool {
    node.parent()
        .is_some_and(|parent| parent.kind() == "decorated_definition")
}

/// The decorator expressions stacked above a definition, innermost spelling first.
fn decorators(node: &Node, src: &[u8]) -> Vec<String> {
    let Some(outer) = node.parent().filter(|p| p.kind() == "decorated_definition") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for kid in kids(&outer) {
        if kid.kind() != "decorator" {
            continue;
        }
        let spelling = text(&kid, src).trim_start_matches('@').to_string();
        // `@dataclass` and `@dataclass(frozen=True)` name the same decorator.
        let name = spelling
            .split(['(', '['])
            .next()
            .unwrap_or(spelling.as_str())
            .trim()
            .to_string();
        out.push(name);
    }
    out
}

fn field_name(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
}

fn mark(out: &mut Draft, name: &str, stack: &Stack) {
    if stack.scope == Scope::Module {
        out.aux.insert(format!("top:{name}"), "1".into());
    }
}

/// Module-level data, and the two assignments that are really declarations.
fn assignment(node: Node, src: &[u8], stack: &Stack, out: &mut Draft) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let right = node.child_by_field_name("right");
    let right_text = right.map(|r| text(&r, src)).unwrap_or("");
    if stack.scope == Scope::Class || stack.scope == Scope::Function {
        if left.kind() != "identifier" {
            monkeypatch(left, src, right_text, out);
        }
        return;
    }
    match left.kind() {
        "identifier" => {
            let name = text(&left, src).to_string();
            if name == "__all__" {
                let names: Vec<String> = right_text
                    .split(['[', ']', ',', '"', '\''])
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_alphabetic() || c == '_'))
                    .map(String::from)
                    .collect();
                if !names.is_empty() {
                    out.aux.insert("__all__".into(), names.join(","));
                }
                out.symbols.push(module_symbol(
                    "var",
                    &name,
                    &node,
                    vec!["dunder_all".to_string()],
                ));
                mark(out, "__all__", stack);
                return;
            }
            // Python has no `const`. The honest rule is the shape of the value: a scalar or a tuple
            // is a constant, a collection or a call is mutable state. Name case alone mislabels
            // every `CONFIG = {}` in the language.
            let tuple_literal = right_text.starts_with('(')
                && right_text.ends_with(')')
                && right_text.contains(',');
            let constant_shape = !right_text.contains("namedtuple")
                && !right_text.starts_with('{')
                && !right_text.starts_with('[')
                && (!right_text.contains('(') || tuple_literal);
            let kind = if right_text.contains("namedtuple") {
                out.note("namedtuple_call", &name);
                SymbolKind::Class
            } else if name.len() > 1 && name.chars().all(|c| !c.is_lowercase()) && constant_shape {
                SymbolKind::Const
            } else {
                SymbolKind::Var
            };
            out.symbols
                .push(module_symbol(kind.as_str(), &name, &node, Vec::new()));
            mark(out, &name, stack);
        }
        "pattern_list" | "tuple" => {
            for target in kids(&left) {
                if target.kind() == "identifier" {
                    let name = text(&target, src).to_string();
                    out.symbols
                        .push(module_symbol("var", &name, &node, Vec::new()));
                    mark(out, &name, stack);
                }
            }
        }
        "attribute" => monkeypatch(left, src, right_text, out),
        _ => {}
    }
}

fn module_symbol(kind: &str, name: &str, node: &Node, modifiers: Vec<String>) -> Symbol {
    Symbol {
        kind: SymbolKind::parse(kind).unwrap_or(SymbolKind::Var),
        name: name.to_string(),
        container: String::new(),
        span: span_of(node),
        visibility: python_visibility(name),
        exported: false,
        complexity: Complexity::default(),
        modifiers,
    }
}

/// `Shape.describe = lambda self: ...` declares a method of `Shape` in *this* file, the same way a
/// Rust `impl` in another file does. Whether the extractor should attribute it that way is a policy
/// question, so this module decides it, and the note is the price.
fn monkeypatch(left: Node, src: &[u8], right: &str, out: &mut Draft) {
    if left.kind() != "attribute" {
        // `Registry.items[item.name] = ...` writes data, not a member, and marking the file for it
        // would make every dict-literate file unenumerable. What *is* unenumerable is a write to the
        // module namespace itself: `globals()["Dynamic"] = type(...)` puts a name in the module that
        // no file declares, and `setattr(obj, name, ...)` puts a member on an object named as a
        // string. Both are stated as the same thing the model has a word for.
        let spelling = text(&left, src);
        if spelling.contains("globals()") || spelling.contains("locals()") {
            out.note("dynamic_binding", spelling.trim());
            out.mark_file_synthetic(synthetic::DYNAMIC_METHOD);
        } else if spelling.trim_start().starts_with("setattr(") {
            out.note("setattr", spelling.trim());
            out.mark_file_synthetic(synthetic::MONKEYPATCH);
        }
        return;
    }
    let looks_like_function = right.contains("lambda")
        || right.starts_with("property")
        || right.contains("staticmethod")
        || right.contains("classmethod");
    if !looks_like_function {
        return;
    }
    let spelling = text(&left, src).replace(' ', "");
    if let Some((owner, member)) = spelling.split_once('.')
        && owner.chars().next().is_some_and(|c| c.is_uppercase())
    {
        let name = qualify(owner, member.trim_end_matches('('));
        out.symbols.push(Symbol {
            kind: SymbolKind::Method,
            name: name.clone(),
            container: owner.to_string(),
            span: span_of(&left),
            visibility: Visibility::Public,
            exported: false,
            complexity: Complexity::default(),
            modifiers: vec!["monkeypatch".to_string()],
        });
        out.note("monkeypatch", &name);
        // The class on the left may have members this file writes and no file declares.
        out.mark_file_synthetic(synthetic::MONKEYPATCH);
    }
}

/// Imports may sit anywhere in the file, including inside a function body.
fn import_nodes(node: Node, src: &[u8], stack: &Stack, out: &mut Draft) {
    match node.kind() {
        "import_statement" => {
            for kid in kids(&node) {
                match kid.kind() {
                    "dotted_name" => {
                        let mut import =
                            RawImport::new(text(&kid, src), "*", "import", span_of(&node));
                        import.module = vec![stack.container.clone()];
                        out.imports.push(import);
                    }
                    "aliased_import" => {
                        if let Some(name) = kid.child_by_field_name("name") {
                            let mut import =
                                RawImport::new(text(&name, src), "*", "import", span_of(&node));
                            import.module = vec![stack.container.clone()];
                            out.imports.push(import);
                        }
                    }
                    _ => {}
                }
            }
        }
        "import_from_statement" => {
            let module = node
                .child_by_field_name("module_name")
                .map(|n| text(&n, src))
                .unwrap_or("");
            let mut names: Vec<String> = Vec::new();
            for kid in kids(&node) {
                match kid.kind() {
                    "dotted_name" => names.push(text(&kid, src).to_string()),
                    "aliased_import" => {
                        if let Some(name) = kid.child_by_field_name("name") {
                            names.push(text(&name, src).to_string());
                        }
                    }
                    "wildcard_import" => names.push("*".to_string()),
                    _ => {}
                }
            }
            if names.is_empty() {
                names.push("*".to_string());
            }
            for name in names {
                let mut import = RawImport::new(module, name, "from_import", span_of(&node));
                import.module = vec![stack.container.clone()];
                out.imports.push(import);
            }
        }
        _ => {}
    }
}

/// `from .pkg.mod import name` → the file, if the corpus has it.
fn module_candidates(base_dir: &str, dotted: &str) -> Vec<String> {
    let joined = dotted.replace('.', "/");
    vec![
        join_dir(base_dir, &format!("{joined}.py")),
        join_dir(base_dir, &format!("{joined}/__init__.py")),
    ]
}

pub fn resolve(
    file: &str,
    draft: &Draft,
    index: &mut Index,
    _policy: &ExtractPolicy,
    _truncated: &mut usize,
) -> Vec<engine::Edge> {
    let mut out: Vec<engine::Edge> = Vec::new();
    for import in &draft.imports {
        let dir = dir_of(file);
        let spec = import.spec.as_str();
        let dots = spec.chars().take_while(|c| *c == '.').count();
        let rest = &spec[dots..];
        if import.form == "import" {
            // `import a.b` names a module, not a symbol: the edge is to the file as a whole, which
            // is a wildcard edge in this model's vocabulary.
            let hit = module_candidates("", rest)
                .into_iter()
                .find(|c| index.has(c));
            match hit {
                Some(target) => {
                    let mut edge = engine::Edge::wildcard(target);
                    edge.form = import.form.to_string();
                    edge.specifier = spec.to_string();
                    out.push(edge);
                }
                None => out.push(unresolved(import)),
            }
            continue;
        }
        // Relative or absolute `from ... import ...`.
        let base = if dots == 0 {
            String::new()
        } else {
            let mut current = dir.clone();
            for _ in 1..dots {
                current = dir_of(&current);
            }
            current
        };
        let mut target = None;
        if !rest.is_empty() {
            target = module_candidates(&base, rest)
                .into_iter()
                .find(|c| index.has(c));
        } else if index.has(&join_dir(&base, &format!("{}.py", import.symbol))) {
            // `from . import service`: what is imported is a module, so the edge is to that file as
            // a whole, not to a symbol inside the package's `__init__`.
            let mut edge =
                engine::Edge::wildcard(join_dir(&base, &format!("{}.py", import.symbol)));
            edge.form = import.form.to_string();
            edge.specifier = spec.to_string();
            out.push(edge);
            continue;
        } else {
            let candidate = join_dir(&base, "__init__.py");
            if index.has(&candidate) {
                target = Some(candidate);
            }
        }
        let Some(found) = target else {
            out.push(unresolved(import));
            continue;
        };
        if import.symbol == "*" {
            let mut edge = engine::Edge::wildcard(found);
            edge.form = import.form.to_string();
            edge.specifier = spec.to_string();
            out.push(edge);
        } else if found == file {
            // A self-import of one's own package is not a dependency on anything.
            continue;
        } else {
            let mut edge = engine::Edge::resolved(found, import.symbol.clone());
            edge.form = import.form.to_string();
            edge.specifier = spec.to_string();
            out.push(edge);
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

/// Python publishes nothing.
///
/// `from .util import label` in `models.py` makes `label` *visible* under `models.label`, but it is
/// a use, not a re-export, and attributing it as one would claim an intent the language does not
/// state. Rust's `pub use` and TypeScript's `export … from` do state it, and those two do publish;
/// this one does not, on purpose.
pub fn publish(_file: &str, _draft: &Draft, _edges: &[engine::Edge], _index: &mut Index) {}

fn export_names(file: &str, view: &CorpusView) -> BTreeSet<String> {
    let Some(draft) = view.drafts.get(file) else {
        return BTreeSet::new();
    };
    let mut names = BTreeSet::new();
    let declared: BTreeSet<String> = draft
        .symbols
        .iter()
        .filter(|symbol| symbol.container.is_empty())
        .map(|symbol| symbol.name.clone())
        .collect();
    if let Some(all) = draft.aux.get("__all__") {
        for name in all.split(',') {
            if declared.contains(name) {
                names.insert(name.to_string());
            }
        }
    } else {
        for name in declared {
            if python_visibility(&name).is_public() && !name.starts_with('_') {
                names.insert(name);
            }
        }
    }
    names
}
