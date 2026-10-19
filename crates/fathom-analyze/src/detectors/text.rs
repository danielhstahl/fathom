//! `text.rs` — the shared lexical primitives, and nothing else.
//!
//! Every detector reads lines, and every detector is tempted to write its own "is this a comment". This
//! module is those few functions written once, so that "the corpus says this is a comment" is one answer
//! and not twelve slightly different ones — the ADR-001 §7.10 argument, applied at the smallest scale in
//! the crate.
//!
//! # The honesty these functions are for
//!
//! None of them parses. There is no tree-sitter here, deliberately: the `lang-*` features are optional
//! (ADR-002 §6.2), a detector that needed a parse would be a detector that silently does nothing in the
//! `minimal` build, and every claim a detector makes about *syntax* has to be priced as the guess it is
//! (see each detector's own confidence). What a line-level claim can carry is a *lexical* fact — a
//! `#[allow(...)]` is a directive whether or not the parser likes it — and what it cannot is recorded in
//! the detector that made it, not here.
//!
//! The one thing this module does insist on: a function that answers about a *line* never answers about a
//! whole file, and a function that answers about a dialect never guesses one from a path it does not
//! recognise. [`Dialect::Unknown`] is a real answer and callers must handle it.

use std::collections::BTreeSet;

/// The language family a path looks like.
///
/// Guessing from the suffix is exactly as good as it sounds and is priced that way. It buys two things:
/// comment vocabulary that does not fire inside a shell script's `#`, and the ability to refuse to look
/// for debug prints in a Markdown file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    Rust,
    Python,
    Ruby,
    Go,
    Java,
    Kotlin,
    CSharp,
    JavaScriptTypeScript,
    Swift,
    Php,
    Shell,
    /// YAML, TOML, JSON, INI: configuration with comments, no statements.
    Config,
    /// Markdown, rst, adoc: prose. Debug markers and secrets in prose are quoted, not planted.
    Prose,
    /// C and its relatives, including headers.
    C,
    /// Nothing recognised. A detector that needs a dialect and gets this says nothing.
    Unknown,
}

impl Dialect {
    /// The published name, used in evidence details.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Dialect::Rust => "rust",
            Dialect::Python => "python",
            Dialect::Ruby => "ruby",
            Dialect::Go => "go",
            Dialect::Java => "java",
            Dialect::Kotlin => "kotlin",
            Dialect::CSharp => "csharp",
            Dialect::JavaScriptTypeScript => "js/ts",
            Dialect::Swift => "swift",
            Dialect::Php => "php",
            Dialect::Shell => "shell",
            Dialect::Config => "config",
            Dialect::Prose => "prose",
            Dialect::C => "c",
            Dialect::Unknown => "unknown",
        }
    }

    /// Does anything in this family have statements a person can leave a debug print in?
    #[must_use]
    pub const fn is_code(self) -> bool {
        !matches!(self, Dialect::Config | Dialect::Prose | Dialect::Unknown)
    }
}

/// The dialect a path looks like, from the suffix table. `None` rather than [`Dialect::Unknown`] so a
/// caller can tell "no suffix" from "a suffix this table does not know".
#[must_use]
pub fn dialect(path: &str) -> Option<Dialect> {
    let name = path.rsplit('/').next().unwrap_or(path).to_ascii_lowercase();
    for (exact, dialect) in EXACT_NAMES {
        if name == *exact {
            return Some(*dialect);
        }
    }
    let (_, ext) = name.rsplit_once('.')?;
    SUFFIXES
        .iter()
        .find(|(suffix, _)| *suffix == ext)
        .map(|(_, dialect)| *dialect)
}

const EXACT_NAMES: &[(&str, Dialect)] = &[
    ("dockerfile", Dialect::Config),
    ("makefile", Dialect::Shell),
    ("rakefile", Dialect::Ruby),
    ("gemfile", Dialect::Ruby),
    ("justfile", Dialect::Shell),
];

const SUFFIXES: &[(&str, Dialect)] = &[
    ("rs", Dialect::Rust),
    ("py", Dialect::Python),
    ("pyi", Dialect::Python),
    ("rb", Dialect::Ruby),
    ("go", Dialect::Go),
    ("java", Dialect::Java),
    ("kt", Dialect::Kotlin),
    ("kts", Dialect::Kotlin),
    ("cs", Dialect::CSharp),
    ("js", Dialect::JavaScriptTypeScript),
    ("jsx", Dialect::JavaScriptTypeScript),
    ("ts", Dialect::JavaScriptTypeScript),
    ("tsx", Dialect::JavaScriptTypeScript),
    ("mjs", Dialect::JavaScriptTypeScript),
    ("swift", Dialect::Swift),
    ("php", Dialect::Php),
    ("sh", Dialect::Shell),
    ("bash", Dialect::Shell),
    ("zsh", Dialect::Shell),
    ("c", Dialect::C),
    ("h", Dialect::C),
    ("cc", Dialect::C),
    ("cpp", Dialect::C),
    ("hpp", Dialect::C),
    ("yaml", Dialect::Config),
    ("yml", Dialect::Config),
    ("toml", Dialect::Config),
    ("json", Dialect::Config),
    ("ini", Dialect::Config),
    ("cfg", Dialect::Config),
    ("md", Dialect::Prose),
    ("markdown", Dialect::Prose),
    ("rst", Dialect::Prose),
    ("adoc", Dialect::Prose),
    ("txt", Dialect::Prose),
];

/// Line-leading comment markers per dialect. A line is a comment when its *first* non-blank content is
/// one of these; a trailing comment is not a comment line, which is why [`strip_inline_comment`] exists
/// separately.
#[must_use]
pub fn comment_markers(dialect: Dialect) -> &'static [&'static str] {
    match dialect {
        Dialect::Rust
        | Dialect::Go
        | Dialect::Java
        | Dialect::Kotlin
        | Dialect::CSharp
        | Dialect::JavaScriptTypeScript
        | Dialect::Swift
        | Dialect::C => &["//", "/*", "*"],
        Dialect::Python | Dialect::Ruby | Dialect::Shell | Dialect::Config => &["#"],
        Dialect::Php => &["//", "#", "/*", "*"],
        Dialect::Prose => &[],
        Dialect::Unknown => &["//", "#", "--", "/*", "*"],
    }
}

/// Is the line blank or entirely a comment?
#[must_use]
pub fn is_blank_or_comment(line: &str, dialect: Dialect) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return true;
    }
    comment_markers(dialect)
        .iter()
        .any(|marker| trimmed.starts_with(marker))
}

/// The line with its trailing line-comment removed. Only for the two-family case where the marker is
/// unambiguous outside a string: `//` for C-family, `#` for the hash families. Quote-awareness is a
/// parser's job; this is stated as a heuristic by the detectors that use it.
#[must_use]
pub fn strip_inline_comment(line: &str, dialect: Dialect) -> &str {
    let marker = match dialect {
        Dialect::Rust
        | Dialect::Go
        | Dialect::Java
        | Dialect::Kotlin
        | Dialect::CSharp
        | Dialect::JavaScriptTypeScript
        | Dialect::Swift
        | Dialect::C
        | Dialect::Php => "//",
        Dialect::Python | Dialect::Ruby | Dialect::Shell => "#",
        // Prose has no comment marker, and `Unknown` has too many: cutting there would eat a URL in a
        // config value or a `--flag` in a shell string, so it is left whole.
        Dialect::Prose | Dialect::Config | Dialect::Unknown => return line,
    };
    let bytes = line.as_bytes();
    let marker = marker.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 0_usize;
    while index < bytes.len() {
        let byte = bytes[index];
        match quote {
            Some(_) if byte == b'\\' => index += 2,
            Some(open) if byte == open => {
                quote = None;
                index += 1;
            }
            Some(_) => index += 1,
            None if matches!(byte, b'"' | b'\'' | b'`') => {
                quote = Some(byte);
                index += 1;
            }
            None if bytes[index..].starts_with(marker) => return line[..index].trim_end(),
            None => index += 1,
        }
    }
    line
}

/// The line folded for comparison: outer whitespace gone, inner runs of whitespace collapsed to one
/// space. The equality a rework detector claims is on lines that differ only in indentation, because an
/// agent that re-indents a block it just moved has not changed it.
#[must_use]
pub fn normalise(line: &str) -> String {
    line.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalise a list of lines and drop the blanks. Returns a set, because every caller of this in the
/// catalogue is asking "do these two episodes' line sets overlap".
#[must_use]
pub fn line_set(lines: &[String]) -> BTreeSet<String> {
    lines
        .iter()
        .map(|line| normalise(line))
        .filter(|line| !line.is_empty())
        .collect()
}

/// The Jaccard overlap of two line sets: shared over union, `0.0` when either is empty.
///
/// Jaccard rather than "share of the older episode", because a revert of a three-line commit inside a
/// nine-hundred-line episode is not a revert *of that episode*, and a subset measure would say it was.
#[must_use]
pub fn overlap(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        0.0
    } else {
        shared as f64 / union as f64
    }
}

/// Do these lines carry meaning, or only punctuation and comments?
///
/// The copy detector's cheapest false-positive guard: a run of `}` and `,` repeats everywhere and means
/// nothing, and a block has to be mostly tokens before "copied" is a claim worth a row.
#[must_use]
pub fn substantive(lines: &[String], dialect: Dialect) -> bool {
    let mut letters = 0_usize;
    let mut total = 0_usize;
    for line in lines {
        if is_blank_or_comment(line, dialect) {
            continue;
        }
        let folded = normalise(line);
        total += folded.chars().count();
        letters += folded.chars().filter(char::is_ascii_alphabetic).count();
    }
    total > 0 && letters.saturating_mul(2) >= total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_suffix_names_a_dialect_and_an_absent_one_names_none() {
        assert_eq!(dialect("src/parser.rs"), Some(Dialect::Rust));
        assert_eq!(dialect("tests/test_api.py"), Some(Dialect::Python));
        assert_eq!(dialect("docs/README.md"), Some(Dialect::Prose));
        assert_eq!(dialect("Dockerfile"), Some(Dialect::Config));
        assert_eq!(dialect("LICENSE"), None, "no dot, no guess");
        assert_eq!(dialect("thing.weird"), None);
        assert!(!Dialect::Prose.is_code());
        assert!(Dialect::Rust.is_code());
    }

    #[test]
    fn a_hash_is_a_comment_in_python_and_not_in_rust() {
        assert!(is_blank_or_comment("# note", Dialect::Python));
        assert!(!is_blank_or_comment("# note", Dialect::Rust));
        assert!(is_blank_or_comment("  // note", Dialect::Rust));
        assert!(is_blank_or_comment("/// doc", Dialect::Rust));
        assert!(
            is_blank_or_comment("", Dialect::Python),
            "a blank line is not content either"
        );
        assert!(
            !is_blank_or_comment("# a secret", Dialect::Prose),
            "prose has no comment marker, so nothing in a Markdown file is 'only a comment'"
        );
    }

    #[test]
    fn an_inline_comment_is_cut_at_the_marker() {
        assert_eq!(
            strip_inline_comment("let x = 1; // set it", Dialect::Rust),
            "let x = 1;"
        );
        assert_eq!(
            strip_inline_comment("x = 1  # set it", Dialect::Python),
            "x = 1"
        );
        assert_eq!(
            strip_inline_comment("no marker here", Dialect::Rust),
            "no marker here"
        );
    }

    #[test]
    fn indentation_is_not_a_change() {
        assert_eq!(normalise("   let   x = 1;  "), normalise("let x = 1;"));
        assert_eq!(
            overlap(
                &line_set(&["a".to_owned(), "b".to_owned()]),
                &line_set(&["  a".to_owned()])
            ),
            0.5,
            "shared over union: two lines, one shared"
        );
        assert_eq!(overlap(&line_set(&[]), &line_set(&["a".to_owned()])), 0.0);
    }

    #[test]
    fn a_revert_is_a_full_overlap_and_a_nesting_is_not() {
        let added = line_set(&[
            "fn a() {".to_owned(),
            "return 1;".to_owned(),
            "}".to_owned(),
        ]);
        let reverted = added.clone();
        assert!((overlap(&added, &reverted) - 1.0).abs() < f64::EPSILON);
        let bigger = line_set(&[
            "fn a() {".to_owned(),
            "return 1;".to_owned(),
            "}".to_owned(),
            "fn b() {}".to_owned(),
            "fn c() {}".to_owned(),
        ]);
        assert!(
            overlap(&added, &bigger) < 0.7,
            "three shared lines inside a five-line episode is not the same change: {}",
            overlap(&added, &bigger)
        );
    }

    #[test]
    fn braces_are_not_substance() {
        let braces: Vec<String> = ["}", "}", "{", ",", "}"]
            .iter()
            .map(|s| (*s).to_owned())
            .collect();
        assert!(!substantive(&braces, Dialect::Rust));
        let real: Vec<String> = [
            "let mut seen = HashMap::new();".to_owned(),
            "for item in items { seen.insert(item.id, item); }".to_owned(),
        ]
        .to_vec();
        assert!(substantive(&real, Dialect::Rust));
        let commented: Vec<String> = ["// one".to_owned(), "// two".to_owned()].to_vec();
        assert!(
            !substantive(&commented, Dialect::Rust),
            "a comment block repeating itself is documentation, not copy-paste growth"
        );
    }
}
