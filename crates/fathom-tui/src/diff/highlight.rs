//! `fathom_tui::diff::highlight` — what a token is, to the depth a terminal-only highlighter can tell.
//!
//! This is not a parser and does not pretend to be one: it scans a single line, it has no memory of the
//! line above it, and its vocabulary is the six or seven faces a reviewer can actually use — keyword,
//! type, literal, number, comment, preprocessor, name. A `/* … */` that opens on the line above is
//! coloured from the marker down and no further, and the honest sentence about that is in
//! `docs/concepts/diff-viewer.md` rather than in a comment nobody reads.
//!
//! Why hand-rolled: the tree-sitter surface is `e3.1`'s and lives in `fathom-analyze`, which
//! `fathom-tui` may name only through `insight` (ADR-002 §5.2, lint `M02`). A diff pane cannot wait on
//! a grammar crate it is not allowed to link, and highlighting is presentation, so it lives with the
//! presentation layer. When `e3.1` publishes token spans through the delta model, the pane reads those
//! and this module shrinks to the fallback — the faces and the call site stay, the scanner goes.

use std::ops::Range;

use crate::style::Face;

/// The languages this scanner has an opinion about. Anything else is [`Language::Plain`], which means
/// "no faces", not "no diff".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    /// `.rs`
    Rust,
    /// `.py`
    Python,
    /// `.js`, `.jsx`, `.mjs`
    JavaScript,
    /// `.ts`, `.tsx`
    TypeScript,
    /// `.go`
    Go,
    /// `.rb`
    Ruby,
    /// `.java`, `.kt`, `.scala`
    Java,
    /// `.c`, `.h`
    C,
    /// `.cpp`, `.hpp`, `.cc`
    Cpp,
    /// `.json`
    Json,
    /// `.yaml`, `.yml`
    Yaml,
    /// `.toml`
    Toml,
    /// `.md`
    Markdown,
    /// `.sh`, `.bash`, `.zsh`
    Shell,
    /// Nothing we recognise, or a file with no extension.
    Plain,
}

impl Language {
    /// The language a path's extension implies. Unknown is `Plain`, and `Plain` still renders.
    #[must_use]
    pub fn for_path(path: &str) -> Language {
        let head = path.rsplit('/').next().unwrap_or(path);
        // Names without an extension of their own, so a `Dockerfile` is not "plain" by accident.
        match head {
            "Dockerfile" | "Containerfile" => return Language::Shell,
            "Makefile" | "CMakeLists.txt" => return Language::Shell,
            _ => {}
        }
        let extension = match head.rsplit_once('.') {
            Some((_, extension)) => extension.to_ascii_lowercase(),
            None => return Language::Plain,
        };
        match extension.as_str() {
            "rs" => Language::Rust,
            "py" | "pyi" => Language::Python,
            "js" | "jsx" | "mjs" | "cjs" | "vue" => Language::JavaScript,
            "ts" | "tsx" => Language::TypeScript,
            "go" => Language::Go,
            "rb" | "rake" | "gemspec" => Language::Ruby,
            "java" | "kt" | "kts" | "scala" => Language::Java,
            "c" | "h" => Language::C,
            "cpp" | "cxx" | "cc" | "hpp" | "hh" => Language::Cpp,
            "json" | "jsonc" | "json5" => Language::Json,
            "yaml" | "yml" => Language::Yaml,
            "toml" => Language::Toml,
            "md" | "markdown" | "mdx" => Language::Markdown,
            "sh" | "bash" | "zsh" | "fish" => Language::Shell,
            _ => Language::Plain,
        }
    }

    /// The name the strip prints, so a reviewer can see what the pane thought the file was.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::JavaScript => "javascript",
            Language::TypeScript => "typescript",
            Language::Go => "go",
            Language::Ruby => "ruby",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "c++",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Toml => "toml",
            Language::Markdown => "markdown",
            Language::Shell => "shell",
            Language::Plain => "plain",
        }
    }

    /// The shape of the language's comments and literals.
    const fn spec(self) -> Spec {
        Spec {
            keywords: match self {
                Language::Rust => &[
                    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else",
                    "enum", "extern", "false", "fn", "for", "if", "impl", "in", "let", "loop",
                    "match", "mod", "move", "mut", "pub", "ref", "return", "self", "Self",
                    "static", "struct", "super", "trait", "true", "type", "union", "unsafe", "use",
                    "where", "while", "async",
                ],
                Language::Python => &[
                    "and", "as", "assert", "async", "await", "break", "class", "continue", "def",
                    "del", "elif", "else", "except", "False", "finally", "for", "from", "global",
                    "if", "import", "in", "is", "lambda", "None", "nonlocal", "not", "or", "pass",
                    "raise", "return", "True", "try", "while", "with", "yield", "self", "cls",
                ],
                Language::JavaScript | Language::TypeScript => &[
                    "async",
                    "await",
                    "break",
                    "case",
                    "catch",
                    "class",
                    "const",
                    "continue",
                    "debugger",
                    "default",
                    "delete",
                    "do",
                    "else",
                    "enum",
                    "export",
                    "extends",
                    "false",
                    "finally",
                    "for",
                    "function",
                    "if",
                    "import",
                    "in",
                    "instanceof",
                    "interface",
                    "let",
                    "new",
                    "null",
                    "of",
                    "package",
                    "private",
                    "protected",
                    "public",
                    "return",
                    "static",
                    "super",
                    "switch",
                    "this",
                    "throw",
                    "true",
                    "try",
                    "type",
                    "typeof",
                    "var",
                    "void",
                    "while",
                    "with",
                    "yield",
                ],
                Language::Go => &[
                    "break",
                    "case",
                    "chan",
                    "const",
                    "continue",
                    "default",
                    "defer",
                    "else",
                    "fallthrough",
                    "for",
                    "func",
                    "go",
                    "goto",
                    "if",
                    "import",
                    "interface",
                    "map",
                    "package",
                    "range",
                    "return",
                    "select",
                    "struct",
                    "switch",
                    "type",
                    "var",
                    "true",
                    "false",
                    "nil",
                    "string",
                    "error",
                    "int",
                    "int64",
                    "byte",
                    "bool",
                ],
                Language::Ruby => &[
                    "alias",
                    "and",
                    "begin",
                    "break",
                    "case",
                    "class",
                    "def",
                    "defined?",
                    "do",
                    "else",
                    "elsif",
                    "end",
                    "ensure",
                    "false",
                    "for",
                    "if",
                    "in",
                    "module",
                    "next",
                    "nil",
                    "not",
                    "or",
                    "redo",
                    "rescue",
                    "retry",
                    "return",
                    "self",
                    "super",
                    "then",
                    "true",
                    "undef",
                    "unless",
                    "until",
                    "when",
                    "while",
                    "yield",
                    "require",
                    "require_relative",
                ],
                Language::Java => &[
                    "abstract",
                    "assert",
                    "boolean",
                    "break",
                    "byte",
                    "case",
                    "catch",
                    "char",
                    "class",
                    "const",
                    "continue",
                    "default",
                    "do",
                    "double",
                    "else",
                    "enum",
                    "extends",
                    "false",
                    "final",
                    "finally",
                    "float",
                    "for",
                    "goto",
                    "if",
                    "implements",
                    "import",
                    "instanceof",
                    "int",
                    "interface",
                    "long",
                    "new",
                    "null",
                    "package",
                    "private",
                    "protected",
                    "public",
                    "return",
                    "short",
                    "static",
                    "super",
                    "switch",
                    "this",
                    "throw",
                    "throws",
                    "transient",
                    "true",
                    "try",
                    "void",
                    "while",
                    "fun",
                    "val",
                    "var",
                    "object",
                    "override",
                    "when",
                ],
                Language::C | Language::Cpp => &[
                    "auto",
                    "bool",
                    "break",
                    "case",
                    "catch",
                    "char",
                    "class",
                    "const",
                    "consteval",
                    "constexpr",
                    "continue",
                    "decltype",
                    "default",
                    "delete",
                    "do",
                    "double",
                    "else",
                    "enum",
                    "explicit",
                    "extern",
                    "false",
                    "float",
                    "for",
                    "friend",
                    "goto",
                    "if",
                    "inline",
                    "int",
                    "long",
                    "namespace",
                    "new",
                    "noexcept",
                    "nullptr",
                    "operator",
                    "private",
                    "protected",
                    "public",
                    "register",
                    "return",
                    "short",
                    "signed",
                    "sizeof",
                    "static",
                    "struct",
                    "switch",
                    "template",
                    "this",
                    "throw",
                    "true",
                    "try",
                    "typedef",
                    "typename",
                    "union",
                    "unsigned",
                    "using",
                    "virtual",
                    "void",
                    "volatile",
                    "while",
                ],
                Language::Json => &["true", "false", "null"],
                Language::Yaml => &[
                    "true", "false", "null", "yes", "no", "on", "off", "version", "kind",
                    "metadata", "spec",
                ],
                Language::Toml => &[
                    "true",
                    "false",
                    "string",
                    "integer",
                    "float",
                    "boolean",
                    "datetime",
                    "default",
                    "features",
                    "dependencies",
                    "workspace",
                    "package",
                    "patch",
                    "profile",
                ],
                Language::Shell => &[
                    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done",
                    "case", "esac", "function", "in", "local", "export", "return", "set", "echo",
                    "exit", "source", "trap", "readonly",
                ],
                Language::Markdown | Language::Plain => &[],
            },
            line_comment: match self {
                Language::Rust
                | Language::JavaScript
                | Language::TypeScript
                | Language::Go
                | Language::Java
                | Language::C
                | Language::Cpp => &["//"],
                Language::Python | Language::Ruby | Language::Shell | Language::Yaml => &["#"],
                Language::Toml => &["#"],
                Language::Markdown => &[],
                Language::Json | Language::Plain => &[],
            },
            quotes: match self {
                Language::Python => "'\"",
                Language::Ruby => "'\"",
                Language::Shell => "'\"",
                Language::Markdown => "",
                Language::Plain => "",
                _ => "\"",
            },
            preproc: matches!(self, Language::C | Language::Cpp | Language::Rust),
            block_comment: match self {
                Language::Rust
                | Language::JavaScript
                | Language::TypeScript
                | Language::Go
                | Language::Java
                | Language::C
                | Language::Cpp => Some(("/*", "*/")),
                _ => None,
            },
        }
    }
}

/// The per-language knobs.
struct Spec {
    keywords: &'static [&'static str],
    line_comment: &'static [&'static str],
    quotes: &'static str,
    preproc: bool,
    block_comment: Option<(&'static str, &'static str)>,
}

/// Token faces for one line, in byte ranges, ordered and non-overlapping.
///
/// An empty `Vec` means "no faces", which is what [`Language::Plain`] and any line the scanner cannot
/// make sense of both produce — never a guess.
#[must_use]
pub fn tokens(line: &str, language: Language) -> Vec<(Range<usize>, Face)> {
    sanitize(line, scanned(line, language))
}

/// Drop any range that does not land on character boundaries, and any that is empty.
///
/// The scanners below walk bytes, and a byte offset is only a *character* offset while the text is
/// ASCII. Patch text is whatever somebody committed, so the exit checks it: a face that cannot be
/// applied is skipped, and the line is still drawn. Colour is the last thing this pane trusts.
fn sanitize(line: &str, ranges: Vec<(Range<usize>, Face)>) -> Vec<(Range<usize>, Face)> {
    ranges
        .into_iter()
        .filter_map(|(range, face)| {
            let start = boundary(line, range.start);
            let end = boundary(line, range.end);
            (start < end)
                .then_some(start..end)
                .map(|clean| (clean, face))
        })
        .collect()
}

fn scanned(line: &str, language: Language) -> Vec<(Range<usize>, Face)> {
    if language == Language::Plain || line.is_empty() {
        return Vec::new();
    }
    let spec = language.spec();
    let mut out: Vec<(Range<usize>, Face)> = Vec::new();

    // A line whose first non-space is a comment marker is a comment, whole.
    let head = line.trim_start();
    let indent = line.len() - head.len();
    if language == Language::Markdown {
        return markdown(line);
    }
    for marker in spec.line_comment {
        if head.starts_with(marker) {
            out.push((indent..line.len(), Face::Comment));
            return out;
        }
    }
    if spec.preproc && head.starts_with('#') {
        // `#include`, `#define`, and Rust's `#[cfg(...)]` — a directive, not a comment.
        out.push((indent..line.len(), Face::Preproc));
        return out;
    }
    for marker in spec.line_comment {
        if let Some(offset) = find_outside_string(line, marker, spec.quotes) {
            out.push((offset..line.len(), Face::Comment));
            let here = &mut out;
            scan_code(
                &line[..offset],
                &mut |range, face| here.push((range, face)),
                &spec,
                0,
            );
            return out;
        }
    }
    if let Some((open, _close)) = spec.block_comment
        && let Some(offset) = line.find(open)
    {
        // One line, no memory of the last: colour from the marker to the end and say nothing more.
        scan_code(
            &line[..offset],
            &mut |range, face| out.push((range, face)),
            &spec,
            0,
        );
        out.push((offset..line.len(), Face::Comment));
        return out;
    }
    if spec.quotes.contains('"') && language == Language::Json {
        return json(line);
    }
    scan_code(line, &mut |range, face| out.push((range, face)), &spec, 0);
    out
}

/// Scan code that is not inside a comment.
fn scan_code(text: &str, emit: &mut dyn FnMut(Range<usize>, Face), spec: &Spec, base: usize) {
    let bytes = text.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            index += 1;
            continue;
        }
        if spec.quotes.contains(byte as char) {
            let quote = byte;
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = boundary(text, index + 2);
                    continue;
                }
                if bytes[index] == quote {
                    index += 1;
                    break;
                }
                index += 1;
            }
            emit(
                (base + start)..(base + index).min(text.len()),
                Face::Literal,
            );
            continue;
        }
        if byte.is_ascii_digit() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'.' | b'_' | b'x' | b'-'))
            {
                index += 1;
            }
            emit(base + start..base + index, Face::Number);
            continue;
        }
        if byte == b'#' && spec.preproc {
            emit(base + index..base + text.len(), Face::Preproc);
            return;
        }
        if is_ident_start(byte) {
            let start = index;
            while index < bytes.len() && is_ident_byte(bytes[index]) {
                index += 1;
            }
            let word = &text[start..index];
            let face = if spec.keywords.contains(&word) {
                Face::Keyword
            } else if looks_like_name(text, index) {
                Face::Function
            } else if word.chars().next().is_some_and(char::is_uppercase) {
                Face::Type
            } else {
                continue;
            };
            emit(base + start..base + index, face);
            continue;
        }
        index += 1;
    }
}

/// `foo(` — the name in a call or a definition.
fn looks_like_name(text: &str, after: usize) -> bool {
    text[after..].starts_with('(')
}

const fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_' || byte >= 0x80
}

const fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'?' || byte == b'!' || byte >= 0x80
}

/// Does `marker` start here? Only ever asked at a character boundary: a byte walk that steps through
/// a multi-byte character would otherwise slice the string in half, and that is a panic, not a miss.
#[must_use]
fn is_at(line: &str, index: usize, marker: &str) -> bool {
    line.is_char_boundary(index) && line[index..].starts_with(marker)
}

/// Move to the next character boundary at or after `index`.
///
/// A byte scanner that skips an escape pair can land in the middle of a multi-byte character, and
/// slicing there panics. Patch text is not ours to keep ASCII — a `\u{2603}` inside a string literal
/// in somebody's diff is a normal Tuesday, and a highlighter that crashes the pane is not.
fn boundary(line: &str, index: usize) -> usize {
    let mut index = index.min(line.len());
    while index < line.len() && !line.is_char_boundary(index) {
        index += 1;
    }
    index
}

/// Find a marker that is not inside a string literal. Cheap, and good enough for one line.
fn find_outside_string(line: &str, marker: &str, quotes: &str) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut open: Option<u8> = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(quote) = open {
            if byte == b'\\' {
                index = boundary(line, index + 2);
                continue;
            }
            if byte == quote {
                open = None;
            }
        } else if quotes.contains(byte as char) {
            open = Some(byte);
        } else if is_at(line, index, marker) {
            return Some(index);
        }
        index += 1;
    }
    None
}

/// JSON: a `"key":` is a name, everything else that is quoted is a literal.
fn json(line: &str) -> Vec<(Range<usize>, Face)> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'"' {
            let start = index;
            index += 1;
            while index < bytes.len() {
                if bytes[index] == b'\\' {
                    index = boundary(line, index + 2);
                    continue;
                }
                if bytes[index] == b'"' {
                    index += 1;
                    break;
                }
                index += 1;
            }
            let face = if line[index..].trim_start().starts_with(':') {
                Face::Type
            } else {
                Face::Literal
            };
            out.push((start..index, face));
            continue;
        }
        if bytes[index].is_ascii_digit() {
            let start = index;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'.')
            {
                index += 1;
            }
            out.push((start..index, Face::Number));
            continue;
        }
        if bytes[index].is_ascii_alphabetic() {
            let start = index;
            while index < bytes.len() && bytes[index].is_ascii_alphanumeric() {
                index += 1;
            }
            let word = &line[start..index];
            if word == "true" || word == "false" || word == "null" {
                out.push((start..index, Face::Keyword));
            }
            continue;
        }
        index += 1;
    }
    out
}

/// Markdown: a heading, a list bullet, a fence, or inline code. Enough that a changed `README.md`
/// line does not read as prose noise.
fn markdown(line: &str) -> Vec<(Range<usize>, Face)> {
    let mut out = Vec::new();
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.starts_with('#') {
        out.push((indent..line.len(), Face::Keyword));
        return out;
    }
    if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
        out.push((indent..line.len(), Face::Preproc));
        return out;
    }
    if trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) && trimmed.contains(". ")
    {
        let cut = indent + trimmed.find(' ').unwrap_or(1) + 1;
        out.push((indent..cut.min(line.len()), Face::Marker));
    }
    let mut index = 0;
    while let Some(found) = line[index..].find('`') {
        let start = index + found;
        match line[start + 1..].find('`') {
            Some(offset) => {
                out.push((start..start + offset + 2, Face::Literal));
                index = start + offset + 2;
            }
            None => break,
        }
    }
    out
}
