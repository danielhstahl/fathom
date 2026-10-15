//! Version ordering and range satisfaction, and the honesty that comes with both (e2.6 / reco-kvs).
//!
//! The delta model has to answer two questions that look trivial and are not: *did this version go up or
//! down*, and *does this resolved version satisfy the range that asked for it*. Every ecosystem spells
//! versions and ranges differently, and fathom reads files rather than running resolvers, so the only
//! defensible answer is a comparison that says *unknown* when it cannot see its way to one.
//!
//! Three rules:
//!
//! 1. **A direction, not a diff.** [`direction`] answers "upgrade, downgrade, or can't tell". A git commit
//!    pinned where a version belongs is not orderable, so it answers *can't tell* rather than inventing an
//!    ordering out of a hash. A silent "up" would turn an unorderable pair into a fake upgrade.
//! 2. **Three-valued satisfaction.** [`satisfies`] returns `yes`, `no` or `unknown`. `unknown` is not a
//!    failure: `${spring.version}`, `workspace:*`, `file:../shared` and a PEP 508 marker are all real
//!    range text this module refuses to guess at. A *conflict* may only be claimed on `no`, which is what
//!    keeps a peer-dependency warning from being a false accusation.
//! 3. **The grammar is a list, not a vibe.** Caret, tilde (`~` and PEP 440's `~=`), comparators with
//!    optional whitespace, `||` alternatives, `x`/`*` wildcards, Maven/Ivy intervals (`[1.2,2.0)`,
//!    `(,1.5]`), and bare exact versions. Anything else is `unknown`, deliberately and by test.
//!
//! What is *not* here on purpose: any claim about a resolver's own semantics. This answers "does the
//! version the lock chose sit inside the range the file wrote", which is all a reviewer can be told from a
//! file.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};

/// Which way a version moved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// The new version is greater than the old one.
    Up,
    /// The new version is less than the old one — the one a reviewer should see first.
    Down,
    /// Ordered and equal: the same version spelled differently, e.g. `1.2` and `1.2.0`.
    Same,
    /// Not orderable at all — a commit, a revision, a wheel tag, a `${property}`.
    Unknown,
}

impl Direction {
    /// The stable string, which is also the JSON spelling.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Direction::Up => "up",
            Direction::Down => "down",
            Direction::Same => "same",
            Direction::Unknown => "unknown",
        }
    }

    /// Whether a human should read this as a regression risk.
    #[must_use]
    pub const fn is_downgrade(self) -> bool {
        matches!(self, Direction::Down)
    }
}

/// Whether a version satisfies a range — three values, on purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Satisfies {
    /// Proven satisfied.
    Yes,
    /// Proven *not* satisfied. Only this value may raise a conflict.
    No,
    /// Real range text that this module will not guess at.
    Unknown,
}

impl Satisfies {
    /// The stable string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Satisfies::Yes => "yes",
            Satisfies::No => "no",
            Satisfies::Unknown => "unknown",
        }
    }
}

/// A parsed version: the numeric core, plus the prerelease identifiers that sort below its release.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Version {
    numbers: Vec<u64>,
    pre: Vec<String>,
}

impl Version {
    /// Order by the numeric core only, zero-padding the shorter side: `1.2` and `1.2.0` are equal.
    fn cmp_numbers(&self, bound: &[u64]) -> Ordering {
        let width = self.numbers.len().max(bound.len());
        for index in 0..width {
            let left = self.numbers.get(index).copied().unwrap_or(0);
            let right = bound.get(index).copied().unwrap_or(0);
            if left != right {
                return left.cmp(&right);
            }
        }
        Ordering::Equal
    }

    /// Is this version at least `bound`, where a prerelease precedes the release it previews?
    fn at_least(&self, bound: &Self) -> bool {
        match self.cmp_numbers(&bound.numbers) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => {
                // `1.2.3` ≥ `1.2.3`, `1.2.3` ≥ `1.2.3-rc1`, but `1.2.3-rc1` < `1.2.3`.
                if bound.pre.is_empty() {
                    self.pre.is_empty()
                } else {
                    true
                }
            }
        }
    }

    /// Compare the prerelease identifiers of two versions with equal numeric cores.
    fn cmp_pre(&self, other: &Self) -> Ordering {
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => Ordering::Equal,
            (true, false) => Ordering::Greater,
            (false, true) => Ordering::Less,
            (false, false) => {
                for (left, right) in self.pre.iter().zip(other.pre.iter()) {
                    let order = match (left.parse::<u64>(), right.parse::<u64>()) {
                        (Ok(left), Ok(right)) => left.cmp(&right),
                        // A numeric identifier sorts below an alphanumeric one, as in semver §11.
                        (Ok(_), Err(_)) => Ordering::Less,
                        (Err(_), Ok(_)) => Ordering::Greater,
                        (Err(_), Err(_)) => left.cmp(right),
                    };
                    if order != Ordering::Equal {
                        return order;
                    }
                }
                self.pre.len().cmp(&other.pre.len())
            }
        }
    }
}

/// Parse a version: `v1.2.3`, `1.2`, `1.2.3-rc.1`, `1.2.3+build.9`.
///
/// `None` means "not a version this comparison can order", which includes a commit hash, a Python local
/// version with a non-numeric local part, a `1.0.0rc1` with no separator, and a Gradle `${property}`.
fn parse(text: &str) -> Option<Version> {
    let text = text.trim().trim_start_matches(['v', 'V']);
    if text.is_empty() {
        return None;
    }
    // Build metadata never orders, and a Python local version (`1.2.3+cpu1`) is the same shape.
    let core = text.split('+').next()?;
    let (core, pre) = match core.split_once('-') {
        Some((core, pre)) => (core, Some(pre)),
        None => (core, None),
    };
    if core.is_empty() {
        return None;
    }
    let mut numbers = Vec::with_capacity(4);
    for part in core.split('.') {
        if part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        numbers.push(part.parse::<u64>().ok()?);
    }
    Some(Version {
        numbers,
        pre: pre
            .map(|pre| pre.split('.').map(str::to_owned).collect())
            .unwrap_or_default(),
    })
}

/// Order two version strings. [`None`] means "not orderable", never "equal".
#[must_use]
pub fn compare(left: &str, right: &str) -> Option<Ordering> {
    let left = parse(left)?;
    let right = parse(right)?;
    Some(
        left.cmp_numbers(&right.numbers)
            .then_with(|| left.cmp_pre(&right)),
    )
}

/// Which way `from` → `to` moved. An unorderable pair is [`Direction::Unknown`], not a silent "up".
#[must_use]
pub fn direction(from: &str, to: &str) -> Direction {
    match compare(from, to) {
        Some(Ordering::Greater) => Direction::Down,
        Some(Ordering::Less) => Direction::Up,
        Some(Ordering::Equal) => Direction::Same,
        None => Direction::Unknown,
    }
}

/// Does `version` satisfy `range`, in whatever spelling the file used?
#[must_use]
pub fn satisfies(range: &str, version: &str) -> Satisfies {
    let range = range.trim();
    if range.is_empty()
        || matches!(range, "*" | "x" | "X")
        || range.eq_ignore_ascii_case("latest")
        || range.eq_ignore_ascii_case("any")
    {
        return Satisfies::Yes;
    }
    let Some(version) = parse(version) else {
        return Satisfies::Unknown;
    };
    // A PEP 508 or Gemfile environment marker is not part of the version test, and this module does not
    // know the environment, so the range as a whole is unknowable rather than half-tested.
    if range.contains(';') {
        return Satisfies::Unknown;
    }
    let mut unknown = false;
    for alternative in range.split("||") {
        match conjunction(alternative.trim(), &version) {
            Satisfies::Yes => return Satisfies::Yes,
            Satisfies::Unknown => unknown = true,
            Satisfies::No => {}
        }
    }
    if unknown {
        Satisfies::Unknown
    } else {
        Satisfies::No
    }
}

/// Every comparator in one `||`-free alternative has to hold.
fn conjunction(text: &str, version: &Version) -> Satisfies {
    if text.is_empty() {
        return Satisfies::Unknown;
    }
    // Maven / Ivy intervals: `[1.2,2.0)`, `(,1.5]`, `[1.2]`, `[1.2,)`.
    if text.starts_with(['[', '(']) && text.ends_with([']', ')']) {
        return interval(text, version);
    }
    let comparators = split_comparators(text);
    if comparators.is_empty() {
        return Satisfies::Unknown;
    }
    let mut unknown = false;
    let mut held = 0usize;
    for comparator in &comparators {
        match comparator_match(comparator, version) {
            Satisfies::Yes => held += 1,
            Satisfies::Unknown => unknown = true,
            Satisfies::No => {}
        }
    }
    if held == comparators.len() {
        return Satisfies::Yes;
    }
    if unknown {
        Satisfies::Unknown
    } else {
        Satisfies::No
    }
}

/// Split a conjunction into comparators, joining a separated operator (`>= 1.2`) with its bound.
fn split_comparators(text: &str) -> Vec<String> {
    let raw: Vec<&str> = text
        .split([' ', '\t', ','])
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .collect();
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    let mut index = 0;
    while index < raw.len() {
        let token = raw[index];
        let separated = matches!(
            token,
            ">=" | "<=" | "==" | ">" | "<" | "=" | "~" | "~>" | "^"
        );
        if separated && let Some(next) = raw.get(index + 1) {
            out.push(format!("{token}{next}"));
            index += 2;
            continue;
        }
        out.push(token.to_owned());
        index += 1;
    }
    out
}

/// One comparator: `^1.2`, `~1.2.3`, `~=1.4.5`, `>=1.2`, `1.2.x`, `1.2.3`.
fn comparator_match(text: &str, version: &Version) -> Satisfies {
    if let Some(base) = text.strip_prefix('^') {
        return caret(base, version);
    }
    // `~` (npm, cargo) and `~>`/`~=` (Ruby, PEP 440) are *not* the same rule: `~1.2` stops at 1.3.0 while
    // `~> 1.2` stops at 2.0. Reading one as the other silently narrows or widens half the corpus.
    for prefix in ["~=", "~>", "~"] {
        if let Some(base) = text.strip_prefix(prefix) {
            return tilde(base, version, prefix != "~");
        }
    }
    // Longest operator first: `>=` has to be tried before `>` or `>=1.2` becomes a `>` of `=1.2`.
    for operator in [">=", "<=", "==", ">", "<", "="] {
        if let Some(base) = text.strip_prefix(operator) {
            let Some(bound) = parse(base) else {
                return Satisfies::Unknown;
            };
            let order = version
                .cmp_numbers(&bound.numbers)
                .then_with(|| version.cmp_pre(&bound));
            let ok = match operator {
                ">=" => order != Ordering::Less,
                "<=" => order != Ordering::Greater,
                ">" => order == Ordering::Greater,
                "<" => order == Ordering::Less,
                _ => order == Ordering::Equal,
            };
            return if ok { Satisfies::Yes } else { Satisfies::No };
        }
    }
    // A wildcard range: `1.2.x`, `1.x`, `2.*`.
    if text.ends_with(".x")
        || text.ends_with(".X")
        || text.ends_with(".*")
        || matches!(text, "x" | "X" | "*")
    {
        return wildcard(text, version);
    }
    // A bare version is an exact match, and one that is not version-shaped is unknowable rather than
    // silently unequal: `org.example:lib` and `1.2.3` do not disagree, they are different questions.
    let Some(bound) = parse(text) else {
        return Satisfies::Unknown;
    };
    if version.cmp_numbers(&bound.numbers) != Ordering::Equal {
        return Satisfies::No;
    }
    if version.cmp_pre(&bound) != Ordering::Equal {
        return Satisfies::No;
    }
    Satisfies::Yes
}

/// `^1.2.3` — at least this, less than the next breaking release (npm's and cargo's shared rule).
fn caret(base: &str, version: &Version) -> Satisfies {
    let Some(bound) = parse(base) else {
        return Satisfies::Unknown;
    };
    if !version.at_least(&bound) {
        return Satisfies::No;
    }
    let upper = match bound.numbers.as_slice() {
        [] => return Satisfies::Unknown,
        [major] => vec![major + 1],
        // `^0.x` and `^0.0.x` pin tighter than the major, because a zero major promises nothing.
        [0, minor] => vec![0, minor + 1],
        [0, 0, patch] => vec![0, 0, patch + 1],
        [0, minor, ..] if *minor > 0 => vec![0, minor + 1],
        [0, 0, ..] => vec![0, 0, 1],
        [major, ..] => vec![major + 1],
    };
    if version.cmp_numbers(&upper) != Ordering::Less {
        return Satisfies::No;
    }
    Satisfies::Yes
}

/// `~1.2.3` — at least this, less than the next minor; `~1` less than the next major.
///
/// `ruby_style` asks for the `~>`/`~=` rule instead, which releases the *last spelled* segment rather than
/// the second-to-last one: `~> 1.2` means `>= 1.2, < 2.0`.
fn tilde(base: &str, version: &Version, ruby_style: bool) -> Satisfies {
    let Some(bound) = parse(base) else {
        return Satisfies::Unknown;
    };
    if !version.at_least(&bound) {
        return Satisfies::No;
    }
    let upper = if ruby_style {
        // Release the last spelled segment, then bump what is left: `~> 1.2.3` and `~=1.4.5` stop at the
        // next minor, `~> 1.2` at the next major.
        let mut upper = bound.numbers.clone();
        if upper.len() > 1 {
            upper.pop();
        }
        if let Some(last) = upper.last_mut() {
            *last += 1;
        }
        upper
    } else {
        match bound.numbers.as_slice() {
            [] => return Satisfies::Unknown,
            [major] => vec![major + 1],
            [major, minor, ..] => vec![*major, minor + 1],
        }
    };
    if version.cmp_numbers(&upper) != Ordering::Less {
        return Satisfies::No;
    }
    Satisfies::Yes
}

/// `1.2.x` — anything whose first spelled segments match the stem.
fn wildcard(text: &str, version: &Version) -> Satisfies {
    let stem = [".x", ".X", ".*"]
        .iter()
        .find_map(|suffix| text.strip_suffix(*suffix))
        .unwrap_or_else(|| text.trim_end_matches(['x', 'X']));
    if stem.is_empty() {
        return Satisfies::Yes;
    }
    let Some(bound) = parse(stem) else {
        return Satisfies::Unknown;
    };
    if version.cmp_numbers(&bound.numbers) == Ordering::Less {
        return Satisfies::No;
    }
    let mut upper = bound.numbers.clone();
    if let Some(last) = upper.last_mut() {
        *last += 1;
    }
    if version.cmp_numbers(&upper) != Ordering::Less {
        return Satisfies::No;
    }
    Satisfies::Yes
}

/// A Maven / Ivy interval: `[1.2,2.0)`, `(,1.5]`, `[1.2]`.
fn interval(text: &str, version: &Version) -> Satisfies {
    let Some(inner) = text
        .strip_prefix(['[', '('])
        .and_then(|inner| inner.strip_suffix([']', ')']))
    else {
        return Satisfies::Unknown;
    };
    let inclusive_low = text.starts_with('[');
    let inclusive_high = text.ends_with(']');
    let mut parts = inner.split(',');
    let low = parts.next().unwrap_or_default().trim();
    let high = parts.next().map_or("", str::trim);
    if parts.next().is_some() {
        // More than one comma is not an interval this reader knows.
        return Satisfies::Unknown;
    }
    if !low.is_empty() {
        let Some(bound) = parse(low) else {
            return Satisfies::Unknown;
        };
        let order = version.cmp_numbers(&bound.numbers);
        if order == Ordering::Less || (order == Ordering::Equal && !inclusive_low) {
            return Satisfies::No;
        }
    }
    if !high.is_empty() {
        let Some(bound) = parse(high) else {
            return Satisfies::Unknown;
        };
        let order = version.cmp_numbers(&bound.numbers);
        if order == Ordering::Greater || (order == Ordering::Equal && !inclusive_high) {
            return Satisfies::No;
        }
    }
    Satisfies::Yes
}
