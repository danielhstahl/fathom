//! The shapes `e3.1`'s structural extraction agrees on: the normalised symbol, the visibility,
//! the import edge, the "cannot enumerate" marker, and the per-file result.
//!
//! Everything here is data, and every type serialises in a fixed order (`BTreeMap`, sorted `Vec`,
//! field order written by hand), because the whole point of a per-file structural model is that a
//! later revision can be *diffed* against an earlier one — in `e3.2`'s graph, in `e3.7`'s
//! breaking-change report, and in this module's own snapshot test. A hash map in any of these
//! shapes makes byte-equality a coin flip, so it is not used.
//!
//! The normalisation rules these shapes commit to are written in
//! [`docs/concepts/structure-extraction.md`](../../../../docs/concepts/structure-extraction.md)
//! ("The normalisation rules the corpus commits to") and are inherited from spike `s1` with the
//! corpus itself:
//!
//! * identity is `(file, kind, dotted name)`;
//! * kinds are one closed vocabulary of ten, mapped per language;
//! * declarations inside a function body are not symbols;
//! * fields are not symbols anywhere, but accessors count as methods;
//! * members are excluded from the export set;
//! * an export is a name nameable by code outside the declaring unit, per that language's own rules.

use crate::structure::policy::ExtractPolicy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The kind vocabulary: ten names, not each language's own sixty.
///
/// `e3.2` builds one node type per kind, so an extractor that cannot say which of these ten a
/// declaration is has not told the graph anything. A Rust `trait` and a Java `interface` are both
/// [`SymbolKind::Interface`]; a Rust `mod` is [`SymbolKind::Module`]; a Go `type X struct` is
/// [`SymbolKind::Struct`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    /// A module or namespace: a Rust `mod`, a Python module's own scope, a TS `namespace`.
    Module,
    /// A class, in whatever spelling the language uses for it.
    Class,
    /// A contract: a Rust `trait`, a Java or TypeScript `interface`.
    Interface,
    /// A struct or record, including a Go `type X struct` and a Rust `union`.
    Struct,
    /// An enumerated set of cases.
    Enum,
    /// A type alias or type synonym.
    Type,
    /// A free function (module-level, not attached to a type).
    Fn,
    /// A function attached to a type: a method, an associated fn, an accessor.
    Method,
    /// A binding the language says does not change.
    Const,
    /// A binding that does change.
    Var,
}

impl SymbolKind {
    /// The closed vocabulary, in the order the documentation lists it.
    pub const ALL: [SymbolKind; 10] = [
        SymbolKind::Module,
        SymbolKind::Class,
        SymbolKind::Interface,
        SymbolKind::Struct,
        SymbolKind::Enum,
        SymbolKind::Type,
        SymbolKind::Fn,
        SymbolKind::Method,
        SymbolKind::Const,
        SymbolKind::Var,
    ];

    /// The stable spelling, which is also the JSON and truth-file spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Class => "class",
            SymbolKind::Interface => "interface",
            SymbolKind::Struct => "struct",
            SymbolKind::Enum => "enum",
            SymbolKind::Type => "type",
            SymbolKind::Fn => "fn",
            SymbolKind::Method => "method",
            SymbolKind::Const => "const",
            SymbolKind::Var => "var",
        }
    }

    /// Parse the closed vocabulary. `None` means the caller handed a string from outside the
    /// contract (a hand-authored truth file with a typo), which is a stated failure, not a
    /// fallback kind.
    pub fn parse(text: &str) -> Option<SymbolKind> {
        SymbolKind::ALL.into_iter().find(|k| k.as_str() == text)
    }

    /// Whether this kind is a container in the "may hold members" sense, which is the sense
    /// `e3.7` needs when it asks whether a member's parent was removed.
    pub fn is_type_like(self) -> bool {
        matches!(
            self,
            SymbolKind::Class
                | SymbolKind::Interface
                | SymbolKind::Struct
                | SymbolKind::Enum
                | SymbolKind::Module
        )
    }
}

/// A source language, as far as extraction cares.
///
/// All six of ADR-002 §6.5's grammars are *named* here even where this build has no extractor for
/// them: detection is cheap and honest, and "this is Java, and this build cannot read Java" is a
/// different answer from "this is nothing". The three without an extractor are `e3.1b`
/// (`reco-dlp.1`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    /// Rust, `.rs`.
    Rust,
    /// Python, `.py` and `.pyi`.
    Python,
    /// TypeScript, `.ts`, `.mts`, `.cts`, `.tsx`, and `.d.ts` (see [`crate::structure::util::is_d_ts`]).
    TypeScript,
    /// Go. Detected; its extractor is `e3.1b` (`reco-dlp.1`).
    Go,
    /// Ruby. Detected; its extractor is `e3.1b`, and its scan policy needs its own budget (s1's cost finding).
    Ruby,
    /// Java. Detected; its extractor is `e3.1b`.
    Java,
    /// Not one of the six. A file with this language still gets its file-level analysis.
    Unknown,
}

impl Language {
    /// The stable string, which is also the JSON and `fathom.toml` spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Language::Rust => "rust",
            Language::Python => "python",
            Language::TypeScript => "typescript",
            Language::Go => "go",
            Language::Ruby => "ruby",
            Language::Java => "java",
            Language::Unknown => "unknown",
        }
    }

    /// The ADR-002 §6.5 build feature that would have to be enabled to read this language.
    /// [`Language::Unknown`] has none, because no feature would help.
    pub fn feature(self) -> Option<&'static str> {
        match self {
            Language::Rust => Some("lang-rust"),
            Language::Python => Some("lang-python"),
            Language::TypeScript => Some("lang-typescript"),
            Language::Go => Some("lang-go"),
            Language::Ruby => Some("lang-ruby"),
            Language::Java => Some("lang-java"),
            Language::Unknown => None,
        }
    }

    /// Detect by file name and path, the way `e1.x`'s path taxonomy does: extension first, and
    /// the two paths whose *name* changes the language's rules (`.d.ts`, `pom.xml`-adjacent names)
    /// handled where the extractor needs them.
    pub fn detect(path: &str) -> Language {
        let name = path.rsplit('/').next().unwrap_or(path);
        // `.d.ts` is TypeScript under a name that changes nothing for extraction, so it is
        // resolved here rather than made a language of its own.
        if name.ends_with(".ts") {
            return Language::TypeScript;
        }
        match name.rsplit('.').next().unwrap_or("") {
            "rs" => Language::Rust,
            "py" | "pyi" => Language::Python,
            "ts" | "mts" | "cts" | "tsx" | "jsx" => Language::TypeScript,
            "go" => Language::Go,
            "rb" | "rake" | "gemspec" => Language::Ruby,
            "java" => Language::Java,
            _ => Language::Unknown,
        }
    }

    /// Whether a language is one of the six, as opposed to [`Language::Unknown`].
    pub fn is_known(self) -> bool {
        self != Language::Unknown
    }
}

/// Where a declaration is in the file. Byte offsets are the stable identity (`e3.2` keys a node on
/// them plus the name); lines are what a human and a diff reader read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Span {
    /// Byte offset of the first character of the declaration.
    pub start_byte: usize,
    /// Byte offset just past the last character of the declaration.
    pub end_byte: usize,
    /// One-based line of the first character.
    pub start_line: usize,
    /// One-based line of the last character.
    pub end_line: usize,
}

impl Span {
    /// How many lines the declaration covers.
    pub fn lines(&self) -> usize {
        self.end_line.saturating_sub(self.start_line) + 1
    }
}

/// The complexity proxies one declaration carries.
///
/// These are *proxies*, deliberately: nothing here is a cyclomatic-computation claim, it is a
/// count of the syntactic constructs a reader has to hold. `e4.4`'s scorer and `e5.x`'s ranking
/// may weigh them; nobody may read them as a quality judgement. They are counted by node kind
/// inside the declaration's own subtree, per language, and the counting is bounded
/// (see [`crate::structure::ExtractPolicy::max_complexity_nodes`]) so a 40 000-line generated
/// array cannot make extraction super-linear.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Complexity {
    /// Lines the declaration spans, header included.
    pub body_lines: usize,
    /// `if` / `match` / `switch` / ternary arms and `catch` blocks: the branches a reader forks over.
    pub branches: usize,
    /// Loops and comprehensions: the iterations a reader traces.
    pub loops: usize,
    /// Declared parameters, for a member: excluding an implicit receiver.
    pub params: usize,
    /// `return` statements: an early exit is a branch the reader has to remember to the end.
    pub returns: usize,
    /// Member declarations inside a type: methods counted as the corpus counts them, fields not.
    pub members: usize,
    /// Nesting depth of control flow inside the declaration, so a flat 200-line function and a
    /// 200-line function eight deep are distinguishable.
    pub nesting: usize,
}

/// Visibility, normalised to the four answers the graph can use.
///
/// The distinction between [`Visibility::Public`] and the two narrower forms is load-bearing for
/// `e3.7`: a `pub(crate)` item is nameable inside the crate and not outside it, and a breaking
/// change report that called it removed would be wrong in the interesting direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Whether the visibility is the fully public one.
    Private,
    /// Rust `pub(crate)`, Python's leading-underscore convention, TypeScript's `private`/`#`.
    Crate,
    /// `pub(in path)`, `pub(super)`, package-private Java, `protected`.
    Scoped,
    /// The stable string, which is also the JSON spelling.
    Public,
}

impl Visibility {
    /// The stable string.
    pub fn as_str(self) -> &'static str {
        match self {
            Visibility::Private => "private",
            Visibility::Crate => "crate",
            Visibility::Scoped => "scoped",
            Visibility::Public => "public",
        }
    }

    /// The scope this note applies to.
    pub fn is_public(self) -> bool {
        self == Visibility::Public
    }
}

/// Why a scope cannot be enumerated. The categories are the ones spike `s1` measured
/// (`docs/concepts/structure-extraction.md`, "The blind spots"); a scope marked with any of them
/// is a scope whose declaration list is *incomplete by construction*, not one that happens to be
/// short.
pub mod synthetic {
    /// A `macro_rules!` is in scope, so items may be written by expansion.
    pub const MACRO_DEFINITION: &str = "macro_definition";
    /// A macro was *invoked* here with a shape that could have declared items.
    pub const MACRO_INVOCATION: &str = "macro_invocation";
    /// A decorator wrapped this class or member: `@dataclass` writes `__init__`.
    pub const DECORATED: &str = "decorated";
    /// A member was assigned from outside the class body (`Shape.describe = lambda ...`).
    pub const MONKEYPATCH: &str = "monkeypatch";
    /// Methods were written in a loop over a collection (`define_method(n) for %i[a b]`).
    pub const DYNAMIC_METHOD: &str = "dynamic_method";
    /// A compiler or runtime synthesises members here (a Java `record`'s accessors).
    pub const SYNTHESISED_ACCESSORS: &str = "synthesised_accessors";
    /// A barrel or `pub use *` re-exports names this file cannot enumerate.
    pub const WILDCARD_REEXPORT: &str = "wildcard_reexport";
    /// A declaration was merged into a module that lives elsewhere (`declare module 'x'`).
    pub const AMBIENT_MERGE: &str = "ambient_merge";
    /// The extractor saw a shape it knows it cannot enumerate, and said so rather than guessing.
    pub const UNKNOWN_SHAPE: &str = "unknown_shape";
}

/// One scope that cannot be enumerated, and why.
///
/// `scope` is the dotted name of the enclosing declaration — the empty string means the file's own
/// top-level scope. A scope in this list is never reported as "declares nothing": the file's export
/// and definition lists are labelled `may_have_more` wherever this list is non-empty for it or for
/// one of its ancestors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntheticScope {
    /// The base as written: `Shape`, `fmt::Display`, `collections.namedtuple`.
    pub scope: String,
    /// One of the [`synthetic`] categories, deduplicated and sorted.
    pub reasons: Vec<String>,
}

impl SyntheticScope {
    /// The declaration the relationship belongs to.
    pub fn new(scope: impl Into<String>, reason: &str) -> SyntheticScope {
        SyntheticScope {
            scope: scope.into(),
            reasons: vec![reason.to_string()],
        }
    }

    /// The stable string.
    pub fn record(&mut self, reason: &str) {
        if !self.reasons.iter().any(|r| r == reason) {
            self.reasons.push(reason.to_string());
            self.reasons.sort();
        }
    }
}

/// How a type relates to another type. `e3.2` draws these as edges of their own because
/// "who implements `Shape`?" and "what does `Circle` extend?" are different questions with
/// different answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupertypeKind {
    /// `class Circle(Shape)`, `class Base extends Shape` — inheritance of a base.
    Base,
    /// `impl Shape for Rect`, `trait Shape: Debug` — a contract, not an ancestry.
    Trait,
    /// `class Circle implements Shape`, `interface A extends B`.
    Interface,
    /// Python `class A(metaclass=M)`.
    MetaClass,
}

impl SupertypeKind {
    /// The statement's form, for the report: `use`, `mod_decl`, `from_import`, `reexport`, `reexport_star`, `import`, `namespace_import`, `side_effect_import`.
    pub fn as_str(self) -> &'static str {
        match self {
            SupertypeKind::Base => "base",
            SupertypeKind::Trait => "trait",
            SupertypeKind::Interface => "interface",
            SupertypeKind::MetaClass => "metaclass",
        }
    }
}

/// One implemented/extended relationship, with the same-file/known-package resolution `e3.2`
/// needs in order to not draw an edge to a name it cannot place.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Supertype {
    /// The declaring symbol's dotted name (`Circle`).
    pub of: String,
    /// The base as written (`Shape`, `fmt::Display`, `Shape, Protocol`).
    pub base: String,
    /// The stable string.
    pub kind: SupertypeKind,
    /// The corpus file that declares the base, when resolution placed it. `None` is the honest
    /// answer for a standard-library or third-party base, and is *not* the same as "no base".
    pub resolved: Option<String>,
    /// A resolved edge to a named symbol.
    pub span: Span,
}

/// An import as the source states it, before resolution. Kept in the output because every
/// "why did that not resolve" question in the spike ended in this row.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Import {
    /// The specifier as written (`./geo`, `crate::geo::point`, `demo.version`, `com.acme.model.*`).
    pub specifier: String,
    /// The symbol the statement names, or `*` for a whole-unit import.
    pub symbol: String,
    /// A short spelling of the statement's form, for the report (`use`, `mod_decl`, `from_import`,
    /// `reexport`, `reexport_star`, `namespace_import`, `side_effect_import`).
    pub form: String,
    /// True when the statement re-exports what it imports rather than consuming it.
    pub reexport: bool,
    /// The stable string.
    pub span: Span,
}

/// How a dependency edge got its answer, which is the difference between spike `s1` finding 2 and
/// a graph that quietly loses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// The statement named a symbol and resolution found the unit that declares it.
    Resolved,
    /// The unit is known, its contents are not: a star import, a `pub use *`, a barrel `export *`,
    /// a whole-module import, a side-effect import. "Who depends on `models.py`?" has to answer
    /// *one of these, contents unknown* rather than *nobody*.
    Wildcard,
    /// Resolution ran, and could not place the target inside this repository: an external
    /// dependency, or an internal one the corpus does not contain. Recorded so a miss is a named
    /// state rather than an absence.
    Unresolved,
}

impl EdgeKind {
    /// The unit depended on: corpus-relative path, or empty when the target is outside the corpus.
    pub fn as_str(self) -> &'static str {
        match self {
            EdgeKind::Resolved => "resolved",
            EdgeKind::Wildcard => "wildcard",
            EdgeKind::Unresolved => "unresolved",
        }
    }
}

/// One resolved dependency: this file depends on that unit, for that symbol, with that provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Corpus-relative path of the unit depended on; empty for an external or unplaceable target.
    pub target: String,
    /// The symbol depended on; `*` exactly when `kind == EdgeKind::Wildcard`.
    pub symbol: String,
    /// Whether the contents are known.
    pub kind: EdgeKind,
    /// The external specifier kept verbatim when `kind == EdgeKind::Unresolved`, otherwise empty.
    pub specifier: String,
    /// The file the answer was reached *through* when it is not the declaring file: a barrel hop
    /// (`index.ts → geo/index.ts → geo/point.ts`) leaves the middle hop here, because a re-export
    /// is a forwarding fact and `e3.7` needs to know which file to blame.
    pub via: Option<String>,
    /// The statement's form, copied from the [`Import`] that produced this edge.
    pub form: String,
    /// How many resolution passes this edge needed. Bounded by the policy, and recorded so a
    /// file whose chains need every pass is visible in the data rather than in a comment.
    pub passes: usize,
}

impl Edge {
    /// Whether the file could be read structurally at all.
    pub fn resolved(target: impl Into<String>, symbol: impl Into<String>) -> Edge {
        Edge {
            target: target.into(),
            symbol: symbol.into(),
            kind: EdgeKind::Resolved,
            specifier: String::new(),
            via: None,
            form: String::new(),
            passes: 1,
        }
    }

    /// The file the report is about.
    pub fn wildcard(target: impl Into<String>) -> Edge {
        Edge {
            target: target.into(),
            symbol: "*".to_string(),
            kind: EdgeKind::Wildcard,
            specifier: String::new(),
            via: None,
            form: String::new(),
            passes: 1,
        }
    }

    /// The declaration list. May be empty; empty is a claim only where [`Structure::may_have_shrunk`] says nothing.
    pub fn is_wildcard(&self) -> bool {
        self.kind == EdgeKind::Wildcard
    }
}

/// What a file is, independent of whether anything could be read out of it: size, lines and the
/// parse's own complaint. This half is produced for *every* file, including the ones whose language
/// this build cannot read, because "structure unavailable" must not be indistinguishable from "this
/// file is not part of the review".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileLevel {
    /// Names importable from outside this unit.
    pub path: String,
    /// Import statements as written, in source order.
    pub language: Language,
    /// Dependencies, resolved and otherwise.
    pub bytes: usize,
    /// Newlines in the text, so `lines + 1` is the line count of a file with no trailing newline
    /// and `lines` is the count of one that ends with a newline. One rule, stated once.
    pub newlines: usize,
    /// Implemented and inherited relationships.
    pub blank_lines: usize,
    /// Scopes that cannot be enumerated.
    pub code_lines: usize,
    /// Nodes the parser could not place, as a count. Zero is common; non-zero is information, not
    /// necessarily a failure — tree-sitter recovers.
    pub parse_errors: usize,
    /// Whether the byte count of the file the extractor refused to open: the limit is in the
    /// policy, and the refusal is reported, never silent.
    pub refused: bool,
}

impl FileLevel {
    /// Measure the half of the answer that needs no grammar: size and line shape.
    ///
    /// This is the function that makes "structure unavailable *plus* file-level analysis" possible
    /// in a build with no grammars compiled in at all, which is why it lives here and not in the
    /// extractor.
    #[must_use]
    pub fn measure(path: &str, text: &[u8]) -> FileLevel {
        let mut newlines = 0usize;
        let mut blank = 0usize;
        let mut code = 0usize;
        for line in text.split(|byte| *byte == b'\n') {
            if line.is_empty() {
                continue;
            }
            if line.iter().all(u8::is_ascii_whitespace) {
                blank += 1;
            } else {
                code += 1;
            }
        }
        for byte in text {
            if *byte == b'\n' {
                newlines += 1;
            }
        }
        FileLevel {
            path: path.to_string(),
            language: Language::detect(path),
            bytes: text.len(),
            newlines,
            blank_lines: blank,
            code_lines: code,
            parse_errors: 0,
            refused: false,
        }
    }
}

/// Why a file has no structural model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum UnavailableReason {
    /// Not one of the six grammars. There is nothing to rebuild.
    LanguageUnknown,
    /// The grammar is one of the six and this binary was compiled without it (ADR-002 §6.5).
    GrammarNotCompiled {
        /// The ADR-002 §6.5 feature name that would have to be enabled.
        feature: String,
    },
    /// The grammar is compiled in and this language's extractor is a later ticket (`e3.1b`).
    ExtractorNotShipped,
    /// Above `ExtractPolicy::max_file_bytes`.
    TooLarge {
        /// The byte limit that refused the file.
        limit: usize,
    },
    /// Not valid UTF-8 in the part the extractor would read.
    NotUtf8,
    /// The parse produced more error than code: a truncated or binary file.
    ParseFailed {
        /// How many error nodes the parse reported.
        errors: usize,
    },
    /// The grammar could not be handed to the parser at all.
    Parser,
}

impl UnavailableReason {
    /// The stable string, which is also the JSON spelling.
    pub fn as_str(&self) -> &'static str {
        match self {
            UnavailableReason::LanguageUnknown => "language_unknown",
            UnavailableReason::GrammarNotCompiled { .. } => "grammar_not_compiled",
            UnavailableReason::ExtractorNotShipped => "extractor_not_shipped",
            UnavailableReason::TooLarge { .. } => "too_large",
            UnavailableReason::NotUtf8 => "not_utf8",
            UnavailableReason::ParseFailed { .. } => "parse_failed",
            UnavailableReason::Parser => "parser",
        }
    }

    /// The sentence a surface prints. Written here rather than in the UI layer so the phrase
    /// "structure unavailable" and the reason arrive together everywhere they are said.
    pub fn sentence(&self) -> String {
        match self {
            UnavailableReason::LanguageUnknown => {
                "structure unavailable: not one of the grammars fathom reads".to_string()
            }
            UnavailableReason::GrammarNotCompiled { feature } => format!(
                "structure unavailable: this build was compiled without the `{feature}` grammar"
            ),
            UnavailableReason::ExtractorNotShipped => {
                "structure unavailable: this language's extractor has not shipped yet".to_string()
            }
            UnavailableReason::TooLarge { limit } => format!(
                "structure unavailable: the file is larger than the {limit} byte extraction limit"
            ),
            UnavailableReason::NotUtf8 => "structure unavailable: the file is not text".to_string(),
            UnavailableReason::ParseFailed { errors } => format!(
                "structure unavailable: the parse reported {errors} errors and returned no structure"
            ),
            UnavailableReason::Parser => {
                "structure unavailable: the parser could not be initialised".to_string()
            }
        }
    }
}

/// The record for one file: the file-level facts, and either the structure or the reason there is
/// none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileStructure {
    /// The file the report is about.
    pub path: String,
    /// The language detection said.
    pub language: Language,
    /// Bytes of the file.
    pub file_level: FileLevel,
    /// `None` alongside a reason. Never empty-and-silent: an empty `symbols` in a `Some` record is
    /// a claim that the file declares nothing *and can be asked*.
    pub structure: Option<Structure>,
    /// Why there is no structure. `Some` exactly when `structure` is `None` — a lost file and an
    /// unreadable file are different facts, and only one of them is a bug.
    pub reason: Option<UnavailableReason>,
}

/// The structural half of a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Structure {
    /// Newlines in the text.
    pub symbols: Vec<Symbol>,
    /// The top-level symbols nameable from outside this unit, per the language's own rules.
    pub exports: Vec<String>,
    /// Imports as written, in source order.
    pub imports: Vec<Import>,
    /// Resolved (and wildcard, and unresolved) dependencies, sorted.
    pub edges: Vec<Edge>,
    /// Lines with no visible characters.
    pub supertypes: Vec<Supertype>,
    /// The scopes whose declaration list cannot be enumerated (s1 finding 1).
    pub synthetic_scopes: Vec<SyntheticScope>,
    /// Every note the extractor recorded, sorted and counted: `[{kind, detail}]`.
    pub notes: Vec<Note>,
    /// What the bounded resolution pass cost this file.
    pub resolution: Resolution,
}

impl Structure {
    /// The declaration with exactly this dotted name, if this file has one.
    pub fn find(&self, name: &str) -> Option<&Symbol> {
        self.symbols.iter().find(|s| s.name == name)
    }

    /// The declarations whose container is `container`.
    pub fn members_of(&self, container: &str) -> Vec<&Symbol> {
        self.symbols
            .iter()
            .filter(|s| s.container == container)
            .collect()
    }

    /// The scopes that cannot be enumerated, as a lookup.
    pub fn synthetic(&self) -> BTreeMap<&str, &[String]> {
        self.synthetic_scopes
            .iter()
            .map(|s| (s.scope.as_str(), s.reasons.as_slice()))
            .collect()
    }

    /// Whether `scope` is one that cannot be enumerated.
    pub fn is_synthetic(&self, scope: &str) -> bool {
        self.synthetic_scopes.iter().any(|s| s.scope == scope)
    }

    /// The `e3.7` sentence: whether this file's lists might be shorter than the truth.
    ///
    /// This is the mechanism spike `s1` named as the highest-value thing `e3.1` had to build. A
    /// breaking-change report that says *removed* about a macro-generated item is confidently
    /// wrong about the most interesting code in the repository; this returns the reason it cannot
    /// say that, when it cannot.
    pub fn may_have_shrunk(&self) -> Option<String> {
        if self.synthetic_scopes.is_empty() {
            return None;
        }
        let mut reasons: Vec<&str> = self
            .synthetic_scopes
            .iter()
            .flat_map(|s| s.reasons.iter().map(|r| r.as_str()))
            .collect();
        reasons.sort_unstable();
        reasons.dedup();
        let scopes: Vec<&str> = self
            .synthetic_scopes
            .iter()
            .map(|s| {
                if s.scope.is_empty() {
                    "file"
                } else {
                    s.scope.as_str()
                }
            })
            .collect();
        Some(format!(
            "may have shrunk, not shrunk: {} cannot be enumerated ({})",
            scopes.join(", "),
            reasons.join(", ")
        ))
    }

    /// Every wildcard edge — the answer to "who depends on this?" half of the dependency question
    /// when the contents are unknown.
    pub fn wildcard_edges(&self) -> Vec<&Edge> {
        self.edges.iter().filter(|e| e.is_wildcard()).collect()
    }
}

/// One named declaration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Symbol {
    /// Which of the ten kinds this is.
    pub kind: SymbolKind,
    /// Dotted from the file's own top level: `Rect.new`, `Geo.Vec`, `Shape.area`.
    pub name: String,
    /// The container's dotted name, empty at top level. Part of nothing's identity (the dotted
    /// name already carries it) but the field `e3.2` groups by.
    pub container: String,
    /// Where the declaration is in the file.
    pub span: Span,
    /// Visibility as the language spells it, before the export rule is applied.
    pub visibility: Visibility,
    /// Whether the export rule put this name in [`Structure::exports`].
    pub exported: bool,
    /// The counted proxies of how much a reader has to hold.
    pub complexity: Complexity,
    /// Language spellings worth keeping (`async`, `unsafe`, `abstract`, `static`, `property`,
    /// `classmethod`, `getter`, `overload`), sorted and deduplicated.
    pub modifiers: Vec<String>,
}

impl Symbol {
    /// Whether this declaration sits at the file's top level.
    pub fn top_level(&self) -> bool {
        self.container.is_empty()
    }
}

/// One extractor note: a category and the thing it happened to. The categories are not scored —
/// they are the audit trail that separates "the source does not contain this fact" from "the
/// extractor is incomplete", which spike `s1` called the most useful output it produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    /// The category, from the extractor's own vocabulary.
    pub kind: String,
    /// The thing it happened to, verbatim: a name, a specifier, a spelling.
    pub detail: String,
}

/// What the bounded, iterative resolution pass cost this file (s1 finding 6).
///
/// Resolution is the only super-linear step in extraction, so it is bounded per file by
/// [`crate::structure::ExtractPolicy::max_resolution_passes`], and the bound is reported rather
/// than assumed: `converged: false` means the answer is a lower bound on the edges this file has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolution {
    /// Passes this file's edge set actually needed.
    pub passes_used: usize,
    /// Whether the fixpoint stopped because nothing changed, as opposed to hitting the bound.
    pub converged: bool,
    /// The bound that was in force when this ran — the config value, echoed into the data so a
    /// stored row can be read years later without guessing which build made it.
    pub bound: usize,
    /// Resolved edges whose re-export chain hit the hop bound before it found a declaration.
    pub truncated_chains: usize,
}

impl Resolution {
    /// The ledger of a file that had nothing to resolve.
    pub fn none() -> Resolution {
        Resolution {
            passes_used: 0,
            converged: true,
            bound: 0,
            truncated_chains: 0,
        }
    }
}

/// The answer to "who depends on `X`?", with the unknown kept as a separate answer.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependents {
    /// Files that import the named symbol (or, for a whole-unit query, the file) by name.
    pub named: Vec<String>,
    /// Files whose dependency on this unit *or one of its barrels* is real but of unknown
    /// contents. `e3.2`'s caller-list and `e3.7`'s breaking-change report both have to read this.
    pub unknown: Vec<String>,
}

impl Dependents {
    /// The sentence a surface prints. Ordered so the confident half comes first and the unknown
    /// half is still said.
    pub fn sentence(&self) -> String {
        match (self.named.is_empty(), self.unknown.is_empty()) {
            (true, true) => "no dependents found in this corpus".to_string(),
            (false, true) => format!(
                "{} named dependent(s): {}",
                self.named.len(),
                self.named.join(", ")
            ),
            (true, false) => format!(
                "{} file(s) depend on it with contents unknown: {}",
                self.unknown.len(),
                self.unknown.join(", ")
            ),
            (false, false) => format!(
                "{} named dependent(s) ({}); and {} more depend on it with contents unknown: {}",
                self.named.len(),
                self.named.join(", "),
                self.unknown.len(),
                self.unknown.join(", ")
            ),
        }
    }
}

/// One input file. The caller owns the bytes: `e1.x`'s ingestion already read them, and an
/// extractor that opened files itself would not be the pure function ADR-004 §5's cache keys assume.
#[derive(Debug, Clone, Copy)]
pub struct SourceFile<'a> {
    /// Corpus-relative path. The extractor never opens it; the caller read it.
    pub path: &'a str,
    /// The file's bytes.
    pub text: &'a [u8],
}

impl<'a> SourceFile<'a> {
    /// One input, from a path and the bytes already read.
    #[must_use]
    pub fn new(path: &'a str, text: &'a [u8]) -> SourceFile<'a> {
        SourceFile { path, text }
    }
}

/// The result of one extraction run over one corpus.
#[derive(Debug, Clone, Serialize)]
pub struct CorpusStructure {
    /// The policy that was in force, echoed for provenance.
    pub policy: ExtractPolicy,
    /// Every input file, in path order. `files.len() == sources.len()`, always.
    pub files: Vec<FileStructure>,
    /// Run-level totals, so a report can state a total without re-walking the records.
    pub summary: Summary,
}

impl CorpusStructure {
    /// Who depends on `file`, or on `file`'s `symbol`: the named half and the unknown half, kept
    /// apart (s1 finding 2).
    #[must_use]
    pub fn dependents_of(&self, file: &str, symbol: Option<&str>) -> Dependents {
        crate::structure::query::dependents_of(self, file, symbol)
    }

    /// The record for one path, if it was in the input.
    #[must_use]
    pub fn record(&self, path: &str) -> Option<&FileStructure> {
        self.files.iter().find(|record| record.path == path)
    }

    /// Every input path, whether or not anything could be read out of it.
    #[must_use]
    pub fn paths(&self) -> Vec<&str> {
        self.files
            .iter()
            .map(|record| record.path.as_str())
            .collect()
    }
}

/// Run-level counts, so a report can state a total without re-walking the records.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Summary {
    /// Input files, which is also output records: nothing is dropped.
    pub files: usize,
    /// Records that produced a structural model.
    pub extracted: usize,
    /// Records that produced a stated reason instead.
    pub unavailable: usize,
    /// Declarations found, across every record.
    pub symbols: usize,
    /// Exported names found, across every record.
    pub exports: usize,
    /// Implemented or inherited relationships found.
    pub supertypes: usize,
    /// Scopes marked cannot-enumerate.
    pub synthetic_scopes: usize,
    /// Edges that named a symbol and found its declaring unit.
    pub edges_resolved: usize,
    /// Edges whose unit is known and contents are not (s1 finding 2).
    pub edges_wildcard: usize,
    /// Edges that could not be placed inside the corpus.
    pub edges_unresolved: usize,
    /// Highest number of resolution passes any one file needed.
    pub passes_used_max: usize,
    /// Files whose edge sets were still growing when the bound stopped the fixpoint.
    pub non_converged: usize,
    /// Files whose re-export chains hit the hop bound.
    pub truncated_chains: usize,
    /// The same totals, split by language, because a floor is per language.
    pub by_language: BTreeMap<String, LanguageSummary>,
}

/// Per-language totals inside [`Summary`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct LanguageSummary {
    /// Files of this language in the input.
    pub files: usize,
    /// Files of this language that produced a model.
    pub extracted: usize,
    /// Files of this language that produced a reason instead.
    pub unavailable: usize,
    /// Declarations found in this language.
    pub symbols: usize,
    /// Exported names found in this language.
    pub exports: usize,
    /// Supertype relationships found in this language.
    pub supertypes: usize,
    /// Cannot-enumerate scopes in this language.
    pub synthetic_scopes: usize,
    /// Resolved edges in this language.
    pub edges_resolved: usize,
    /// Wildcard edges in this language: contents unknown, dependency real.
    pub edges_wildcard: usize,
    /// Edges this corpus could not place.
    pub edges_unresolved: usize,
}
