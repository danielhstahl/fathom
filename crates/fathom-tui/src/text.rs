//! `fathom_tui::text` — the cell arithmetic every drawn line goes through.
//!
//! A frame is a grid of *cells*, and a terminal counts cells differently from Rust counting
//! bytes or even characters: a CJK ideograph is two cells, a combining mark is none, an emoji is
//! usually two, and a byte that means "control" is none at all but is still dangerous. Get any of
//! that wrong by one cell and the box is one cell wider than the terminal, which wraps, and a
//! wrapped box shifts every line below it by one — so this module is the single place the answer
//! is computed, and everything that draws a line calls it.
//!
//! Three rules, in order of how much damage they do when they are wrong:
//!
//! 1. **A line is only ever cut at a grapheme-cluster boundary.** `é` is one cluster whether it is
//!    `U+00E9` or `e` + `U+0301`; `👨‍👩‍👧` is one cluster of five characters. Cutting
//!    inside a cluster does not produce a shorter cluster, it produces a lone combining mark or a
//!    broken surrogate drawing as `?`, and the ellipsis lands one cell early. [`cut`] and
//!    [`clip`] walk clusters, never characters.
//! 2. **What is measured is what is drawn.** [`width`] is the width of the *sanitized* text, so a
//!    padded column cannot disagree with the string it padded — which is the whole reason
//!    [`clip`] and [`fit`] sanitize here rather than at the call site.
//! 3. **Nothing invisible is drawn as itself.** A commit message is attacker-controlled text in
//!    somebody else's terminal. A `\r`, a `U+202E` RIGHT-TO-LEFT OVERRIDE or a `U+2060` WORD
//!    JOINER can reorder, overwrite or hide part of a frame, and none of them show up as a missing
//!    glyph. [`sanitize`] replaces them with a visible placeholder of the width it counts, so the
//!    hostile byte becomes something a reviewer can see and quote. (RTL *letters* are kept: they
//!    are content. What is removed is the invisible formatting that pretends to be nothing.)
//!
//! What this module deliberately does **not** do is bidi reordering. A terminal does that, with
//! its own rules, and a frame that computed a different order would disagree with what was drawn.
//! The invariant Fathom keeps is the one it can keep: every line is exactly the cells it claims,
//! no cluster is split, and no control or formatting character reaches the screen unseen.

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

/// The ellipsis: one cell, and the only character [`clip`] ever adds.
pub const ELLIPSIS: char = '…';

/// What a byte that may not be drawn as itself turns into: one cell, and impossible to miss.
pub const REPLACEMENT: char = '\u{fffd}';

/// The character a control character becomes when it is only a break in the text — a tab or a
/// newline inside a single line is a blank, not a mystery.
pub const BLANK: char = ' ';

/// What `ch` has to be drawn as instead of itself, if anything.
///
/// `None` means "draw it as itself", which is the case for every ordinary letter, mark and symbol.
/// The `Some` arms are the characters that are either invisible (so a fact could hide inside them)
/// or line-shaped (so a one-line string could become two).
#[must_use]
pub const fn substitute(ch: char) -> Option<char> {
    match ch {
        // Line and field breaks: a line of a frame is one line, and a pane that wraps is the bug.
        '\t' | '\n' | '\r' | '\u{0b}' | '\u{0c}' => Some(BLANK),
        // C0 and DEL. `unicode-width` gives these no width at all, which is exactly the problem:
        // an unprintable byte that also costs no cells is free to hide behind a column border.
        c if (c as u32) < 0x20 || c == '\u{007f}' => Some(REPLACEMENT),
        // C1 (U+0080–U+009F): the same argument, and the same absence of width.
        '\u{0080}'..='\u{009f}' => Some(REPLACEMENT),
        // Bidi embedding, override and isolate controls. These are the characters that can turn a
        // path into an executable-looking string and a filename into a different filename, and the
        // reason a review tool must not paste them through.
        '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' => {
            Some(REPLACEMENT)
        }
        // Word joiner, invisible times/plus, notation character overlaps, and BOM-as-space. Note
        // what is *not* in this range's reach: U+200D ZERO WIDTH JOINER, which is how emoji
        // families are spelled, stays alone on the next arm of the argument and is kept.
        '\u{2060}'..='\u{2064}' | '\u{206a}'..='\u{206f}' | '\u{feff}' => Some(REPLACEMENT),
        _ => None,
    }
}

/// Is there anything in `text` that may not be drawn as itself?
///
/// The fast path for [`sanitize`], because most text is clean and this runs on every drawn line.
/// Note that an ASCII-only test is not the fast path: `\t`, `\r` and the C0 controls are ASCII and
/// are exactly what this looks for.
///
/// The byte scan first, and it is a sound over-approximation: every character [`substitute`] can
/// name has either a control byte or a UTF-8 lead byte of `0xC2` or higher (C1 controls lead with
/// `C2`/`C3`, and the bidi and zero-width controls all lead with `E2`). Clean printable ASCII —
/// which is what a source file mostly is — leaves after one vectorised scan and never looks at a
/// character.
#[must_use]
pub fn needs_sanitizing(text: &str) -> bool {
    text.as_bytes()
        .iter()
        .any(|&byte| byte < 0x20 || byte == 0x7f || byte >= 0xc2)
        && text.chars().any(|ch| substitute(ch).is_some())
}

/// Replace every character that may not be drawn as itself with a visible placeholder.
///
/// Width-preserving by construction: each substitution costs exactly one cell, which is what
/// [`width`] counts for it, so a sanitized string is the same width as the padded column it was
/// measured for.
#[must_use]
pub fn sanitize(text: &str) -> String {
    if !needs_sanitizing(text) {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match substitute(ch) {
            Some(replacement) => out.push(replacement),
            None => out.push(ch),
        }
    }
    out
}

/// Cells `text` occupies: two for a wide ideograph, zero for a combining mark, one for everything
/// that had no width and was substituted for.
///
/// Summing the clusters is the same sum as summing the characters — a cluster is only ever cut
/// between [`cut`]'s boundaries — but it is phrased per cluster because that is the unit a
/// terminal advances by, and it is where a future `unicode-width` that composes emoji sequences
/// would change the answer.
#[must_use]
pub fn width(text: &str) -> usize {
    if text.is_ascii() {
        // Every ASCII character costs exactly one cell under the rules above — control characters
        // included, because they were substituted for. This is the hot path (every padded column
        // in every frame asks it), so it gets the branch that avoids the table.
        return text.len();
    }
    text.chars()
        .map(|ch| substitute(ch).map_or_else(|| ch.width().unwrap_or(0), |_| 1))
        .sum()
}

/// Cells one grapheme cluster costs. Same arithmetic as [`width`], exposed for a caller that is
/// already walking clusters.
#[must_use]
pub fn cluster_width(cluster: &str) -> usize {
    cluster
        .chars()
        .map(|ch| substitute(ch).map_or_else(|| ch.width().unwrap_or(0), |_| 1))
        .sum()
}

/// The byte offset to cut `text` at so that the text *plus an ellipsis* fits `cells`.
///
/// `None` means nothing was clipped. The offset is always a grapheme-cluster boundary, which is the
/// whole point: cutting inside `e` + COMBINING ACUTE ACCENT, or inside a ZWJ emoji family, does not
/// produce a shorter cluster — it produces a different character and an ellipsis one cell early.
///
/// This measures `text` as it is; sanitizing is the caller's job and the model layers do it on the
/// way in ([`crate::timeline::episode::Episode::sanitize`], `diff::expand`, `diff::unquote`), so a
/// byte offset here is an offset into the same bytes the caller holds.
#[must_use]
pub fn cut(text: &str, cells: usize) -> Option<usize> {
    if width(text) <= cells {
        return None;
    }
    if cells == 0 {
        return Some(0);
    }
    // Reserve one cell for the ellipsis, then take whole clusters while they still fit. A cluster
    // too wide for what is left is dropped whole: a half emoji is not a smaller emoji, it is a
    // different character, and the reviewer would be quoted a `?` nobody wrote.
    let mut used = 0_usize;
    let mut last = 0_usize;
    for (start, cluster) in text.grapheme_indices(true) {
        let cells_wide = cluster_width(cluster);
        if used + cells_wide + 1 > cells {
            break;
        }
        used += cells_wide;
        last = start + cluster.len();
    }
    Some(last)
}

/// Clip to `cells` cells, adding `…` if anything was dropped, and sanitize on the way.
#[must_use]
pub fn clip(text: &str, cells: usize) -> String {
    let owned;
    let clean = if needs_sanitizing(text) {
        owned = sanitize(text);
        owned.as_str()
    } else {
        text
    };
    match cut(clean, cells) {
        None => clean.to_owned(),
        Some(cut_at) => {
            let mut out = clean[..cut_at].to_owned();
            out.push(ELLIPSIS);
            out
        }
    }
}

/// Clip to `cells` and pad with blanks to exactly `cells`, so every column lines up.
///
/// This is what every drawn cell passes through, which is what makes "did this clip?" a string
/// comparison instead of a screenshot review.
#[must_use]
pub fn fit(text: &str, cells: usize) -> String {
    if cells == 0 {
        return String::new();
    }
    let mut out = clip(text, cells);
    let pad = cells.saturating_sub(width(&out));
    out.push_str(&" ".repeat(pad));
    out
}

/// The trailing blanks off a line, for `--plain`: a text surface has no columns to line up.
#[must_use]
pub fn trim_line(text: &str) -> String {
    text.trim_end().to_owned()
}
