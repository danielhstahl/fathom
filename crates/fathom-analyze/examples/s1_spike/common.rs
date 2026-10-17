//! Spike `s1` — helpers the six throwaway extractors share.

use std::path::Path;

/// Text a node covers. Panics only on non-UTF-8 input, which the corpus does not contain;
/// this is a spike, and a spike that hides a decode failure is worse than one that shouts.
pub fn text<'a>(node: &tree_sitter::Node, src: &'a [u8]) -> &'a str {
    node.utf8_text(src).unwrap_or("<non-utf8>")
}

/// Child with a given field name.
pub fn field<'a>(node: &tree_sitter::Node<'a>, name: &str) -> Option<tree_sitter::Node<'a>> {
    node.child_by_field_name(name)
}

/// Direct named children, in order.
pub fn kids<'a>(node: &tree_sitter::Node<'a>) -> Vec<tree_sitter::Node<'a>> {
    let mut cursor = node.walk();
    node.named_children(&mut cursor).collect()
}

pub fn has_kind(node: &tree_sitter::Node, kind: &str) -> bool {
    kids(node).iter().any(|k| k.kind() == kind)
}

/// Join a dotted name onto a container prefix.
pub fn qualify(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        name.to_string()
    } else {
        format!("{prefix}.{name}")
    }
}

/// `Vec<T>` / `crate::geo::Point<'a>` / `*const u8` -> the base name a method belongs to.
pub fn base_type(ty: &str) -> String {
    let mut out = String::new();
    for ch in ty.trim().chars() {
        match ch {
            '<' | '(' | '[' | ' ' | '*' | '&' | '\'' | '{' => break,
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

pub fn parent_dir(path: &str) -> String {
    dir_of(path)
}

pub fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// `pkg/shape/shape.go` -> `pkg/shape`.
pub fn dir_of(path: &str) -> String {
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// `geo/mod.rs` -> `geo`; `lib.rs` -> ``; `geo/point.rs` -> `geo/point`.
pub fn stem(path: &str) -> String {
    let no_ext = match path.rfind('.') {
        Some(i) => &path[..i],
        None => path,
    };
    if no_ext.ends_with("/mod") || no_ext == "mod" {
        let trim = &no_ext[..no_ext.len() - 4];
        if trim.is_empty() {
            String::new()
        } else {
            trim.to_string()
        }
    } else {
        no_ext.to_string()
    }
}

pub fn extension(path: &str) -> String {
    match path.rfind('.') {
        Some(i) => path[i + 1..].to_string(),
        None => String::new(),
    }
}

/// True for `.d.ts`, which the TypeScript extractor treats as ambient.
pub fn is_d_ts(path: &str) -> bool {
    path.ends_with(".d.ts")
}
