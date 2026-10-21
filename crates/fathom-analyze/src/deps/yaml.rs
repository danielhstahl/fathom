//! The indentation-based YAML subset two lockfiles need — and the reason it is not a real YAML parser
//! (e2.5 / reco-j83).
//!
//! `pnpm-lock.yaml` and `pnpm-workspace.yaml` are the only YAML in the ticket's format list (`yarn.lock`
//! is YAML-shaped but is its own grammar and gets its own parser in [`super::node`]). The honest options
//! were a YAML crate or something small: a crate is a new vendored dependency, an MSRV question and a
//! feature surface fathom does not need, and ADR-002 §7 holds that a dependency has to pay for itself.
//! What lockfiles actually use is four constructs — nested block mappings, block sequences, quoted or
//! plain scalars, and the occasional flow collection used for metadata — so this file implements those
//! four and refuses everything else in a way the caller can report.
//!
//! Refusing is the important part. A general YAML parser quietly succeeds on a document this module has
//! not been taught; this one returns an error with a line number, and the caller turns that into a typed
//! `unparsed` with the raw text attached, which is the outcome the ticket asks for.
//!
//! Supported: block mappings (`key: value`, `key:` + a deeper block), block sequences (`- item`,
//! `- key: value`), single/double quoting, `|` and `>` block scalars, blank and `#` comment lines,
//! `\r\n`. Anything else is an error naming its line.

use crate::deps::text::{Notes, is_flow, unquote};

/// The deepest nesting this subset will build.
///
/// Every level of a block mapping or block sequence is one frame of [`parse_block`] recursion, and the
/// indent that buys it is one space of indentation in a file nobody has to be sincere about. A 100 KB
/// `pnpm-lock.yaml` whose first 5 000 lines add one space each would otherwise overflow the stack of
/// whatever was reading it, and a stack overflow is a `SIGSEGV`, not an error the caller can report
/// (`e7.8`, threat model F-5). Real lockfiles nest a handful deep; pnpm's own deepest practical block is
/// about six. A hundred is generous and still 1 000× below the depth that hurts.
const MAX_DEPTH: usize = 100;

/// A parsed YAML value, deliberately minimal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A mapping, in document order — sorted order would lose the meaning of a lockfile's `importers`.
    Map(Vec<(String, Node)>),
    /// A sequence, in document order.
    Seq(Vec<Node>),
    /// A scalar, already unquoted. An empty scalar is what `key:` with no block parses to.
    Scalar(String),
}

impl Node {
    /// The value at `key` in a mapping.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The mapping's entries, or an empty slice.
    #[must_use]
    pub fn as_map(&self) -> &[(String, Node)] {
        match self {
            Node::Map(entries) => entries,
            _ => &[],
        }
    }

    /// The sequence's items, or an empty slice.
    #[must_use]
    pub fn as_seq(&self) -> &[Node] {
        match self {
            Node::Seq(items) => items,
            _ => &[],
        }
    }

    /// The scalar's text, or `None` for a collection.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Node::Scalar(text) => Some(text),
            _ => None,
        }
    }

    /// The scalar at `key`, if there is one.
    #[must_use]
    pub fn str_at(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Node::as_str)
    }

    /// A short description for a note: `mapping`, `sequence`, `scalar`.
    #[must_use]
    pub fn describe(&self) -> &'static str {
        match self {
            Node::Map(_) => "mapping",
            Node::Seq(_) => "sequence",
            Node::Scalar(_) => "scalar",
        }
    }
}

/// One structural line.
struct Line {
    /// Leading spaces.
    indent: usize,
    /// The line with indentation and trailing whitespace removed.
    text: String,
    /// 1-based line number, for the note or error that names it.
    number: usize,
}

/// Parse the supported subset.
///
/// The error is a one-line sentence with a line number in it; the caller prefixes whatever the format
/// needs and files it as a [`MalformedSyntax`](crate::deps::model::UnparsedReason::MalformedSyntax) or a
/// [`Truncated`](crate::deps::model::UnparsedReason::Truncated) reason depending on what it knows about
/// the read.
pub fn parse(text: &str, notes: &mut Notes) -> Result<Node, String> {
    let mut lines = Vec::new();
    for (index, raw) in crate::deps::text::lines(text).enumerate() {
        let trimmed = raw.trim_end();
        let stripped = trimmed.trim_start();
        if stripped.is_empty() || stripped.starts_with('#') {
            continue;
        }
        if raw.contains('\t') && raw[..raw.len() - raw.trim_start().len()].contains('\t') {
            return Err(format!(
                "line {}: tab indentation is not representable in this subset",
                index + 1
            ));
        }
        lines.push(Line {
            indent: trimmed.len() - stripped.len(),
            text: stripped.to_owned(),
            number: index + 1,
        });
    }
    if lines.is_empty() {
        return Ok(Node::Map(Vec::new()));
    }
    if lines[0].indent != 0 {
        return Err(format!(
            "line {}: the first structural line is indented",
            lines[0].number
        ));
    }
    let mut cursor = 0usize;
    let node = parse_block(&mut lines, &mut cursor, 0, 1, notes)?;
    if cursor != lines.len() {
        return Err(format!(
            "line {}: content left over at the top level",
            lines[cursor].number
        ));
    }
    Ok(node)
}

/// Parse the block that starts at `*cursor`.
fn parse_block(
    lines: &mut [Line],
    cursor: &mut usize,
    block: usize,
    depth: usize,
    notes: &mut Notes,
) -> Result<Node, String> {
    if depth > MAX_DEPTH {
        return Err(format!(
            "line {}: nested more than {MAX_DEPTH} levels deep, which is not a lockfile",
            lines[*cursor].number
        ));
    }
    let line = &lines[*cursor];
    if line.text == "-" || line.text.starts_with("- ") {
        parse_seq(lines, cursor, block, depth, notes)
    } else {
        parse_map(lines, cursor, block, depth, notes)
    }
}

fn parse_map(
    lines: &mut [Line],
    cursor: &mut usize,
    block: usize,
    depth: usize,
    notes: &mut Notes,
) -> Result<Node, String> {
    let mut entries: Vec<(String, Node)> = Vec::new();
    while *cursor < lines.len() {
        let line = &lines[*cursor];
        if line.indent < block {
            break;
        }
        if line.indent > block {
            return Err(format!("line {}: unexpected indentation", line.number));
        }
        if line.text == "-" || line.text.starts_with("- ") {
            return Err(format!(
                "line {}: a sequence item where a key was due",
                line.number
            ));
        }
        let number = line.number;
        let text = line.text.clone();
        let Some((key, value)) = split_key(&text) else {
            return Err(format!(
                "line {number}: expected `key: value`, found `{}`",
                crate::deps::text::clip(&text, 40)
            ));
        };
        *cursor += 1;
        let key = unquote(key).to_owned();
        let value = value.trim();
        if value.is_empty() {
            let has_child = lines.get(*cursor).is_some_and(|next| next.indent > block);
            if has_child {
                let child = parse_block(lines, cursor, lines[*cursor].indent, depth + 1, notes)?;
                entries.push((key, child));
            } else {
                entries.push((key, Node::Scalar(String::new())));
            }
            continue;
        }
        if value.starts_with('|') || value.starts_with('>') {
            let mut text = String::new();
            while lines.get(*cursor).is_some_and(|next| next.indent > block) {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(&lines[*cursor].text);
                *cursor += 1;
            }
            entries.push((key, Node::Scalar(text)));
            continue;
        }
        if is_flow(value) {
            notes.at(
                number,
                format!(
                    "flow collection `{}` kept as text, not parsed",
                    crate::deps::text::clip(value, 40)
                ),
            );
        }
        entries.push((key, Node::Scalar(unquote(value).to_owned())));
    }
    Ok(Node::Map(entries))
}

fn parse_seq(
    lines: &mut [Line],
    cursor: &mut usize,
    block: usize,
    depth: usize,
    notes: &mut Notes,
) -> Result<Node, String> {
    let mut items: Vec<Node> = Vec::new();
    while *cursor < lines.len() {
        let line = &lines[*cursor];
        if line.indent != block {
            if line.indent < block {
                break;
            }
            return Err(format!("line {}: unexpected indentation", line.number));
        }
        if !(line.text == "-" || line.text.starts_with("- ")) {
            break;
        }
        let number = line.number;
        let body = line.text[1..].to_owned();
        let content_col = block + 1 + (body.len() - body.trim_start().len());
        let body = body.trim().to_owned();
        if body.is_empty() {
            *cursor += 1;
            if lines.get(*cursor).is_some_and(|next| next.indent > block) {
                items.push(parse_block(
                    lines,
                    cursor,
                    lines[*cursor].indent,
                    depth + 1,
                    notes,
                )?);
                continue;
            }
            return Err(format!("line {number}: empty sequence item"));
        }
        if split_key(&body).is_some() {
            // `- key: value` (or `- key:` with a block under it): rewrite this line as the map's first
            // entry and let the mapping parser read it at the item's own column.
            lines[*cursor] = Line {
                indent: content_col,
                text: body,
                number,
            };
            items.push(parse_map(lines, cursor, content_col, depth + 1, notes)?);
            continue;
        }
        *cursor += 1;
        items.push(Node::Scalar(unquote(&body).to_owned()));
    }
    Ok(Node::Seq(items))
}

/// Split `key: value` outside quotes, requiring the YAML-mandated space after the colon (or an
/// end-of-line colon). That requirement is what lets a pnpm key like `'@babel/parser@7.23.0':` — and an
/// unquoted `react-dom@18.2.0:` — parse as a key rather than as a url.
fn split_key(text: &str) -> Option<(&str, &str)> {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let ch = bytes[index] as char;
        if ch == '"' || ch == '\'' {
            let quote = ch;
            index += 1;
            while index < bytes.len() {
                if bytes[index] as char == quote {
                    break;
                }
                index += 1;
            }
            index += 1;
            continue;
        }
        if ch == ':' && (index + 1 == bytes.len() || bytes[index + 1] == b' ') {
            return Some((&text[..index], &text[index + 1..]));
        }
        index += 1;
    }
    None
}
