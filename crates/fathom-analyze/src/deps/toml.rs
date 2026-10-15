//! TOML read as a *map*, not as a syntax tree (e2.5 / reco-j83).
//!
//! `toml_edit` is a document model built to preserve a file for rewriting: `Item::Table` for `[header]`
//! blocks, `Value::InlineTable` for `{ inline = "tables" }`, `Value::Array` for arrays, and a `Formatted<T>`
//! wrapper on every leaf so the original spacing survives a round-trip. That is exactly the wrong shape for
//! reading a manifest, because the same dependency can be written three ways and the form is a matter of
//! author style:
//!
//! ```toml
//! serde = "1"
//!
//! [dependencies]
//! serde = { version = "1", features = ["derive"] }
//!
//! [dependencies.serde]
//! version = "1"
//! ```
//!
//! A parser that walks `Item`s gets one of three shapes per dependency and has to handle all three. Getting
//! that wrong is not a crash: an inline-table dependency silently reads as "no version given", which is a
//! wrong answer rather than a loud one — and the first form above is what a Cargo.toml uses for its simplest
//! line, the second is what it uses for anything with a feature, so a whole real-world manifest can quietly
//! come back with no versions at all. (The first version of this module did exactly that, and the golden
//! fixture caught it.)
//!
//! So this module flattens the document once, into [`Map`] and [`Node`], where a table is a table no matter
//! which of the three forms it was written in and a leaf is a string/bool/int/float/array/whatever without a
//! `Formatted` wrapper in sight. Callers then read like the data: `map.str_at("version")`,
//! `map.node("features").and_then(|f| f.as_array())`.
//!
//! Nothing here is dependency-specific, and nothing here is lossy in a way the callers need: positions are
//! dropped (the `Span` model belongs to `fathom-config`, which is mapping YAML onto Rust and cannot afford to
//! lose them), and comments/whitespace are dropped with them, because a dependency manifest is read for its
//! facts and then thrown away.

use toml_edit::{Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use crate::deps::Failure;

/// Parse a TOML document, reporting a syntax failure the way `text` does.
///
/// `toml_edit`'s error carries a span when it can find one; the line number is what ends up in the note,
/// because that is the half a reviewer can act on.
pub fn parse(text: &str) -> Result<DocumentMut, Failure> {
    match text.parse::<DocumentMut>() {
        Ok(document) => Ok(document),
        Err(err) => {
            let offset = err.span().map_or(0, |span| span.start);
            let reason = crate::deps::text::toml_reason(&err.to_string(), offset, text);
            let detail = err.to_string().replace('\n', " ").trim().to_owned();
            let line = line_at(text, offset);
            Err(Failure::new(
                reason,
                if line == 0 {
                    detail
                } else {
                    format!("line {line}: {detail}")
                },
            ))
        }
    }
}

/// The 1-based line containing a byte offset, or 0 when the offset is past the end.
fn line_at(text: &str, offset: usize) -> usize {
    if offset > text.len() {
        return 0;
    }
    text[..offset].matches('\n').count() + 1
}

/// A TOML table seen as a key/value map, written as `[header]`, as `{ inline = true }`, or as the document
/// root.
#[derive(Debug, Clone, Copy)]
pub enum Map<'a> {
    Table(&'a Table),
    Inline(&'a InlineTable),
}

impl<'a> Map<'a> {
    /// View a value as a map if it is one — inline tables, and the pseudo-tables inside them.
    pub fn of_value(value: &'a Value) -> Option<Map<'a>> {
        match value {
            Value::InlineTable(inline) => Some(Map::Inline(inline)),
            _ => None,
        }
    }

    /// The value under `key`, if the key is present at all.
    pub fn node(&self, key: &str) -> Option<Node<'a>> {
        match self {
            // `Table::item` is not available as a bare lookup on `&Table` for a missing key: `iter` would
            // surface an `Item::None`, which is exactly the "absent" case callers must not see as present.
            Map::Table(table) => table.get(key).and_then(Node::of_item),
            Map::Inline(inline) => inline.get(key).map(Node::of_value),
        }
    }

    /// Every key/value pair, in document order.
    pub fn entries(&self) -> Vec<(String, Node<'a>)> {
        match self {
            Map::Table(table) => table
                .iter()
                .filter_map(|(key, item)| Node::of_item(item).map(|node| (key.to_owned(), node)))
                .collect(),
            Map::Inline(inline) => inline
                .iter()
                .map(|(key, value)| (key.to_owned(), Node::of_value(value)))
                .collect(),
        }
    }

    /// A string field.
    pub fn str_at(&self, key: &str) -> Option<&'a str> {
        self.node(key)?.as_str()
    }

    /// A boolean field.
    pub fn bool_at(&self, key: &str) -> Option<bool> {
        self.node(key)?.as_bool()
    }

    /// A field that may be a string or a list of strings, flattened.
    ///
    /// Manifests use one field for both shapes constantly — `[features] cli = "a"` and `cli = ["a", "b"]` are
    /// both "this feature turns on those things".
    pub fn strings_at(&self, key: &str) -> Vec<String> {
        match self.node(key) {
            Some(node) => node.as_strings(),
            None => Vec::new(),
        }
    }
}

/// Any TOML value, with the `Formatted` wrappers gone.
#[derive(Debug, Clone, Copy)]
pub enum Node<'a> {
    Str(&'a str),
    Int(i64),
    Float(f64),
    Bool(bool),
    Array(&'a Array),
    ArrayTables(&'a ArrayOfTables),
    Map(Map<'a>),
    /// Something the reader has no shape for. Present so a caller can still say "there is a value here" and
    /// describe it, rather than losing it.
    Other(&'a Value),
}

impl<'a> Node<'a> {
    fn of_item(item: &'a Item) -> Option<Node<'a>> {
        match item {
            Item::None => None,
            Item::Table(table) => Some(Node::Map(Map::Table(table))),
            Item::ArrayOfTables(tables) => Some(Node::ArrayTables(tables)),
            Item::Value(value) => Some(Node::of_value(value)),
        }
    }

    fn of_value(value: &'a Value) -> Node<'a> {
        match value {
            Value::String(text) => Node::Str(text.value().as_str()),
            Value::Integer(number) => Node::Int(*number.value()),
            Value::Float(number) => Node::Float(*number.value()),
            Value::Boolean(flag) => Node::Bool(*flag.value()),
            Value::Array(array) => Node::Array(array),
            Value::InlineTable(inline) => Node::Map(Map::Inline(inline)),
            _ => Node::Other(value),
        }
    }

    /// An array of tables, which is a list whose elements are all maps. Kept apart from `Array` because
    /// `[[package]]` blocks — Cargo.lock, poetry.lock — are the bulk of what reads it.
    pub fn as_array_tables(&self) -> Option<Vec<Map<'a>>> {
        match self {
            Node::ArrayTables(tables) => Some(tables.iter().map(Map::Table).collect()),
            // An inline array of inline tables (`source = { deps = [{ name = "x" }] }`) is the same shape.
            Node::Array(array) => {
                let mut out = Vec::with_capacity(array.len());
                for value in array.iter() {
                    out.push(Map::of_value(value)?);
                }
                (!out.is_empty()).then_some(out)
            }
            _ => None,
        }
    }

    /// A string, and only a string. Integers are *not* stringified here: a caller that wants "1" out of `1`
    /// says so, because a version field that is a number is usually a typo worth seeing.
    pub fn as_str(&self) -> Option<&'a str> {
        match self {
            Node::Str(text) => Some(text),
            _ => None,
        }
    }

    /// A string, or a number rendered the way it was written. This is what version fields want: `python = 3.10`
    /// is invalid in a real `pyproject.toml`, but it is also the kind of thing that appears in a half-edited
    /// one, and "3.1" as a requirement is closer to the truth than dropping the requirement.
    pub fn as_version(&self) -> Option<String> {
        match self {
            Node::Str(text) => Some((*text).to_owned()),
            Node::Int(number) => Some(number.to_string()),
            Node::Float(number) => Some(number.to_string()),
            Node::Bool(flag) => Some(flag.to_string()),
            _ => None,
        }
    }

    /// This node as a map, if it is one.
    pub fn as_map(&self) -> Option<Map<'a>> {
        match self {
            Node::Map(map) => Some(*map),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Node::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// The elements of a list, or `None` when this is not a list at all.
    ///
    /// The distinction matters when a field is *supposed* to be a list: `[dependencies]` written as a bare
    /// string is a mistake worth noting, and reading it as a one-element list would hide it.
    pub fn as_list(&self) -> Option<Vec<Node<'a>>> {
        match self {
            Node::Array(array) => Some(array.iter().map(Node::of_value).collect()),
            Node::ArrayTables(tables) => Some(
                tables
                    .iter()
                    .map(|table| Node::Map(Map::Table(table)))
                    .collect(),
            ),
            _ => None,
        }
    }

    /// The elements of an array, or a one-element list if this is a bare scalar.
    pub fn as_array(&self) -> Vec<Node<'a>> {
        self.as_list().unwrap_or_else(|| vec![*self])
    }

    /// A string, or every string in an array.
    pub fn as_strings(&self) -> Vec<String> {
        match self {
            Node::Array(_) | Node::ArrayTables(_) => self
                .as_array()
                .into_iter()
                .filter_map(|element| element.as_version())
                .collect(),
            other => other.as_version().into_iter().collect(),
        }
    }

    /// A short, human-readable rendering, for the notes that explain what was skipped.
    pub fn describe(&self) -> String {
        match self {
            Node::Str(text) => (*text).to_owned(),
            Node::Int(number) => number.to_string(),
            Node::Float(number) => number.to_string(),
            Node::Bool(flag) => flag.to_string(),
            Node::Array(array) => format!("[{} element(s)]", array.len()),
            Node::ArrayTables(tables) => format!("[{} table(s)]", tables.len()),
            Node::Map(map) => format!("{{{} field(s)}}", map.entries().len()),
            Node::Other(value) => value.to_string().trim().to_owned(),
        }
    }
}
