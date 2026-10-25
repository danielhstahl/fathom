//! The byte scanner every signature parse rides on.
//!
//! Extracting a declaration's *header* is a tokenising job with no tokeniser: the grammar has already
//! told us where the declaration starts and ends, and the answer we need is which `{` opens its body,
//! which `,` separates two parameters, and which `:` introduces a type. Three things make that a
//! scanner rather than a `find`:
//!
//! * **strings lie** — `fn quote() -> &'static str { "}" }` has a brace inside a literal, and a
//!   `find('{')` returns it;
//! * **generics nest** — `Map<String, Vec<u8>>` has two commas and an angle pair, and a split on `,`
//!   that cannot tell `<` from *less than* cuts a parameter in half;
//! * **comments lie** — `pub fn a() { /* } */ }`, and the comment above a declaration is where the
//!   deprecation marker lives, so it cannot be thrown away before the prologue is read.
//!
//! So [`Scan::new`] walks the text once, replaces the *contents* of string and character literals
//! with `~` and blanks line comments (byte length preserved, so every offset still refers to the
//! original text), and records the bracket and generic depth at every byte. Everything downstream —
//! header ends, top-level comma splits, receiver and return extraction, field lines — is index
//! arithmetic over [`Scan::mask`], [`Scan::depth`] and [`Scan::angle`].
//!
//! The scanner is tolerant on purpose: an unterminated literal or an unbalanced bracket still
//! produces a mask, and a caller that cannot make sense of the result says *unparsed* rather than
//! guessing (see [`crate::api::classify::Reason::SignatureUnparsed`]). This is a source-level
//! approximation of a compiler's view, written to fail loudly rather than quietly.

use crate::structure::Language;

/// The placeholder byte that stands in for string contents.
const MASK: u8 = b'~';

/// One text, masked and measured.
pub struct Scan {
    /// The original text.
    pub raw: String,
    /// The same length as `raw`, with string contents replaced by `~` and line comments blanked.
    pub mask: String,
    /// Bracket depth (`()`, `[]`, `{}`) at each byte of `mask`. The depth *inside* an opener, so the
    /// `(` of `f(x)` reads 1 and its `,` reads 1 too.
    pub depth: Vec<u8>,
    /// Bracket depth *before* each byte, which is what "top level" has to mean: the `(` that opens a
    /// parameter list sits at depth 1 by [`Scan::depth`] and is nevertheless top-level, and a scanner
    /// that asked [`Scan::depth`] would never find a parameter list at all.
    pub outside: Vec<u8>,
    /// Generic depth at each byte, from the `<` heuristic below.
    pub angles: Vec<u8>,
    /// Which bytes were inside a string literal or a comment when the mask was built.
    pub blanked: Vec<bool>,
}

impl Scan {
    /// Mask and measure one text. Never fails: an unterminated literal is blanked to the end of its
    /// line, which is what a reader would guess too.
    #[must_use]
    pub fn new(text: &str, language: Language) -> Scan {
        let bytes = text.as_bytes();
        let mut mask: Vec<u8> = bytes.to_vec();
        let mut depth: Vec<u8> = vec![0; bytes.len()];
        let mut outside: Vec<u8> = vec![0; bytes.len()];
        let mut angles: Vec<u8> = vec![0; bytes.len()];
        let mut blanked: Vec<bool> = vec![false; bytes.len()];
        let mut level: i32 = 0;
        let mut angle = 0i32;
        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            // --- string and character literals ----------------------------------------------------
            if let Some((terminator, triple)) = opening_quote(byte, index, bytes, language) {
                let start = index;
                let lead = if triple { 3 } else { 1 };
                index += lead;
                while index < bytes.len() {
                    if bytes[index] == b'\\' {
                        blank_range(&mut mask, &mut blanked, index, (index + 2).min(bytes.len()));
                        index += 2;
                        continue;
                    }
                    if bytes[index] == terminator {
                        if triple {
                            // A triple-quoted literal closes on its own delimiter and nothing else: a
                            // lone quote inside it is content (`"""He said "hi"."""` is one string), and
                            // calling it the close would read the rest of the docstring as code.
                            if bytes[index..].len() >= 3
                                && bytes[index + 1] == terminator
                                && bytes[index + 2] == terminator
                            {
                                index += 3;
                            } else {
                                index += 1;
                                continue;
                            }
                        } else {
                            index += 1;
                        }
                        break;
                    }
                    if !triple && bytes[index] == b'\n' {
                        // An unterminated single-line literal: give up at the newline rather than
                        // blanking the rest of the file. A triple-quoted literal runs across newlines —
                        // that is what it is for — so stopping it here would hand the rest of a
                        // Python docstring back to the scanner as source.
                        break;
                    }
                    index += 1;
                }
                blank_range(
                    &mut mask,
                    &mut blanked,
                    start.saturating_add(lead).min(bytes.len()),
                    index,
                );
                continue;
            }
            // --- comments --------------------------------------------------------------------------
            if is_line_comment(byte, index, bytes, language) {
                let start = index;
                while index < bytes.len() && bytes[index] != b'\n' {
                    index += 1;
                }
                blank_range(&mut mask, &mut blanked, start, index);
                continue;
            }
            if byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'*' {
                let start = index;
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
                blank_range(&mut mask, &mut blanked, start, index);
                continue;
            }
            // --- brackets --------------------------------------------------------------------------
            let level_before = level;
            match byte {
                b'(' | b'[' | b'{' => level += 1,
                b')' | b']' | b'}' => level -= 1,
                b'<' if angle < 8 && angle_opens(index, bytes) => angle += 1,
                b'>' if angle > 0 && !is_arrow(index, bytes) => angle -= 1,
                _ => {}
            }
            outside[index] = clamp(level_before);
            depth[index] = clamp(level);
            angles[index] = clamp(angle);
            index += 1;
        }
        Scan {
            raw: text.to_string(),
            // Masking can cut a multi-byte character in half, and the mask must stay **byte-for-byte
            // aligned with `raw`**: `depth`, `outside` and `angles` are indexed by mask offsets, and
            // `line_start`/`line_end`/`text` slice `raw` with them. `String::from_utf8_lossy` looks like the
            // obvious repair and is not one: it replaces one invalid byte with U+FFFD, which is three bytes,
            // so every offset after the cut shifts by two — which is how a repo containing `→` inside a
            // blanked comment made `raw[..at]` panic on a non-char boundary in shipped code. So the broken
            // bytes are replaced one-for-one with a space (what `blank_range` already uses), which keeps
            // `mask.len() == raw.len()` as an invariant rather than a hope.
            mask: aligned_mask(mask, text),
            depth,
            outside,
            angles,
            blanked,
        }
    }

    /// The bracket depth at `at` — the depth *inside* the bracket that contains `at`, or, at an
    /// opener, the depth its contents will have.
    #[must_use]
    pub fn depth(&self, at: usize) -> u8 {
        *self.depth.get(at).unwrap_or(&0)
    }

    /// The bracket depth around `at`: what [`Scan::depth`] reports once the bracket at `at` itself is
    /// accounted for. A caller asking "is this comma between two arguments, or inside a nested list?"
    /// wants this, not [`Scan::depth`].
    #[must_use]
    pub fn outside(&self, at: usize) -> u8 {
        *self.outside.get(at).unwrap_or(&0)
    }

    /// The generic depth at `at`.
    #[must_use]
    pub fn angle(&self, at: usize) -> u8 {
        *self.angles.get(at).unwrap_or(&0)
    }

    /// Whether the byte at `at` sits outside every bracket and every generic argument list.
    #[must_use]
    pub fn top_level(&self, at: usize) -> bool {
        !self.blanked(at) && self.outside(at) == 0 && self.angle_outside(at) == 0
    }

    /// The generic depth around `at`, in the same sense as [`Scan::outside`].
    #[must_use]
    pub fn angle_outside(&self, at: usize) -> u8 {
        let stored = *self.angles.get(at).unwrap_or(&0);
        match self.mask.as_bytes().get(at).copied() {
            // `<` raised the count to `stored`, so the surrounding count is one lower; `>` lowered it,
            // so the surrounding count is one higher. Both are the reading a caller wants when it asks
            // whether a byte is *outside* a generic parameter list.
            Some(b'<') => stored.saturating_sub(1),
            Some(b'>') => stored.saturating_add(1),
            _ => stored,
        }
    }

    /// The bracket and generic level *around* `at` — how many were already open when `at` was reached.
    /// A range of text usually starts inside something (a method inside an `impl`, a parameter inside
    /// its parentheses), and a caller asking for "the first top-level `:` in this declaration" means
    /// "the first one at the level the declaration itself is on".
    #[must_use]
    pub fn level_at(&self, at: usize) -> (u8, u8) {
        (self.outside(at), self.angle_outside(at))
    }

    /// Whether `at` sits at `base` level or shallower, and is not inside a string or comment.
    #[must_use]
    pub fn at_level(&self, at: usize, base: (u8, u8)) -> bool {
        !self.blanked(at) && self.outside(at) <= base.0 && self.angle_outside(at) <= base.1
    }

    /// Whether `at` is inside a string literal or a comment.
    #[must_use]
    pub fn blanked(&self, at: usize) -> bool {
        *self.blanked.get(at).unwrap_or(&false)
    }

    /// The byte offsets of `separator` inside `range` at the range's own outermost level.
    ///
    /// The base is taken from where the range starts, not from zero: a parameter list passed as the
    /// bytes between its parentheses lives at depth 1, and its separators are the ones at *that*
    /// depth. A scanner that asked "depth 0" here would find no separators at all and hand back the
    /// whole list as one parameter — which is a wrong answer of the kind that reads as a right one.
    #[must_use]
    pub fn split(&self, range: std::ops::Range<usize>, separator: char) -> Vec<usize> {
        let base = self.level_at(range.start);
        let mut out = Vec::new();
        for (offset, ch) in self.mask[range.clone()].char_indices() {
            let at = range.start + offset;
            if ch == separator && self.at_level(at, base) {
                out.push(at);
            }
        }
        out
    }

    /// The first byte of `range` holding one of `candidates` at top level.
    #[must_use]
    pub fn find(&self, range: std::ops::Range<usize>, candidates: &str) -> Option<(usize, char)> {
        let base = self.level_at(range.start);
        for (offset, ch) in self.mask[range.clone()].char_indices() {
            let at = range.start + offset;
            if candidates.contains(ch) && self.at_level(at, base) {
                return Some((at, ch));
            }
        }
        None
    }

    /// The byte just past the closing bracket that matches the opener at `open`.
    #[must_use]
    pub fn close(&self, open: usize) -> Option<usize> {
        let opener = self.mask.as_bytes().get(open).copied()?;
        let closer = match opener {
            b'(' => b')',
            b'[' => b']',
            b'{' => b'}',
            _ => return None,
        };
        // The opener's contents sit one level deeper than the opener's own surroundings, and a closer
        // records the depth it *falls back to*, so the two numbers meet at the match.
        let base = self.outside(open);
        let mut index = open + 1;
        while index < self.mask.len() {
            let byte = self.mask.as_bytes()[index];
            if byte == closer && self.depth(index) <= base {
                return Some(index + 1);
            }
            index += 1;
        }
        None
    }

    /// The end of the line containing `at`, exclusive.
    ///
    /// `at` is snapped forward to a character boundary: the arithmetic that produces it is byte arithmetic
    /// over the mask, and a caller that hands back an offset landed mid-glyph should get the rest of its
    /// line, not a panic. With `aligned_mask` this cannot happen; it stays because the alternative is a
    /// crash in a library a hostile file is pointed at.
    #[must_use]
    pub fn line_end(&self, at: usize) -> usize {
        let at = self.ceil_char(at);
        self.raw[at..]
            .find('\n')
            .map(|offset| at + offset)
            .unwrap_or(self.raw.len())
    }

    /// The start of the line containing `at`.
    #[must_use]
    pub fn line_start(&self, at: usize) -> usize {
        let at = self.floor_char(at);
        self.raw[..at].rfind('\n').map_or(0, |offset| offset + 1)
    }

    /// The masked substring of `range`.
    #[must_use]
    pub fn masked(&self, range: std::ops::Range<usize>) -> &str {
        let start = self.floor_char(range.start);
        let end = self.ceil_char(range.end.min(self.raw.len()));
        &self.mask[start..end]
    }

    /// The raw substring of `range`.
    #[must_use]
    pub fn text(&self, range: std::ops::Range<usize>) -> &str {
        let start = self.floor_char(range.start);
        let end = self.ceil_char(range.end.min(self.raw.len()));
        &self.raw[start..end]
    }

    fn floor_char(&self, at: usize) -> usize {
        let mut value = at.min(self.raw.len());
        while value > 0 && !self.raw.is_char_boundary(value) {
            value -= 1;
        }
        value
    }

    fn ceil_char(&self, at: usize) -> usize {
        let mut value = at.min(self.raw.len());
        while value < self.raw.len() && !self.raw.is_char_boundary(value) {
            value += 1;
        }
        value
    }
}

/// Re-encode a masked byte buffer as valid UTF-8 **without changing its length**.
///
/// Masking operates on bytes, so blanking a range can cut a multi-byte character in half. The naive repair
/// (`String::from_utf8_lossy`) re-encodes each broken byte as U+FFFD — three bytes for one — and every
/// offset after the cut silently moves two bytes right, which misaligns the mask against `raw`, against the
/// `depth`/`outside`/`angles` columns, and against every slice of `raw` a caller takes with a mask-derived
/// index. That is not a theoretical hazard: a repository with `→` inside a blanked comment made
/// `raw[..at]` panic on a non-char boundary.
///
/// The walk is over the *source* string's characters: a character the masker left alone is copied whole, and
/// one it broke has exactly its own bytes replaced with spaces — what `blank_range` already blanks with, so
/// the mask's vocabulary is unchanged and `mask.len() == raw.len()` holds by construction.
fn aligned_mask(mask: Vec<u8>, text: &str) -> String {
    debug_assert_eq!(
        mask.len(),
        text.len(),
        "the mask is built one byte per input byte"
    );
    let mut bytes = mask;
    let mut offset = 0;
    for ch in text.chars() {
        let width = ch.len_utf8();
        let span = &mut bytes[offset..offset + width];
        if std::str::from_utf8(span).is_err() {
            span.fill(b' ');
        }
        offset += width;
    }
    // Unreachable by construction, and still not a `unwrap`: a library this far from the input does not get
    // to panic because a helper got it wrong.
    String::from_utf8(bytes).unwrap_or_else(|_| text.to_owned())
}

/// Clamp a running depth into a `u8` column; a header with 255 open brackets is not a header.
fn clamp(level: i32) -> u8 {
    if level < 0 {
        0
    } else if level > 250 {
        250
    } else {
        level as u8
    }
}

/// Blank `[from, to)` in the mask, recording which bytes were blanked.
fn blank_range(mask: &mut [u8], blanked: &mut [bool], from: usize, to: usize) {
    for index in from..to.min(mask.len()) {
        if mask[index] != b'\n' {
            mask[index] = MASK;
        }
        blanked[index] = true;
    }
}

/// Whether `byte` at `index` opens a literal, and what closes it: `(terminator, is_triple)`.
fn opening_quote(byte: u8, index: usize, bytes: &[u8], language: Language) -> Option<(u8, bool)> {
    let triple = |quote: u8| {
        matches!(language, Language::Python | Language::Ruby)
            && bytes[index..].len() >= 3
            && bytes[index] == quote
            && bytes[index + 1] == quote
            && bytes[index + 2] == quote
    };
    match byte {
        b'"' => Some((b'"', triple(b'"'))),
        // Rust `'a` is a lifetime, not a literal, and treating it as one would blank half a header.
        b'\'' if matches!(language, Language::Rust) => None,
        b'\'' => Some((b'\'', triple(b'\''))),
        b'`' => matches!(language, Language::TypeScript | Language::Ruby).then(|| (b'`', false)),
        _ => None,
    }
}

/// Whether a `/` or `#` at `index` starts a line comment.
fn is_line_comment(byte: u8, index: usize, bytes: &[u8], language: Language) -> bool {
    match language {
        Language::Python | Language::Ruby => byte == b'#',
        _ => byte == b'/' && index + 1 < bytes.len() && bytes[index + 1] == b'/',
    }
}

/// Whether a `<` at `index` opens a generic argument list rather than comparing.
fn angle_opens(index: usize, bytes: &[u8]) -> bool {
    if index == 0 {
        return false;
    }
    let mut cursor = index - 1;
    while cursor > 0 && bytes[cursor].is_ascii_whitespace() {
        cursor -= 1;
    }
    let byte = bytes[cursor];
    byte == b'>' || byte == b'_' || byte.is_ascii_alphanumeric() || byte >= 0x80 || byte == b':'
}

/// Whether a `>` at `index` is really half of `=>`.
fn is_arrow(index: usize, bytes: &[u8]) -> bool {
    index > 0 && bytes[index - 1] == b'='
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::*;

    /// The mask must stay byte-for-byte aligned with `raw`, or every mask-derived offset used to slice `raw`
    /// lands mid-character. This is a regression test for a real panic in shipped code: a source file with a
    /// multi-byte character inside a comment was masked byte-wise, then "repaired" with
    /// `String::from_utf8_lossy`, which re-encodes one broken byte as three — so `line_start` sliced `raw`
    /// at a non-boundary and crashed. It is asserted as an invariant (`mask.len() == raw.len()` at every
    /// offset) rather than as one input, because any multi-byte character under any blanked range reaches it.
    #[test]
    fn a_blank_that_cuts_a_multibyte_character_keeps_the_mask_aligned() {
        // Direct on the helper, because reaching it end-to-end needs a masker that stops mid-glyph and the
        // blanking rules do not always do that; the invariant is what matters and it is cheaper to state.
        let source = "let a = \"x → y\";\n";
        let mut broken = source.as_bytes().to_vec();
        let cut = source.find('→').expect("fixture has the arrow") + 1;
        // Cut the arrow in half, which is what a byte-wise blank does when its range ends inside a character.
        broken[cut] = b' ';
        let mask = aligned_mask(broken, source);
        assert_eq!(
            mask.len(),
            source.len(),
            "the mask is one byte per input byte, whatever it blanked"
        );
        assert!(
            mask.is_char_boundary(cut),
            "the mask is valid UTF-8 at the offset the blank cut: {mask:?}"
        );

        // And the two operations that crashed: called *at* an offset inside a multi-byte character, which is
        // what a byte-arithmetic caller can hand back. Snapping to a boundary is the contract, so this is the
        // assertion that would have failed before the fix — not a length check on the mask.
        let scan = Scan::new("// a → b\nfn f(a: u32) -> u32 { a }\n", Language::Rust);
        let middle = scan.raw.find('→').expect("fixture has the arrow") + 1;
        assert!(
            !scan.raw.is_char_boundary(middle),
            "the fixture must actually straddle the character"
        );
        assert_eq!(
            scan.line_start(middle),
            0,
            "a straddling offset still resolves to the line's start"
        );
        assert_eq!(
            scan.line_end(middle),
            scan.raw.find('\n').expect("fixture has a newline"),
            "a straddling offset still resolves to the line's end"
        );
        for at in 0..=scan.raw.len() {
            let start = scan.line_start(at);
            let end = scan.line_end(at);
            assert!(start <= end, "a line's bounds disagree at {at}");
            // Slicing is the whole point: if any of these went past a boundary the process would abort.
            let _ = scan.text(start..end);
        }
    }

    /// `depth` and `outside` answer different questions, and using the wrong one silently loses a
    /// signature: a caller that looks for a method's parameter list at depth zero finds none, and a
    /// signature with no parameters is a signature that changes are unattributable to — a wrong answer
    /// of the kind that reads like a right one.
    #[test]
    fn depth_and_outside_answer_different_questions() {
        let scan = Scan::new("fn f(a, b) { }", Language::Rust);
        let Some(open) = scan.find(0..scan.raw.len(), "(") else {
            panic!("the fixture has a parameter list");
        };
        assert_eq!(scan.depth(open.0), 1, "inside the paren, by `depth`");
        assert_eq!(
            scan.outside(open.0),
            0,
            "at the file's own level, by `outside`"
        );
        let Some(close) = scan.close(open.0) else {
            panic!("the fixture's paren is closed");
        };
        assert_eq!(
            &scan.raw[open.0..close],
            "(a, b)",
            "`close` is one past the `)`"
        );
    }

    /// A range's own level is what a caller means by "top level in here": a method inside an `impl` is
    /// one level down in the file, and its body's `{` is top level *for that declaration*.
    #[test]
    fn find_is_relative_to_where_the_range_starts() {
        let text = "impl Rect {\n    pub fn area(&self) -> Area {\n        1.0\n    }\n}\n";
        let scan = Scan::new(text, Language::Rust);
        let Some(start) = text.find("pub fn area") else {
            panic!("the fixture declares `area`");
        };
        let Some(brace) = scan.find(start..text.len(), "{") else {
            panic!("the declaration has a body brace at its own level");
        };
        assert_eq!(
            &text[start..brace.0],
            "pub fn area(&self) -> Area ",
            "the header ends where its own body begins, not at depth zero"
        );
    }

    /// Splitting a parameter list must not cut inside a nested list or a generic argument list.
    #[test]
    fn split_cuts_only_at_the_lists_own_level() {
        let text = "f(a: u8, b: Map<String, i32>, c: (i32, i32))";
        let scan = Scan::new(text, Language::Rust);
        let Some(open) = text.find('(') else {
            panic!("the fixture has a list");
        };
        let Some(close) = scan.close(open) else {
            panic!("the fixture's list is closed");
        };
        let cuts = scan.split(open + 1..close - 1, ',');
        let pieces: Vec<String> = cuts
            .iter()
            .scan(open + 1, |from, at| {
                let piece = text[*from..*at].trim().to_string();
                *from = at + 1;
                Some(piece)
            })
            .chain(std::iter::once(
                text[*cuts.last().unwrap_or(&open) + 1..close - 1]
                    .trim()
                    .to_string(),
            ))
            .collect();
        assert_eq!(
            pieces,
            vec!["a: u8", "b: Map<String, i32>", "c: (i32, i32)"],
            "the generic comma and the tuple comma are not separators"
        );
    }

    /// A comment or a string is not syntax: its brackets, colons and commas are inert.
    #[test]
    fn masking_makes_strings_and_comments_inert() {
        let text = "fn f(a: &str) -> String { \"a;b(c\" } // ]}{( ;\n";
        let scan = Scan::new(text, Language::Rust);
        let probes: [(&str, &str); 3] = [
            ("a;b(c", "a string"),
            ("]}{(", "a comment"),
            ("{", "the body brace"),
        ];
        for (needle, what) in probes {
            let Some(at) = text.find(needle) else {
                panic!("the fixture contains {what}");
            };
            if needle == "{" {
                assert!(!scan.blanked(at), "the body brace is real syntax: {what}");
                assert!(
                    scan.find(at + 1..text.len(), "{(").is_none(),
                    "neither the string's paren nor the comment's brace is a bracket"
                );
            } else {
                assert!(scan.blanked(at + 1), "{what} has its contents masked");
            }
        }
    }

    /// A triple-quoted literal is *multi-line*: ending a literal at its newline is right for `"a"`,
    /// which cannot cross one, and wrong for `"""a` — which is what a Python docstring is. Stopping it
    /// at the newline handed the rest of the docstring back to the scanner as source, and the quotes
    /// in the prose then opened literals that blanked the real declarations below (`reco-9k3`).
    #[test]
    fn a_triple_quoted_literal_runs_to_its_own_delimiter_not_to_a_newline() {
        let text = "\"\"\"Prose.\n\nA \"quoted\" phrase, and an apostrophe's.\n\"\"\"\n\
                    def claims(unit: str, keys: dict[str, str]) -> list[str]:\n    return []\n";
        let scan = Scan::new(text, Language::Python);
        let prose = text.find("quoted").expect("the fixture quotes");
        assert!(scan.blanked(prose), "a quote inside the prose is content");
        let declaration = text
            .find("def claims")
            .expect("the fixture declares a function");
        assert!(
            !scan.blanked(declaration),
            "the declaration after the docstring is source, not masked prose"
        );
        let open = scan
            .find(declaration..text.len(), "(")
            .expect("the parameter list is a real paren");
        assert_eq!(scan.outside(open.0), 0, "at the declaration's own level");
        let close = scan.close(open.0).expect("the parameter list is closed");
        assert_eq!(
            &scan.raw[open.0..close],
            "(unit: str, keys: dict[str, str])",
            "the parameters survived the docstring"
        );
        // And the header ends at the colon that closes it, not at one of the annotations inside it.
        let colon = scan
            .find(close..text.len(), ":")
            .expect("the header is closed by a colon");
        assert_eq!(
            &text[colon.0..text.len() - 1],
            ":\n    return []",
            "the return arrow's type is not where the header ends"
        );
    }

    /// The other half: a lone quote inside a triple-quoted literal does not close it. `"""He said
    /// "hi"."""` is one string, and closing it at the inner quote would put every line after that
    /// quote back into the "code" the scanner is reading.
    #[test]
    fn a_lone_quote_inside_a_triple_quoted_literal_is_content() {
        let text = "x = \"\"\"He said \"hi\".\"\"\"\ny = [1, 2]  # done\n";
        let scan = Scan::new(text, Language::Python);
        let hi = text.find("hi").expect("the fixture quotes");
        assert!(scan.blanked(hi), "the inner quote is inside the literal");
        let bracket = text.find('[').expect("the fixture has a list");
        assert!(
            !scan.blanked(bracket),
            "the line after the literal is code again, not the tail of a string"
        );
        assert_eq!(
            scan.depth(bracket),
            1,
            "and its bracket is counted, which is what makes the header rules work"
        );
        assert!(
            scan.blanked(text.find("done").expect("in the comment")),
            "and the comment on that line is masked like any other"
        );
    }

    /// A single-quoted literal still dies at its newline: an unterminated `'` must not blank the rest
    /// of the file, which is the reason the newline rule exists at all.
    #[test]
    fn an_unterminated_single_line_literal_still_stops_at_its_newline() {
        let text = "a = 'unterminated\nb = 2\n";
        let scan = Scan::new(text, Language::Python);
        assert!(scan.blanked(text.find("unterminated").expect("in the literal")));
        assert!(
            !scan.blanked(text.find("b = 2").expect("the next line")),
            "the next line is still source"
        );
    }
}
