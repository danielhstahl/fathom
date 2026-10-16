//! `fathom_tui::diff::word` — which *parts* of a changed line changed.
//!
//! `git show --word-diff` answers this and Fathom has to answer it too, because on the screen people
//! live in the difference is `epoch ==` → `epoch !=` and not the 76 characters around it. The algorithm
//! is a token LCS over the two sides of one changed pair: quadratic in tokens, so it is capped, and a
//! line with more tokens than the cap gets the honest whole-line answer rather than a wrong one.
//!
//! The output is byte ranges on each side, which is all the renderer needs; the *fact* is also printed
//! in words in the pane's footer, because ADR-006 §10.2 says a colour may not carry a fact alone and a
//! `--plain` run has no colour to carry it with.

use std::ops::Range;

/// Tokens per side before the quadratic pass is abandoned.
const MAX_TOKENS: usize = 400;

/// One token, with where it was.
struct Token<'a> {
    text: &'a str,
    range: Range<usize>,
    /// Whitespace tokens cannot be a change of their own; they ride along with their neighbours.
    blank: bool,
}

/// Split into tokens: runs of blanks, runs of word characters, and single other characters.
fn tokenize(text: &str) -> Vec<Token<'_>> {
    let bytes = text.as_bytes();
    let mut out: Vec<Token<'_>> = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        let start = index;
        let byte = bytes[index];
        if byte.is_ascii_whitespace() {
            while index < bytes.len() && bytes[index].is_ascii_whitespace() {
                index += 1;
            }
        } else if byte.is_ascii_alphanumeric() || byte == b'_' {
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index += 1;
            }
        } else {
            // One code point, so a multi-byte character is never cut in half.
            match text[index..].chars().next() {
                Some(ch) => index += ch.len_utf8(),
                None => break,
            }
        }
        out.push(Token {
            text: &text[start..index],
            range: start..index,
            blank: text[start..index].chars().all(char::is_whitespace),
        });
    }
    out
}

/// The changed byte ranges on each side: `(removed ranges on old, added ranges on new)`.
///
/// `(whole line, whole line)` is a real answer and means "more tokens than the diff is allowed to
/// reason about"; an empty pair means the two sides are identical.
#[must_use]
pub fn regions(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    if old == new {
        return (Vec::new(), Vec::new());
    }
    let left = tokenize(old);
    let right = tokenize(new);
    if left.len() > MAX_TOKENS || right.len() > MAX_TOKENS {
        return whole(old, new);
    }
    let keep = lcs_kept(&left, &right);
    let (mut a, mut b) = (Vec::new(), Vec::new());
    merge(&left, &keep.0, old, &mut a);
    merge(&right, &keep.1, new, &mut b);
    (a, b)
}

/// The fallback answer, with the whole line on both sides.
fn whole(old: &str, new: &str) -> (Vec<Range<usize>>, Vec<Range<usize>>) {
    let one = |text: &str| {
        let mut spans = Vec::new();
        if !text.is_empty() {
            spans.push(0..text.len());
        }
        spans
    };
    (one(old), one(new))
}

/// Longest-common-subsequence membership, as one `bool` per token per side.
fn lcs_kept(left: &[Token<'_>], right: &[Token<'_>]) -> (Vec<bool>, Vec<bool>) {
    let (n, m) = (left.len(), right.len());
    // `table[i][j]` = tokens common to `left[i..]` and `right[j..]`.
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if left[i].text == right[j].text {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let mut keep_left = vec![false; n];
    let mut keep_right = vec![false; m];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if left[i].text == right[j].text {
            keep_left[i] = true;
            keep_right[j] = true;
            i += 1;
            j += 1;
        } else if table[i + 1][j] >= table[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    (keep_left, keep_right)
}

/// Turn "which tokens are unchanged" into "which byte ranges changed", merging runs.
fn merge(tokens: &[Token<'_>], kept: &[bool], text: &str, out: &mut Vec<Range<usize>>) {
    let mut open: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        let unchanged = kept[index];
        // A blank between two changed tokens belongs to the change, or the emphasis reads as two.
        let changed = !unchanged || (token.blank && looks_glued(tokens, kept, index));
        match (open, changed) {
            (None, true) => open = Some(token.range.start),
            (Some(start), false) => {
                out.push(start..token.range.start);
                open = None;
            }
            _ => {}
        }
    }
    if let Some(start) = open {
        out.push(start..text.len());
    }
}

/// A blank is part of the change when the tokens either side of it are changed on this side.
fn looks_glued(tokens: &[Token<'_>], kept: &[bool], index: usize) -> bool {
    let before = index
        .checked_sub(1)
        .filter(|i| !tokens[*i].blank)
        .is_some_and(|i| !kept[i]);
    let after = (index + 1..tokens.len())
        .find(|i| !tokens[*i].blank)
        .is_some_and(|i| !kept[i]);
    before && after
}

/// The words a footer line prints: `epoch == → epoch !=`, first few only.
///
/// This is the sentence the colour is not carrying alone, so it says the same thing the emphasis does.
#[must_use]
pub fn summary(old: &str, new: &str, limit: usize) -> String {
    let (removed, added) = regions(old, new);
    if removed.is_empty() && added.is_empty() {
        return "no difference on that line".to_owned();
    }
    let cut = |text: &str, range: &Range<usize>, width: usize| {
        let piece = &text[range.clone()];
        let piece = piece.trim();
        if piece.chars().count() > width {
            let head: String = piece.chars().take(width).collect();
            format!("{head}…")
        } else {
            piece.to_owned()
        }
    };
    let mut pairs: Vec<String> = Vec::new();
    let count = removed.len().max(added.len()).min(limit.max(1));
    for index in 0..count {
        let left = removed
            .get(index)
            .map_or_else(String::new, |range| cut(old, range, 24));
        let right = added
            .get(index)
            .map_or_else(String::new, |range| cut(new, range, 24));
        pairs.push(match (left.is_empty(), right.is_empty()) {
            (true, false) => format!("+{right}"),
            (false, true) => format!("-{left}"),
            _ => format!("{left} → {right}"),
        });
    }
    if removed.len().max(added.len()) > count {
        pairs.push(format!(
            "+{} more",
            crate::app::format_thousands((removed.len().max(added.len()) - count) as u64)
        ));
    }
    pairs.join(" · ")
}
