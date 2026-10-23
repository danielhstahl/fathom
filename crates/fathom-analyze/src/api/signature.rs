//! The signature: what a caller compiles against, read out of the declaration's own text.
//!
//! [`crate::structure`] gives this module a declaration's identity, span, kind and visibility, and it
//! deliberately stops there — a structural model that carried parameter types would be a second,
//! worse parser. But *does this change break a caller?* is a question about the **header**: the
//! parameter list, the optionality of each parameter, the return type, the fields of a struct, the
//! members of an interface. So this module reads the header text of the span the extractor already
//! found, through `textscan::Scan`, and normalises it into [`Signature`] and [`Field`].
//!
//! Three rules keep it honest:
//!
//! 1. **The header, never the body.** A body is where an implementation may change a thousand ways
//!    without touching a contract, and a surface that reported a `+3` inside a function as an API
//!    change would produce the noise that buries the one real change. The exception is the type-like
//!    declarations whose body *is* the contract — a Rust struct's fields, a TypeScript interface's
//!    members, a Go struct's fields, an enum's variants — which [`fields_of`] reads as [`Field`]s,
//!    skipping the lines that belong to a member declaration the extractor already named.
//! 2. **Normalise what a digest needs, keep what a human reads.** [`Signature::head`] is
//!    whitespace-collapsed and comment-free, so reformatting a declaration is not an API change, and
//!    [`Signature::record`] is the field-by-field form the diff compares, so `a: u8` becoming
//!    `b: u8` is a *rename* with a name in it rather than an opaque string difference.
//! 3. **Say when it could not be read.** A header this parser cannot decompose sets
//!    [`Signature::unparsed`], and a change touching an unparsed signature is classified
//!    [`Reason::SignatureUnparsed`](crate::api::classify::Reason::SignatureUnparsed) — indeterminate,
//!    never silently additive, because "the signature differs and I cannot say how" is not the claim
//!    "the signature is compatible".
//!
//! The per-language rules (where a header ends, where the parameters are, what makes a parameter
//! optional, where a deprecation note lives) are written in
//! [`docs/concepts/api-surface.md`](../../../../docs/concepts/api-surface.md) and mirrored here one
//! function per language, so a disagreement between the two is a bug in one of them.

use crate::api::textscan::Scan;
use crate::structure::{Language, Span, Symbol, SymbolKind};
use serde::Serialize;

/// How many member lines one declaration contributes to its record. Above it the type is generated,
/// and a generated type's field list is not a contract a human reviews.
pub const MAX_FIELDS_PER_ITEM: usize = 256;

/// One parameter, as written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Param {
    /// The parameter's name as written, or the whole entry when it has none (a Rust pattern, a Go
    /// bare type). Never empty.
    pub name: String,
    /// The type as written, where the language puts one. `None` is Python's unannotated case and is
    /// *not* the same claim as `Some("")`.
    pub ty: Option<String>,
    /// The default value as written, where the declaration gives one.
    pub default: Option<String>,
    /// Whether a caller may omit it: a default, a `?`, a rest parameter, a Rust `Option<T>`, a
    /// Python keyword-only argument.
    pub optional: bool,
    /// A rest/varargs parameter (`*args`, `**kwargs`, `...rest`, `xs ...int`).
    pub rest: bool,
    /// Python only: written after a bare `*`, so it cannot be passed positionally.
    pub keyword_only: bool,
    /// Python only: written before a bare `/`, so it cannot be passed by name.
    pub positional_only: bool,
}

impl Param {
    /// The one-line form a report prints: `name: Ty`, `?` for optional, `…` for a rest.
    #[must_use]
    pub fn display(&self) -> String {
        let mut out = String::new();
        if self.rest {
            out.push('…');
        }
        out.push_str(&self.name);
        if self.optional && self.default.is_none() {
            out.push('?');
        }
        if let Some(ty) = &self.ty {
            out.push_str(": ");
            out.push_str(ty);
        }
        if let Some(default) = &self.default {
            out.push_str(" = ");
            out.push_str(default);
        }
        out
    }

    /// The digest form. Every field is in it, because a parameter that stopped being keyword-only is a
    /// different contract and must not digest the same.
    #[must_use]
    pub fn record(&self) -> String {
        let mut out = self.name.clone();
        if let Some(ty) = &self.ty {
            out.push(':');
            out.push_str(ty);
        }
        if let Some(default) = &self.default {
            out.push('=');
            out.push_str(default);
        }
        if self.optional {
            out.push_str("??o");
        }
        if self.rest {
            out.push_str("??r");
        }
        if self.keyword_only {
            out.push_str("??kw");
        }
        if self.positional_only {
            out.push_str("??pos");
        }
        out
    }
}

/// One member line of a type-like declaration: a field, an interface member, an enum variant, a
/// Python class attribute.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Field {
    /// The member name when it could be named, else the whole line — so it still diffs, as text.
    pub name: String,
    /// The type as written, where the language puts one.
    pub ty: Option<String>,
    /// Whether the member may be absent: TypeScript's `?`, a default value, a Python attribute
    /// assigned in the class body.
    pub optional: bool,
    /// The normalised line as written: what a report shows, and what an unnameable member digests on.
    pub text: String,
}

impl Field {
    /// The digest form.
    #[must_use]
    pub fn record(&self) -> String {
        let mut out = self.name.clone();
        if let Some(ty) = &self.ty {
            out.push(':');
            out.push_str(ty);
        }
        if self.optional {
            out.push_str("??o");
        }
        out
    }
}

/// The contract one declaration offers, decomposed.
///
/// Every field here is something a caller compiles against. `head` is the whole normalised header and
/// is what a report *shows*; the rest is what the diff *compares*, because a change a report can only
/// describe as "the text differs" is a change a reviewer cannot act on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Default)]
pub struct Signature {
    /// The normalised header: comments dropped, whitespace collapsed, body excluded.
    pub head: String,
    /// Generic parameters, in order, as written.
    pub generics: Vec<String>,
    /// The receiver: Rust's `&self`/`&mut self`, Python's `self`/`cls`, Go's `(r *T)`. A *change*
    /// here breaks every caller that held a shared reference.
    pub receiver: Option<String>,
    /// The parameters, in order.
    pub params: Vec<Param>,
    /// The return type as written, for a callable.
    pub returns: Option<String>,
    /// The declared type of a value-like item (`const`, `static`, `var`).
    pub ty: Option<String>,
    /// The right-hand side of a type alias: a *meaning*, so a change here is a change of contract.
    pub alias: Option<String>,
    /// The initialiser of a `const`/`static`/`var` or a module-level assignment: a *value*, so a
    /// change here is a behaviour change and not a contract change — which is why it is a separate
    /// field from [`Signature::alias`] and why it still digests.
    pub value: Option<String>,
    /// The header could not be decomposed, so everything above is a best effort: a change that touches
    /// it is indeterminate rather than guessed at.
    pub unparsed: bool,
}

impl Signature {
    /// The byte-comparable form of the contract. `head` is in it, so a reformatted or re-commented
    /// declaration digests the same; every structural piece is in it too, so anything a reader would
    /// call a change has a different digest.
    #[must_use]
    pub fn record(&self) -> String {
        let params: Vec<String> = self.params.iter().map(Param::record).collect();
        let mut out = String::from("head=");
        out.push_str(&self.head);
        out.push_str(";gen=");
        out.push_str(&self.generics.join(","));
        out.push_str(";recv=");
        out.push_str(self.receiver.as_deref().unwrap_or(""));
        out.push_str(";params=");
        out.push_str(&params.join(","));
        out.push_str(";ret=");
        out.push_str(self.returns.as_deref().unwrap_or(""));
        out.push_str(";ty=");
        out.push_str(self.ty.as_deref().unwrap_or(""));
        out.push_str(";alias=");
        out.push_str(self.alias.as_deref().unwrap_or(""));
        out.push_str(";value=");
        out.push_str(self.value.as_deref().unwrap_or(""));
        out.push_str(if self.unparsed { ";unparsed" } else { "" });
        out
    }
}

/// The header of one declaration, found in the file's own text.
#[derive(Debug, Clone)]
pub struct Header {
    /// The normalised header text (comments dropped, whitespace collapsed).
    pub head: String,
    /// The byte the body starts at — the `{` of a record or a body; for a declaration with no body,
    /// the byte just past the header.
    pub body_start: usize,
    /// The header could not be located (an unbalanced span, a truncated file).
    pub unparsed: bool,
}

/// Find the header of one declaration inside `scan`.
///
/// The rule is per language and per kind, and short enough to say here:
///
/// * **Python** — a `def`/`class` header ends at the `:` that closes it, which is the first top-level
///   `:` on the declaration's own first line (an annotation would otherwise stop a wrapped header
///   early, so a header that wraps is found by its closing paren instead); anything else at module
///   level ends at its line.
/// * **Rust** — the first top-level `{` (a body, a record, a variant list) or `;` (a trait method, a
///   `type`, a `const`, a unit struct, a `mod x;`).
/// * **Go** — the first top-level `{`, else the line end: Go writes no `;` in source, and a
///   declaration without a body ends with the line.
/// * **TypeScript, Java** — the first top-level `{` or `;`, else the line end, which is the `.d.ts`
///   and interface-member case that would otherwise swallow the next declaration.
/// * **Ruby** — the line end: the surface is the file, so no per-declaration header is read.
#[must_use]
pub fn header_of(scan: &Scan, language: Language, kind: SymbolKind, span: &Span) -> Header {
    let start = span.start_byte.min(scan.raw.len());
    let stop = span.end_byte.max(span.start_byte).min(scan.raw.len());
    let line_end = scan.line_end(start).min(stop);
    let (end, unparsed) = match language {
        Language::Ruby => (line_end, false),
        Language::Unknown => (line_end, true),
        Language::Python => {
            if matches!(
                kind,
                SymbolKind::Fn | SymbolKind::Method | SymbolKind::Class
            ) {
                match scan.find(start..line_end, ":") {
                    Some((at, _)) => (at, false),
                    None => {
                        // The parameter list wrapped onto later lines; the colon that closes the header
                        // is on the closing paren's line.
                        let closing = scan
                            .find(start..stop, "(")
                            .and_then(|(open, _)| scan.close(open));
                        match closing {
                            Some(close) => {
                                let limit = scan.line_end(close.min(stop)).min(stop);
                                match scan.find(close..limit, ":") {
                                    Some((at, _)) => (at, false),
                                    None => (line_end, true),
                                }
                            }
                            None => (line_end, true),
                        }
                    }
                }
            } else {
                (line_end, false)
            }
        }
        Language::Go => match scan.find(start..stop, "{") {
            Some((at, _)) => (at, false),
            None => (line_end, false),
        },
        Language::Rust | Language::TypeScript | Language::Java => {
            match scan.find(start..stop, "{;") {
                Some((at, _)) => (at, false),
                None => (line_end, false),
            }
        }
    };
    Header {
        head: normalise(scan.text(start..end.max(start))),
        body_start: end,
        unparsed,
    }
}

/// Collapse whitespace, drop the separators a header ends on, and drop masked comment remnants.
#[must_use]
pub fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(line);
    }
    while out.ends_with(',') || out.ends_with(';') || out.ends_with('~') || out.ends_with(' ') {
        out.pop();
    }
    out
}

// ==============================================================================================
// the signature
// ==============================================================================================

/// Decompose one header into a [`Signature`].
#[must_use]
pub fn signature_of(language: Language, kind: SymbolKind, head: &str, unparsed: bool) -> Signature {
    let mut signature = Signature {
        head: head.to_string(),
        unparsed,
        ..Signature::default()
    };
    if head.is_empty() {
        signature.unparsed = true;
        return signature;
    }
    // The header is scanned on its own so its `<`/`>` positions can be read; headers are short.
    let scan = Scan::new(head, language);
    let len = scan.raw.len();
    let Some(name) = name_position(language, head) else {
        signature.unparsed = true;
        return signature;
    };
    // A generic parameter list is the `<` that *immediately follows the item's name* — `fn f<T>(…)`,
    // `struct S<T>`. A `<` further along belongs to something the declaration mentions (`-> Option<T>`,
    // `: Map<K, V>`), and reading it as the item's own generics would invent a change of generic count
    // where the item never had one.
    let name_end = word_end(head, name);
    if head[name_end..].starts_with('<')
        && let Some(close) = angle_close(&scan, name_end)
    {
        signature.generics = split_top(&scan, name_end + 1..close - 1)
            .into_iter()
            .map(|part| normalise(scan.text(part)))
            .filter(|part| !part.is_empty())
            .collect();
    }
    // A Go method writes its receiver before the name: `func (r *Rect) Area()`. Reading it as a
    // parameter would hide the fact that `*Rect` becoming `Rect` breaks every caller holding a pointer.
    if language == Language::Go
        && kind == SymbolKind::Method
        && let Some((open, _)) = scan.find(0..name, "(")
        && let Some(close) = scan.close(open)
    {
        let text = normalise(scan.text(open + 1..close - 1));
        if !text.is_empty() {
            signature.receiver = Some(text);
        }
    }
    let params = scan
        .find(name..len, "(")
        .and_then(|(open, _)| scan.close(open).map(|close| (open, close)));
    match params {
        Some((open, close)) if matches!(kind, SymbolKind::Fn | SymbolKind::Method) => {
            signature.params = parse_params(language, &scan, open + 1..close - 1);
            // Rust and Python write the receiver as the first parameter; a change from `&self` to
            // `&mut self` is a change of the receiver, and calling it a parameter change would let the
            // "optional, therefore additive" rule answer a question that was never asked.
            if signature.receiver.is_none()
                && let Some(first) = signature.params.first()
                && is_receiver(language, &first.name)
            {
                signature.receiver = Some(signature.params.remove(0).name);
            }
            signature.returns =
                return_type(language, &scan, close).filter(|value| !value.is_empty());
        }
        // A parenthesised list on a non-callable is a Go method receiver or a tuple struct: the
        // entries are the contract either way, and a receiver is named first.
        Some((open, close)) => {
            for part in split_top(&scan, open + 1..close - 1) {
                let text = normalise(scan.text(part));
                if text.is_empty() {
                    continue;
                }
                if is_receiver(language, &text) {
                    signature.receiver = Some(text);
                } else {
                    signature.params.push(param_of(language, &text));
                }
            }
        }
        None => {}
    }
    if matches!(kind, SymbolKind::Const | SymbolKind::Var | SymbolKind::Type) {
        let (ty, right) = type_and_value(language, kind, head);
        signature.ty = ty;
        match kind {
            SymbolKind::Type => signature.alias = right,
            _ => signature.value = right,
        }
    }
    signature
}

/// The return type: whatever the header says after the parameter list, with the arrow or colon the
/// language writes taken off, and a Rust `where` clause left out (it is a bound on the caller, not a
/// type the caller names).
fn return_type(language: Language, scan: &Scan, close: usize) -> Option<String> {
    let rest = normalise(scan.text(close..scan.raw.len()));
    if rest.is_empty() {
        return None;
    }
    let marker = match language {
        Language::Rust | Language::Python => "->",
        Language::TypeScript | Language::Java => ":",
        Language::Go | Language::Ruby | Language::Unknown => "",
    };
    let rest = if marker.is_empty() {
        rest
    } else {
        match rest.split_once(marker) {
            Some((_, right)) => right.trim().to_string(),
            None => return None,
        }
    };
    let rest = match language {
        Language::Rust => rest.split(" where ").next().unwrap_or(&rest).to_string(),
        _ => rest,
    };
    Some(rest)
}

/// The declared type and the value or alias of a `const`/`var`/`type` header.
fn type_and_value(
    language: Language,
    kind: SymbolKind,
    head: &str,
) -> (Option<String>, Option<String>) {
    if kind == SymbolKind::Type {
        let alias = match language {
            // Go writes `type Alias Underlying` (and `type Alias = Underlying` for a true alias), so
            // the name is the first word and everything after it is what the alias means.
            Language::Go => {
                let rest = head.strip_prefix("type").map(str::trim).unwrap_or(head);
                split_once_whitespace(rest).map(|(_, right)| normalise(&right))
            }
            _ => split_once_top(head, '=').map(|(_, right)| normalise(&right)),
        };
        return (None, alias.filter(|value| !value.is_empty()));
    }
    let (left, default) = match split_once_top(head, '=') {
        Some((left, right)) => (left, Some(normalise(&right))),
        None => (head.to_string(), None),
    };
    // Go writes `const Sides = 4` with no type at all, and `var Version string = "1.0.0"` with one, so
    // the keyword comes off first and what is left is a name plus an optional type.
    let left = normalise(&left);
    let left = match language {
        Language::Go => ["const", "var"]
            .iter()
            .find_map(|keyword| left.strip_prefix(keyword).map(str::trim))
            .filter(|rest| !rest.is_empty() && !rest.starts_with('('))
            .unwrap_or(left.as_str())
            .to_string(),
        _ => left,
    };
    let ty = match language {
        Language::Go => split_once_whitespace(&left).map(|(_, right)| normalise(&right)),
        Language::Java => split_once_whitespace(&left).and_then(|(_, right)| {
            // `public static final Map<String, Integer> LIMITS` — the name is the last word.
            let words: Vec<&str> = right.split_whitespace().collect();
            if words.len() >= 2 {
                Some(words[..words.len() - 1].join(" "))
            } else {
                None
            }
        }),
        _ => split_once_top(&left, ':').map(|(_, right)| normalise(&right)),
    };
    (
        ty.filter(|value| !value.is_empty()),
        default.filter(|value| !value.is_empty()),
    )
}

// ==============================================================================================
// parameters
// ==============================================================================================

/// The receiver, in the spellings that put it in the parameter list.
fn is_receiver(language: Language, text: &str) -> bool {
    let trimmed = text.trim();
    match language {
        Language::Rust => {
            trimmed == "self"
                || trimmed == "&self"
                || trimmed == "&mut self"
                || trimmed == "mut self"
                || trimmed.starts_with("self:")
                || (trimmed.starts_with('&') && trimmed.ends_with("self"))
        }
        Language::Python => trimmed == "self" || trimmed == "cls",
        _ => false,
    }
}

/// Split a parameter list on top-level commas and hand each entry to the language's parser.
fn parse_params(language: Language, scan: &Scan, range: std::ops::Range<usize>) -> Vec<Param> {
    let parts: Vec<String> = split_top(scan, range)
        .into_iter()
        .map(|part| normalise(scan.text(part)))
        .filter(|part| !part.is_empty())
        .collect();
    match language {
        Language::Python => python_params(parts),
        Language::Go => go_params(parts),
        _ => parts.iter().map(|part| param_of(language, part)).collect(),
    }
}

/// Python's parameter grammar, in marker order: `/`, names, `*`, names, `*args`, `**kwargs`.
fn python_params(parts: Vec<String>) -> Vec<Param> {
    let mut out: Vec<Param> = Vec::new();
    let mut keyword_only = false;
    let mut positional_only = false;
    for part in parts {
        match part.as_str() {
            "/" => {
                positional_only = true;
                continue;
            }
            "*" => {
                keyword_only = true;
                continue;
            }
            _ => {}
        }
        let mut param = param_of(Language::Python, &part);
        if param.rest {
            param.keyword_only = part.starts_with("**");
        } else {
            param.keyword_only = keyword_only;
            param.positional_only = positional_only;
        }
        out.push(param);
    }
    out
}

/// Go's grouped parameters: `func f(w, h float64, label string)`.
fn go_params(parts: Vec<String>) -> Vec<Param> {
    let mut out: Vec<Param> = Vec::new();
    let mut pending: Vec<Param> = Vec::new();
    // `func f(w, h float64)` and `func f(Shape)` both look like a list of bare words until the list is
    // read whole: if any entry carries a type, the bare ones are names sharing the next entry's type;
    // if none does, every bare word is an unnamed parameter whose type is itself.
    let any_typed = parts
        .iter()
        .any(|part| split_once_whitespace(part).is_some_and(|(_, rest)| !rest.trim().is_empty()));
    for part in parts {
        if any_typed && split_once_whitespace(&part).is_none() {
            pending.push(Param {
                name: part.trim().to_string(),
                ..Param::default()
            });
            continue;
        }
        let param = param_of(Language::Go, &part);
        if param.ty.is_none() && !param.rest {
            pending.push(param);
            continue;
        }
        let shared = param.ty.clone();
        for mut earlier in pending.drain(..) {
            earlier.ty = shared.clone();
            out.push(earlier);
        }
        out.push(param);
    }
    out.append(&mut pending);
    out
}

/// One parameter, one language: an optional rest marker, a name, a type, a default — with the
/// separators that language uses.
fn param_of(language: Language, text: &str) -> Param {
    let mut text = text.trim().to_string();
    let mut param = Param::default();
    let mut ty: Option<String> = None;
    let mut default: Option<String> = None;
    match language {
        Language::Python => {
            if let Some(rest) = text.strip_prefix("**") {
                param.rest = true;
                text = rest.to_string();
            } else if let Some(rest) = text.strip_prefix('*') {
                param.rest = true;
                text = rest.to_string();
            }
            if let Some((left, right)) = split_once_top(&text, '=') {
                text = left.trim().to_string();
                default = Some(right.trim().to_string());
            }
            if let Some((left, right)) = split_once_top(&text, ':') {
                text = left.trim().to_string();
                ty = Some(right.trim().to_string());
            }
        }
        Language::Rust => {
            if let Some((left, right)) = split_once_top(&text, ':') {
                text = left.trim().to_string();
                ty = Some(right.trim().to_string());
            }
            text = text.trim_start_matches("mut ").trim().to_string();
        }
        Language::TypeScript => {
            if let Some(rest) = text.strip_prefix("...") {
                param.rest = true;
                text = rest.to_string();
            }
            for prefix in ["public ", "private ", "protected ", "readonly "] {
                if let Some(rest) = text.strip_prefix(prefix) {
                    text = rest.to_string();
                }
            }
            if let Some((left, right)) = split_once_top(&text, '=') {
                text = left.trim().to_string();
                default = Some(right.trim().to_string());
            }
            // A destructured parameter (`{ a, b }: Options`) keeps its pattern as its name: the pattern
            // is what a caller has to satisfy.
            if let Some((left, right)) = split_once_top(&text, ':') {
                text = left.trim().to_string();
                ty = Some(right.trim().to_string());
            }
            // TypeScript's optionality is a mark on the *name* (`prefix?: string`), and it has to come
            // off the name: left on, `prefix?` becoming `prefix` reads as a renamed parameter instead of
            // one that became required, which is the difference between a major and nothing at all.
            if text.ends_with('?') {
                text.pop();
                param.optional = true;
            }
        }
        Language::Go => {
            if let Some(rest) = text.strip_prefix("...") {
                param.rest = true;
                text = rest.trim().to_string();
            }
            match split_once_whitespace(&text) {
                Some((name, rest)) if !rest.trim().is_empty() => {
                    text = name.trim().to_string();
                    let rest = rest.trim();
                    // `eps ...float64`: a variadic parameter is a list the caller may leave empty, so it
                    // is optional in the sense that decides additive from breaking.
                    if let Some(element) = rest.strip_prefix("...") {
                        param.rest = true;
                        ty = Some(element.trim().to_string());
                    } else {
                        ty = Some(rest.to_string());
                    }
                }
                _ => {
                    // A bare type: Go writes unnamed parameters, and the type is the contract.
                    ty = Some(text.clone());
                }
            }
        }
        Language::Java => {
            for prefix in ["final ", "public ", "private ", "protected "] {
                if let Some(rest) = text.strip_prefix(prefix) {
                    text = rest.to_string();
                }
            }
            if let Some((left, right)) = split_once_top(&text, '=') {
                text = left.trim().to_string();
                default = Some(right.trim().to_string());
            }
            // Java puts the type before the name: `final Map<String, Integer> byId`.
            if let Some(at) = last_top_level_space(&text) {
                ty = Some(normalise(&text[..at]));
                text = text[at..].trim().to_string();
            }
        }
        Language::Ruby | Language::Unknown => {}
    }
    param.name = if text.is_empty() {
        "(anonymous)".to_string()
    } else {
        text
    };
    param.ty = ty
        .map(|ty| ty.trim().to_string())
        .filter(|ty| !ty.is_empty());
    param.default = default
        .map(|default| default.trim().to_string())
        .filter(|default| !default.is_empty());
    if param.default.is_some() || param.rest {
        param.optional = true;
    }
    if let Some(ty) = &param.ty {
        // `Option<T>` in Rust and `T | None` in Python are the optional spellings that carry a type.
        if ty.starts_with("Option<") || ty.ends_with("| None") || ty.starts_with("Optional<") {
            param.optional = true;
        }
    }
    param
}

// ==============================================================================================
// finding the name in a header
// ==============================================================================================

/// The words that introduce a declaration and therefore are not its name, per language.
fn skip_words(language: Language) -> &'static [&'static str] {
    match language {
        Language::Rust => &[
            "pub", "async", "unsafe", "extern", "default", "fn", "struct", "enum", "trait",
            "union", "type", "const", "static", "mod", "impl", "for",
        ],
        Language::Python => &["async", "def", "class"],
        Language::Go => &["func", "type", "var", "const"],
        Language::TypeScript => &[
            "export",
            "import",
            "declare",
            "default",
            "abstract",
            "async",
            "const",
            "let",
            "var",
            "function",
            "class",
            "interface",
            "type",
            "enum",
            "namespace",
            "module",
            "readonly",
            "static",
            "public",
            "private",
            "protected",
            "get",
            "set",
            "override",
            "as",
            "from",
            "is",
        ],
        Language::Java => &[
            "public",
            "protected",
            "private",
            "static",
            "final",
            "abstract",
            "sealed",
            "non",
            "class",
            "interface",
            "enum",
            "record",
            "default",
            "native",
            "synchronized",
        ],
        Language::Ruby | Language::Unknown => &[],
    }
}

/// Where the declared name starts, or `None` when the header does not look like the declaration its own
/// kind claims — which is an `unparsed`, not a guess.
fn name_position(language: Language, head: &str) -> Option<usize> {
    let skip = skip_words(language);
    let bytes = head.as_bytes();
    let mut index = 0usize;
    for _ in 0..16 {
        index = skip_spaces(head, index);
        if index >= head.len() {
            return None;
        }
        // `pub(crate)`, `pub(in crate::geo)`: Rust's bracketed visibility, which is not a word.
        if language == Language::Rust && bytes[index] == b'(' {
            let scan = Scan::new(head, language);
            index = scan.close(index)?;
            continue;
        }
        // A Go method's receiver sits between `func` and the name.
        if language == Language::Go && bytes[index] == b'(' {
            let scan = Scan::new(head, language);
            index = scan.close(index)?;
            continue;
        }
        if bytes[index] == b'<' {
            // A generic list where the name would be: the name has already been found.
            break;
        }
        let word = word_at(head, index);
        if word.is_empty() {
            break;
        }
        if skip.contains(&word.as_str()) {
            index += word.len();
            continue;
        }
        break;
    }
    let index = skip_spaces(head, index);
    if index >= head.len() {
        return None;
    }
    if !head[index..].starts_with(char::is_alphabetic) && !head[index..].starts_with('_') {
        return None;
    }
    Some(index)
}

/// The byte just past the identifier starting at `at`.
fn word_end(head: &str, at: usize) -> usize {
    let mut index = at;
    while index < head.len() {
        let ch = head[index..].chars().next().unwrap_or('\0');
        if ch.is_alphanumeric() || ch == '_' {
            index += ch.len_utf8();
        } else {
            break;
        }
    }
    index
}

fn skip_spaces(text: &str, at: usize) -> usize {
    let mut index = at;
    while index < text.len() {
        match text.as_bytes()[index] {
            b' ' | b'\t' | b'\n' | b'\r' => index += 1,
            _ => break,
        }
    }
    index
}

fn word_at(text: &str, at: usize) -> String {
    let end = word_end(text, at);
    text[at..end].to_string()
}

// ==============================================================================================
// the member lines of a type-like declaration
// ==============================================================================================

/// The member lines of a type-like declaration: a struct's fields, an interface's members, an enum's
/// variants, a Python class's attributes.
///
/// These bodies are the exception to "the header, never the body", because in them the body *is* the
/// contract: a caller that constructs the struct or implements the interface names every line of it.
/// Lines that belong to a member the extractor already declared as a symbol are skipped — a method is
/// an [`crate::api::surface::Item`], and reporting it a second time as a field would double-count a change to it.
///
/// The second value is whether the list was capped at [`MAX_FIELDS_PER_ITEM`], which makes the item's
/// member list a lower bound and says so in the report.
#[must_use]
pub fn fields_of(
    scan: &Scan,
    language: Language,
    header: &Header,
    span: &Span,
    own: &[&Symbol],
) -> (Vec<Field>, bool) {
    if language == Language::Ruby {
        return (Vec::new(), false);
    }
    let own_names: Vec<&str> = own
        .iter()
        .map(|symbol| symbol.name.rsplit('.').next().unwrap_or(&symbol.name))
        .filter(|name| !name.is_empty())
        .collect();
    let mut lines: Vec<String> = Vec::new();
    match language {
        Language::Python => {
            // The class body is the indented block after the header. A line more indented than the
            // first line of that block belongs to a nested body (a method), and is not a member.
            let raw = &scan.raw[..span.end_byte.min(scan.raw.len())];
            let header_line_end = scan
                .line_end(header.body_start.min(raw.len()))
                .min(raw.len());
            let indent = indent_of(scan.raw.get(..header.body_start).unwrap_or(""));
            let mut block: Option<usize> = None;
            for line in raw[header_line_end.min(raw.len())..].lines() {
                if line.trim().is_empty() {
                    continue;
                }
                let line_indent = indent_of(line);
                if line_indent <= indent {
                    break;
                }
                match block {
                    None => block = Some(line_indent),
                    Some(want) if line_indent != want => continue,
                    _ => {}
                }
                lines.push(line.trim().to_string());
            }
        }
        _ => {
            let open = header.body_start.min(scan.raw.len());
            if scan.raw.as_bytes().get(open) != Some(&b'{') {
                return (Vec::new(), false);
            }
            let Some(close) = scan.close(open) else {
                return (Vec::new(), false);
            };
            for piece in body_pieces(scan, open + 1..close.saturating_sub(1).max(open + 1)) {
                lines.push(piece);
            }
        }
    }
    let mut out: Vec<Field> = Vec::new();
    let mut truncated = false;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if declares_own_member(language, &line, &own_names) {
            continue;
        }
        let Some(field) = field_of(language, &line) else {
            continue;
        };
        if out.len() >= MAX_FIELDS_PER_ITEM {
            truncated = true;
            continue;
        }
        out.push(field);
    }
    (out, truncated)
}

/// The member lines inside a declaration's `{ … }`, cut at the *body's own* level.
///
/// A body contains its methods, and their statements are not members: cutting the interior on every
/// comma, semicolon and newline the way a flat list would be cut would report `return this.w * this.h`
/// as a field of the class that contains it. So a cut only counts at the level the body itself is on,
/// and a piece that begins deeper — inside a method, inside a nested literal — is dropped.
fn body_pieces(scan: &Scan, range: std::ops::Range<usize>) -> Vec<String> {
    let base = scan.level_at(range.start);
    let mut out: Vec<String> = Vec::new();
    let mut start = range.start;
    for (offset, ch) in scan.mask[range.clone()].char_indices() {
        let at = range.start + offset;
        if !matches!(ch, ';' | ',' | '\n' | '{' | '}') || !scan.at_level(at, base) {
            continue;
        }
        if scan.level_at(start) == base {
            let piece = normalise(scan.text(start..at));
            if !piece.is_empty() {
                out.push(piece);
            }
        }
        start = at + 1;
    }
    if scan.level_at(start) == base {
        let piece = normalise(scan.text(start..range.end));
        if !piece.is_empty() {
            out.push(piece);
        }
    }
    out
}

/// Whether a member line is the declaration of a symbol the extractor already named.
fn declares_own_member(language: Language, line: &str, own_names: &[&str]) -> bool {
    let keyword = match language {
        Language::Rust => "fn ",
        Language::Python => "def ",
        Language::Go => "func ",
        _ => "",
    };
    for name in own_names {
        if !keyword.is_empty()
            && (line.starts_with(&format!("{keyword}{name}("))
                || line.contains(&format!(" {keyword}{name}("))
                || line.contains(&format!("{keyword}{name}(")))
        {
            return true;
        }
        // TypeScript and Java write a method as `name(args)` with no keyword.
        if keyword.is_empty()
            && (line.starts_with(&format!("{name}(")) || line.contains(&format!(" {name}(")))
        {
            return true;
        }
    }
    false
}

/// Whether a Rust member line is an enum variant rather than a field: a variant has no colon and no
/// visibility marker, and it is public by virtue of its enum being public.
fn is_variant_like(left: &str) -> bool {
    !left.contains(':')
        && !left.contains('(')
        && left.chars().next().is_some_and(char::is_uppercase)
}

/// One member line, per language. `None` for a line that carries no member (a comment, `pass`, a
/// statement).
fn field_of(language: Language, text: &str) -> Option<Field> {
    let mut text = normalise(text);
    if text.is_empty() || text.starts_with('~') {
        return None;
    }
    let mut name: String;
    let mut ty: Option<String> = None;
    let mut optional = false;
    match language {
        Language::Python => {
            let left = match split_once_top(&text, '=') {
                Some((left, _)) => {
                    optional = true;
                    left.trim().to_string()
                }
                None => text.clone(),
            };
            match split_once_top(&left, ':') {
                Some((left, right)) => {
                    name = left.trim().to_string();
                    ty = Some(right.trim().to_string());
                }
                None => name = left.trim().to_string(),
            }
        }
        Language::Rust => {
            let mut left = text.as_str().trim();
            // A Rust field's access is written on the field, and a field that is not `pub` is not part
            // of what a caller may name — the same rule [`crate::api::extract`] applies to a method.
            // `pub` is therefore required, and the restricted forms of it rejected, before anything else
            // about the line is read.
            for restricted in ["pub(crate) ", "pub(super) ", "pub(in "] {
                if left.starts_with(restricted) {
                    return None;
                }
            }
            let mut public = false;
            if let Some(rest) = left.strip_prefix("pub ") {
                public = true;
                left = rest.trim();
            }
            if !public && !is_variant_like(left) {
                return None;
            }
            match split_once_top(left, ':') {
                Some((left, right)) => {
                    name = left.trim().to_string();
                    ty = Some(right.trim().to_string());
                }
                None => name = left.trim().to_string(),
            }
            text = normalise(left);
        }
        Language::Go => match split_once_whitespace(&text) {
            Some((left, right)) => {
                name = left.trim().to_string();
                ty = Some(right.trim().to_string());
            }
            None => name = text.clone(),
        },
        Language::TypeScript => {
            let mut left = text.as_str().trim();
            for prefix in [
                "public ",
                "private ",
                "protected ",
                "readonly ",
                "static ",
                "abstract ",
                "declare ",
            ] {
                if let Some(rest) = left.strip_prefix(prefix) {
                    left = rest.trim();
                }
            }
            if let Some((left, right)) = split_once_top(left, ':') {
                name = left.trim().to_string();
                ty = Some(right.trim().to_string());
            } else {
                name = left.trim().to_string();
            }
            if let Some(stripped) = name.strip_suffix('?') {
                name = stripped.to_string();
                optional = true;
            }
        }
        Language::Java => {
            match last_top_level_space(&text) {
                Some(at) => {
                    ty = Some(normalise(&text[..at]));
                    name = text[at..].trim().to_string();
                }
                None => name = text.clone(),
            }
            if name.starts_with('=') {
                // `int LIMITS = 4;` — a constant with a value: the type and name split above still hold.
                optional = true;
            }
        }
        Language::Ruby | Language::Unknown => return None,
    }
    if name.is_empty() {
        return None;
    }
    // A member this parser cannot name is still a member: it keeps the whole line as its name so that a
    // change to it is a change, reported, rather than a silence.
    Some(Field {
        name,
        ty: ty.filter(|value| !value.is_empty()),
        optional,
        text,
    })
}

// ==============================================================================================
// deprecation
// ==============================================================================================

/// The author's deprecation note, if the declaration carries a deprecation marker.
///
/// The marker is read from the lines *above* the declaration, because that is where all five languages
/// put it: a Rust attribute, a Python decorator, a Go doc comment, a JSDoc block, a Java annotation.
/// Six lines is enough for any of them and short enough that a comment belonging to the *previous*
/// declaration is not read as this one's.
#[must_use]
pub fn deprecation_of(scan: &Scan, language: Language, span: &Span) -> Option<String> {
    let mut prologue: Vec<String> = Vec::new();
    // Start from the beginning of the declaration's *line*, not from its first byte: TypeScript's
    // extractor spans an exported function from the `function` keyword, so a walk that began at the
    // span would start in the middle of `export function legacy` and give up on the JSDoc above it.
    let mut cursor = scan
        .line_start(span.start_byte.min(scan.raw.len()))
        .saturating_sub(1);
    for _ in 0..16 {
        // The byte before a declaration's line is the line break that ended the previous one, and
        // sometimes its indentation; the line worth reading is the one before that.
        while cursor > 0 && matches!(scan.raw.as_bytes()[cursor], b'\n' | b'\r' | b' ' | b'\t') {
            cursor -= 1;
        }
        if cursor == 0 {
            break;
        }
        let start = scan.line_start(cursor);
        let line = scan.raw[start..=cursor].trim().to_string();
        if line.is_empty() {
            break;
        }
        let kept = match language {
            Language::Python | Language::Ruby => line.starts_with('#') || line.starts_with('@'),
            _ => {
                line.starts_with("//")
                    || line.starts_with("/*")
                    || line.starts_with('*')
                    || line.starts_with("*/")
                    || line.starts_with("#[")
                    || line.starts_with("@")
                    || line.ends_with("*/")
            }
        };
        if !kept {
            break;
        }
        prologue.push(line);
        if start == 0 {
            break;
        }
        cursor = start - 1;
    }
    prologue.reverse();
    let haystack = prologue.join("\n");
    if let Some(note) = deprecation_in(language, &haystack) {
        return Some(note);
    }
    let own = scan.text(span.start_byte.min(scan.raw.len())..span.end_byte.min(scan.raw.len()));
    let head: Vec<&str> = own.lines().take(3).collect();
    deprecation_in(language, &head.join("\n"))
}

/// The five spellings, each with its own marker so that a comment merely *mentioning* deprecation is
/// not read as one.
fn deprecation_in(language: Language, text: &str) -> Option<String> {
    for line in text.lines() {
        let trimmed = line.trim();
        match language {
            Language::Rust => {
                if trimmed.contains("#[deprecated") {
                    return Some(quoted(trimmed).unwrap_or_else(|| "deprecated".to_string()));
                }
            }
            Language::Python => {
                if trimmed.starts_with("@deprecated") || trimmed.contains(".. deprecated::") {
                    return Some(quoted(trimmed).unwrap_or_else(|| "deprecated".to_string()));
                }
            }
            Language::Go => {
                if let Some(note) = trimmed
                    .trim_start_matches('/')
                    .trim()
                    .strip_prefix("Deprecated:")
                {
                    let note = note.trim();
                    return Some(if note.is_empty() {
                        "deprecated".to_string()
                    } else {
                        note.to_string()
                    });
                }
            }
            Language::TypeScript | Language::Java => {
                let bare = trimmed.trim_start_matches('*').trim();
                if let Some(rest) = bare.strip_prefix("@deprecated") {
                    let rest = rest.trim();
                    return Some(if rest.is_empty() {
                        "deprecated".to_string()
                    } else {
                        rest.to_string()
                    });
                }
            }
            Language::Ruby | Language::Unknown => {}
        }
    }
    None
}

/// The first quoted span of a line, which is where a note lives in the spellings that use one.
fn quoted(line: &str) -> Option<String> {
    let (_, rest) = line.split_once('"')?;
    rest.split_once('"').map(|(note, _)| note.to_string())
}

// ==============================================================================================
// shared text helpers
// ==============================================================================================

/// The ranges between top-level commas inside `range`.
fn split_top(scan: &Scan, range: std::ops::Range<usize>) -> Vec<std::ops::Range<usize>> {
    let cuts = scan.split(range.clone(), ',');
    let mut out = Vec::with_capacity(cuts.len() + 1);
    let mut start = range.start;
    for cut in cuts {
        out.push(start..cut);
        start = cut + 1;
    }
    out.push(start..range.end);
    out
}

/// Split on the first top-level `needle`. Depth is tracked over brackets and generics, because
/// `Map<String, Integer> by_id` has a comma that is not a separator.
fn split_once_top(text: &str, needle: char) -> Option<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut angle = 0i32;
    for (index, ch) in chars.iter().enumerate() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '<' if angle < 8
                && index > 0
                && (chars[index - 1].is_alphanumeric()
                    || chars[index - 1] == '>'
                    || chars[index - 1] == '_'
                    || chars[index - 1] == ':') =>
            {
                angle += 1;
            }
            '>' if angle > 0 && index > 0 && chars[index - 1] != '=' => angle -= 1,
            _ => {}
        }
        if depth <= 0 && angle <= 0 && *ch == needle {
            let left: String = chars[..index].iter().collect();
            let right: String = chars[index + 1..].iter().collect();
            return Some((left.trim().to_string(), right.trim().to_string()));
        }
    }
    None
}

/// Split a Go entry into name and the rest: `w float64`, `(r *Rect)`, `opts ...string`.
fn split_once_whitespace(text: &str) -> Option<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut angle = 0i32;
    for (index, ch) in chars.iter().enumerate() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '<' if angle < 8 && index > 0 && chars[index - 1].is_alphanumeric() => angle += 1,
            '>' if angle > 0 && index > 0 && chars[index - 1] != '=' => angle -= 1,
            _ => {}
        }
        if depth <= 0 && angle <= 0 && ch.is_whitespace() {
            let left: String = chars[..index].iter().collect();
            let right: String = chars[index + 1..].iter().collect();
            if right.trim().is_empty() {
                return None;
            }
            return Some((left.trim().to_string(), right.trim().to_string()));
        }
    }
    None
}

/// The byte offset of the last whitespace that is not inside brackets — Java's type/name seam.
fn last_top_level_space(text: &str) -> Option<usize> {
    let chars: Vec<char> = text.chars().collect();
    let mut depth = 0i32;
    let mut angle = 0i32;
    let mut last: Option<usize> = None;
    let mut byte = 0usize;
    for ch in chars.iter() {
        match ch {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            '<' if depth <= 0 && angle < 8 => angle += 1,
            '>' if angle > 0 => angle -= 1,
            _ => {}
        }
        if depth <= 0 && angle <= 0 && ch.is_whitespace() {
            last = Some(byte);
        }
        byte += ch.len_utf8();
    }
    last
}

/// The byte just past the `>` that closes the generic list opening at `open`.
fn angle_close(scan: &Scan, open: usize) -> Option<usize> {
    let mut angle = 0i32;
    let mut index = open;
    let bytes = scan.mask.as_bytes();
    while index < bytes.len() {
        match bytes[index] {
            b'<' => angle += 1,
            b'>' => {
                angle -= 1;
                if angle == 0 {
                    return Some(index + 1);
                }
            }
            b'(' | b'{' => return None,
            _ => {}
        }
        index += 1;
    }
    None
}

/// The indentation of a line, in bytes of leading whitespace.
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}
