//! Tree-sitter helpers shared by every language extractor in this module.
//!
//! Nothing here is language-specific; what is language-specific is which node kinds a caller asks
//! about. The functions are small on purpose — every one of them exists because a per-language
//! extractor needed it twice.

use crate::structure::model::Span;
use tree_sitter::Node;

/// Text a node covers. Non-UTF-8 comes back empty rather than panicking: a hostile file is an
/// input, not a bug (`e7.8`'s rule for every named fuzz entry point).
pub fn text<'a>(node: &Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("")
}

/// Direct child with a given field name.
pub fn field<'a>(node: &Node<'a>, name: &str) -> Option<Node<'a>> {
    node.child_by_field_name(name)
}

/// Direct named children, in source order.
pub fn kids<'a>(node: &Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

/// Direct children named *and* anonymous, in source order.
///
/// `export default` and `export * from` are marked by the anonymous tokens `default` and `*`,
/// which a `named_children()` walk cannot see at all — spike `s1` found that by shipping a
/// TypeScript barrel that exported nothing.
pub fn all_kids<'a>(node: &Node<'a>) -> Vec<Node<'a>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).collect()
}

/// Join a dotted name onto a container prefix.
pub fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// `Vec<T>` / `crate::geo::Point<'a>` / `*const u8` → the base name a method belongs to.
pub fn base_type(ty: &str) -> String {
    let mut out = String::new();
    for ch in ty.trim().chars() {
        match ch {
            '<' | '(' | '[' | ' ' | '*' | '&' | '\'' | '{' | '\n' => break,
            _ => out.push(ch),
        }
    }
    let out = out.trim_end_matches(':');
    match out.rsplit("::").next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => out.to_string(),
    }
}

pub fn join_dir(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// `pkg/shape/shape.go` → `pkg/shape`; a bare name → "".
pub fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

pub fn file_name(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_string()
}

/// `geo/mod.rs` → `geo`; `lib.rs` → `lib`; `geo/point.rs` → `geo/point`.
pub fn stem(path: &str) -> String {
    match path.rfind('.') {
        Some(i) => path[..i].to_string(),
        None => path.to_string(),
    }
}

/// True for `.d.ts`, which the TypeScript extractor treats as an ambient declaration file: the
/// stub *is* the package's only declaration of the symbols in it.
pub fn is_d_ts(path: &str) -> bool {
    file_name(path).ends_with(".d.ts")
}

/// A node's span in the shape the model stores.
pub fn span_of(node: &Node) -> Span {
    Span {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: node.start_position().row + 1,
        end_line: node.end_position().row + 1,
    }
}

/// Count descendants of the listed kinds, bounded.
///
/// The budget is not decoration: without it a declaration wrapping a generated 40 000-element table
/// costs the whole file's share of the run budget. When the budget is exhausted the count stops and
/// `truncated` is returned so the caller can record the note rather than a silently small number.
pub fn count_kinds(node: &Node, kinds: &[&str], budget: usize) -> (usize, bool) {
    let mut counted = 0usize;
    let mut visited = 0usize;
    let mut truncated = false;
    let mut cursor = node.walk();
    let mut stack = vec![*node];
    while let Some(current) = stack.pop() {
        if visited >= budget {
            truncated = true;
            break;
        }
        visited += 1;
        if kinds.contains(&current.kind()) {
            counted += 1;
        }
        for child in current.children(&mut cursor) {
            stack.push(child);
        }
    }
    let _ = cursor;
    (counted, truncated)
}

/// Maximum nesting depth of any of the listed kinds inside `node`, which is the difference between
/// a flat long function and a deeply nested one.
pub fn max_depth_of(node: &Node, kinds: &[&str], budget: usize) -> (usize, bool) {
    let mut best = 0usize;
    let mut visited = 0usize;
    let mut truncated = false;
    let mut stack: Vec<(Node, usize)> = vec![(*node, 0)];
    while let Some((current, depth)) = stack.pop() {
        if visited >= budget {
            truncated = true;
            break;
        }
        visited += 1;
        let here = if kinds.contains(&current.kind()) {
            depth + 1
        } else {
            depth
        };
        best = best.max(here);
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            stack.push((child, here));
        }
    }
    (best, truncated)
}

/// Byte share of the tree that is error or missing nodes, which is how a "the parse returned nothing
/// usable" verdict is decided rather than guessed.
pub fn error_share(root: &Node, budget: usize) -> (usize, usize) {
    let mut error_bytes = 0usize;
    let mut visited = 0usize;
    let mut stack = vec![*root];
    while let Some(current) = stack.pop() {
        if visited >= budget {
            break;
        }
        visited += 1;
        if current.is_error() || current.is_missing() {
            error_bytes += current.byte_range().len();
            continue;
        }
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            stack.push(child);
        }
    }
    (error_bytes, visited)
}
