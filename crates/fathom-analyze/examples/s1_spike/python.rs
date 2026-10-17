//! Spike `s1` — the throwaway Python extractor.

use crate::common::{dir_of, join_dir, kids, qualify, text};
use crate::model::{Def, Edge, Index, RawImport, Scan};
use std::collections::{BTreeMap, BTreeSet};
use tree_sitter::{Language, Node};

pub fn language() -> Language {
    tree_sitter_python::LANGUAGE.into()
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

pub fn scan(root: Node, src: &[u8], file: &str, out: &mut Scan) {
    let _ = file;
    walk(
        root,
        src,
        &Stack {
            container: String::new(),
            scope: Scope::Module,
        },
        out,
    );
}

fn walk(node: Node, src: &[u8], stack: &Stack, out: &mut Scan) {
    for kid in kids(&node) {
        match kid.kind() {
            "class_definition" => {
                let Some(name) = field_name(&kid, src) else {
                    continue;
                };
                let full = qualify(&stack.container, &name);
                out.defs.push(Def::new("class", &full));
                mark(out, &full, stack);
                if decorated(&kid) {
                    out.note("decorated_class", &full);
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
                    );
                }
            }
            "function_definition" => {
                if stack.scope == Scope::Function {
                    // A declaration inside a function body is not a symbol of the file.
                    out.note(
                        "nested_function",
                        text(&kid, src).split('(').next().unwrap_or("?"),
                    );
                    continue;
                }
                let Some(name) = field_name(&kid, src) else {
                    continue;
                };
                let full = qualify(&stack.container, &name);
                let kind = if stack.scope == Scope::Class {
                    "method"
                } else {
                    "fn"
                };
                out.defs.push(Def::new(kind, &full));
                mark(out, &full, stack);
                if let Some(body) = kid.child_by_field_name("body") {
                    walk(
                        body,
                        src,
                        &Stack {
                            container: String::new(),
                            scope: Scope::Function,
                        },
                        out,
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
                // A walker that only looks for `def` loses every decorated member in the file,
                // which is most of what Python's API surface is made of.
                walk(kid, src, stack, out);
            }
            "if_statement" | "try_statement" | "for_statement" | "with_statement" => {
                // Conditional imports and conditional definitions are real: `if TYPE_CHECKING:`
                // and `try: import ujson` are how Python codebases spell optional.
                walk(kid, src, stack, out);
            }
            _ => {
                if stack.scope == Scope::Module {
                    walk(kid, src, stack, out);
                }
            }
        }
    }
}

fn decorated(node: &Node) -> bool {
    node.parent()
        .is_some_and(|p| p.kind() == "decorated_definition")
}

fn field_name(node: &Node, src: &[u8]) -> Option<String> {
    node.child_by_field_name("name")
        .map(|n| text(&n, src).to_string())
}

fn mark(out: &mut Scan, name: &str, stack: &Stack) {
    if stack.scope == Scope::Module {
        out.aux.insert(format!("top:{name}"), "1".into());
    }
}

/// Module-level data, and the two assignments that are really declarations.
fn assignment(node: Node, src: &[u8], stack: &Stack, out: &mut Scan) {
    let Some(left) = node.child_by_field_name("left") else {
        return;
    };
    let right = node.child_by_field_name("right");
    let right_text = right.map(|r| text(&r, src)).unwrap_or("");
    if stack.scope == Scope::Class {
        // A class attribute is a field, and fields are not symbols. A `Model.extra = ...` patch
        // onto another class is a `member:` assignment and is handled below.
        if left.kind() != "identifier" {
            monkeypatch(left, src, right_text, out);
        }
        return;
    }
    if stack.scope == Scope::Function {
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
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty() && s.chars().all(|c| c.is_alphabetic() || c == '_'))
                    .map(|s| s.to_string())
                    .collect();
                if !names.is_empty() {
                    out.aux.insert("__all__".into(), names.join(","));
                }
                out.defs.push(Def::new("var", "__all__"));
                mark(out, "__all__", stack);
                return;
            }
            // Python has no `const`. The honest rule is the shape of the value: a scalar or a
            // tuple is a constant, a collection or a call is mutable state. Name case alone
            // mislabels every `CONFIG = {}` in the language.
            let tuple_literal = right_text.starts_with('(')
                && right_text.ends_with(')')
                && right_text.contains(',');
            let constant_shape = !right_text.contains("namedtuple")
                && !right_text.starts_with('{')
                && !right_text.starts_with('[')
                && (!right_text.contains('(') || tuple_literal);
            let kind = if right_text.contains("namedtuple") {
                "class"
            } else if name.len() > 1 && name.chars().all(|c| !c.is_lowercase()) && constant_shape {
                "const"
            } else {
                "var"
            };
            out.defs.push(Def::new(kind, &name));
            mark(out, &name, stack);
        }
        "pattern_list" | "tuple" => {
            for target in kids(&left) {
                if target.kind() == "identifier" {
                    let name = text(&target, src).to_string();
                    out.defs.push(Def::new("var", &name));
                    mark(out, &name, stack);
                }
            }
        }
        "attribute" => monkeypatch(left, src, right_text, out),
        _ => {}
    }
}

/// `Shape.describe = lambda self: ...` declares a method of `Shape` in *this* file, the same way
/// a Rust `impl` in another file does. Whether the extractor should attribute it that way is a
/// policy the spike is asked to decide, so it decides, and the number is the price.
fn monkeypatch(left: Node, src: &[u8], right: &str, out: &mut Scan) {
    if left.kind() != "attribute" {
        // `Registry.items[item.name] = ...` writes data, not a member.
        return;
    }
    let looks_like_function = right.contains("lambda")
        || right.starts_with("property")
        || right.contains("staticmethod")
        || right.contains("classmethod");
    if !looks_like_function {
        return;
    }
    let text = text(&left, src).replace(' ', "");
    if let Some((owner, member)) = text.split_once('.')
        && owner.chars().next().is_some_and(|c| c.is_uppercase())
    {
        let name = qualify(owner, member.trim_end_matches('('));
        out.defs.push(Def::new("method", &name));
        out.note("monkeypatch", &name);
    }
}

/// Imports may sit anywhere in the file, including inside a function body.
fn import_nodes(node: Node, src: &[u8], stack: &Stack, out: &mut Scan) {
    match node.kind() {
        "import_statement" => {
            for kid in kids(&node) {
                match kid.kind() {
                    "dotted_name" => {
                        let mut imp = RawImport::new(text(&kid, src), "*", "import");
                        imp.module = vec![stack.container.clone()];
                        out.imports.push(imp);
                    }
                    "aliased_import" => {
                        if let Some(name) = kid.child_by_field_name("name") {
                            let mut imp = RawImport::new(text(&name, src), "*", "import");
                            imp.module = vec![stack.container.clone()];
                            out.imports.push(imp);
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
                let mut imp = RawImport::new(module, name, "from_import");
                imp.module = vec![stack.container.clone()];
                out.imports.push(imp);
            }
        }
        _ => {}
    }
}

/// `from .pkg.mod import name` -> the file, if the corpus has it.
fn module_candidates(base_dir: &str, dotted: &str) -> Vec<String> {
    let joined = dotted.replace('.', "/");
    vec![
        join_dir(base_dir, &format!("{joined}.py")),
        join_dir(base_dir, &format!("{joined}/__init__.py")),
    ]
}

pub fn resolve(file: &str, imports: &[RawImport], idx: &mut Index) -> BTreeSet<Edge> {
    let mut out = BTreeSet::new();
    for imp in imports {
        let dir = dir_of(file);
        let spec = imp.spec.as_str();
        let dots = spec.chars().take_while(|c| *c == '.').count();
        let rest = &spec[dots..];
        if imp.form == "import" {
            // `import a.b` names a module, not a symbol: the edge is to the file as a whole.
            let mut hit = None;
            for cand in module_candidates("", rest) {
                if idx.has(&cand) {
                    hit = Some(cand);
                    break;
                }
            }
            match hit {
                Some(target) => {
                    out.insert(Edge::new(target, "*"));
                }
                None => {
                    idx.extra
                        .insert(format!("unresolved:{file}"), spec.to_string());
                }
            }
            continue;
        }
        // Relative or absolute `from ... import ...`.
        let base = if dots == 0 {
            String::new()
        } else {
            let mut cur = dir.clone();
            for _ in 1..dots {
                cur = dir_of(&cur);
            }
            cur
        };
        let mut target = None;
        if !rest.is_empty() {
            for cand in module_candidates(&base, rest) {
                if idx.has(&cand) {
                    target = Some(cand);
                    break;
                }
            }
        } else if idx.has(&join_dir(&base, &format!("{}.py", imp.symbol))) {
            // `from . import service`: what is imported is a module, so the edge is to that
            // file as a whole, not to a symbol inside the package's `__init__`.
            out.insert(Edge::new(
                join_dir(&base, &format!("{}.py", imp.symbol)),
                "*",
            ));
            continue;
        } else {
            target = Some(join_dir(&base, "__init__.py"));
            if !idx.has(target.as_deref().unwrap_or_default()) {
                target = None;
            }
        }
        let Some(found) = target else {
            idx.extra.insert(
                format!("unresolved:{file}"),
                format!("{spec}::{}", imp.symbol),
            );
            continue;
        };
        if imp.symbol == "*" {
            out.insert(Edge::new(found, "*"));
        } else if found == file {
            // A self-import of one's own package: the edge is not a dependency on anything.
            continue;
        } else {
            out.insert(Edge::new(found, imp.symbol.clone()));
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
        let declared: BTreeSet<String> = scan
            .defs
            .iter()
            .filter(|d| !d.name.contains('.'))
            .map(|d| d.name.clone())
            .collect();
        if let Some(all) = scan.aux.get("__all__") {
            for name in all.split(',') {
                if declared.contains(name) {
                    names.insert(name.to_string());
                }
            }
        } else {
            for name in declared {
                if !name.starts_with('_') {
                    names.insert(name);
                }
            }
        }
        out.insert(file.clone(), names);
    }
    out
}
